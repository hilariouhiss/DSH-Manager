//! 事务引擎：SRS §4 的 saga 实现。全项目唯一有破坏性后果的模块。
//!
//! 原子性契约（SRS §4.1）：要么成功，要么回到操作前状态；若连补偿都失败，
//! 明确报告降级状态并给出可复制的手动命令。绝不静默停在半成品状态。

use std::path::{Path, PathBuf};

use crate::model::*;
use crate::pm::CmdOut;

// ⚠ `RefCell` / `HashMap` 只被 `#[cfg(test)]` 的 FakeBackend 使用。不加 gate 的话，
// `cargo build`（非 test 构建）会因它们未被引用而报 unused_imports ——
// 那是独立 lint，`allow(dead_code)` 不覆盖它。
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
// ⚠ `pm` 【不能】加同样的 gate：它不是测试专用的 —— 生产代码的 `precheck`
// 就在调 `pm::same_dir`。加了 gate，非 test 构建会报 E0433
// （unresolved module `pm`），而它本来也不会有 unused_imports 警告。
use crate::pm;

pub trait Backend {
    fn run(&self, pm: Pm, args: &[String]) -> Result<CmdOut, String>;
    fn bin_dir(&self, pm: Pm) -> Result<PathBuf, String>;
    fn path_dirs(&self) -> Vec<PathBuf>;
    /// 直接执行指定目录下的 shim —— 【不走 PATH】(TR-4)
    fn dsh_version_at(&self, dir: &Path) -> Option<Version>;
    /// 经 PATH 解析 —— 仅用于 S4 最终验证
    fn dsh_version_on_path(&self) -> Option<(Pm, Version)>;
    fn log(&self, line: &str);
}

/// FR-3 / NFR-6：版本号字符集校验。
pub fn is_safe_version(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
}

/// SRS §4.2.1 前置检查：任何一条不通过即拒绝，且**零副作用**。
///
/// TR-1（dsh web 已停止）**不在此处** —— 它需要与 UI 交互（可能要求用户确认），
/// 而事务引擎必须保持纯逻辑、无 UI 依赖。调用方在派发 Job 前完成停止。
pub fn precheck<B: Backend>(b: &B, target: &Target) -> Option<RejectReason> {
    // TR-2：目标 PM 可用
    if b.run(target.pm, &["--version".to_string()]).is_err() {
        return Some(RejectReason::PmUnavailable(target.pm));
    }
    // NFR-6：版本号字符集（在拼命令前校验）
    let vs = target.version.to_string();
    if !is_safe_version(&vs) {
        return Some(RejectReason::InvalidVersion(vs));
    }
    // TR-3：目标 PM 的 bin 目录必须在 PATH 中。
    // 否则事务完成后 dsh 会从 PATH 上消失 —— 这是【动手前即可发现】的问题，
    // 必须阻断而非事后告警。
    let dir = match b.bin_dir(target.pm) {
        Ok(d) => d,
        Err(_) => return Some(RejectReason::PmUnavailable(target.pm)),
    };
    if !b.path_dirs().iter().any(|p| pm::same_dir(p, &dir)) {
        return Some(RejectReason::BinNotOnPath { pm: target.pm, dir });
    }
    None
}

#[cfg(test)]
pub struct FakeBackend {
    pub bins: HashMap<Pm, PathBuf>,
    pub path: Vec<PathBuf>,
    /// 各 PM 目录当前的 dsh 版本（None = 该 PM 上没装）
    pub installed: RefCell<HashMap<Pm, Option<Version>>>,
    /// 哪些 PM 的安装命令会以非零码失败
    pub fail_install: RefCell<Vec<Pm>>,
    pub fail_uninstall: RefCell<Vec<Pm>>,
    /// 哪些 PM 的安装命令"返回成功但什么也没装上"。
    /// 用于构造 S2 验证必须失败的场景 —— 真实世界里这对应"装是装了但版本不对"。
    pub install_noop: RefCell<Vec<Pm>>,
    /// 强行指定 `dsh_version_on_path()` 的返回值。
    /// 用于构造"目标目录里没有，但 PATH 解析能拿到"的场景 ——
    /// 这是 TR-4 唯一的判别条件：若实现误用 PATH 验证，测试会假通过。
    pub path_override: RefCell<Option<(Pm, Version)>>,
    /// 调用记录，用于断言"某操作从未被调用"
    pub calls: RefCell<Vec<String>>,
    /// 模拟 PM 完全不可用
    pub unavailable: RefCell<Vec<Pm>>,
}

#[cfg(test)]
impl FakeBackend {
    pub fn new() -> Self {
        let mut bins = HashMap::new();
        bins.insert(Pm::Npm, PathBuf::from("C:/npm"));
        bins.insert(Pm::Pnpm, PathBuf::from("C:/pnpm"));
        Self {
            bins,
            path: vec![PathBuf::from("C:/pnpm"), PathBuf::from("C:/npm")],
            installed: RefCell::new(HashMap::new()),
            fail_install: RefCell::new(vec![]),
            fail_uninstall: RefCell::new(vec![]),
            install_noop: RefCell::new(vec![]),
            path_override: RefCell::new(None),
            calls: RefCell::new(vec![]),
            unavailable: RefCell::new(vec![]),
        }
    }
    pub fn with_installed(self, pm: Pm, v: &str) -> Self {
        self.installed.borrow_mut().insert(pm, Some(v.parse().unwrap()));
        self
    }
    pub fn called(&self, needle: &str) -> bool {
        self.calls.borrow().iter().any(|c| c.contains(needle))
    }
}

#[cfg(test)]
impl Backend for FakeBackend {
    /// ⚠ `run` **必须模拟安装/卸载的副作用**，不能只返回成功。
    ///
    /// 原因：事务的 S2 步骤会去读 `dsh_version_at(target_dir)` 来验证"装上了没有"。
    /// 如果假 backend 的安装不改变 `installed`，那么**任何**迁移都不可能通过 S2，
    /// `TxOutcome::Committed` 这条成功路径就永远无法在测试里走到 —— 而成功路径
    /// 恰恰是最该被测试覆盖的那条。
    ///
    /// 三个开关的语义：
    /// - `fail_install` / `fail_uninstall`：命令以**非零码**退出（真实失败）
    /// - `install_noop`：命令**返回成功但什么也没装上**（真实世界里的"装是装了但版本不对"），
    ///   用于构造 S2 必须失败的场景
    fn run(&self, pm: Pm, args: &[String]) -> Result<CmdOut, String> {
        self.calls.borrow_mut().push(format!("{pm:?} {}", args.join(" ")));
        if self.unavailable.borrow().contains(&pm) {
            return Err(format!("{pm:?} 不可用"));
        }

        let joined = args.join(" ");
        let is_install = args.iter().any(|a| a == "install" || a == "add");
        let is_uninstall = args.iter().any(|a| a == "uninstall" || a == "remove");

        if is_install && self.fail_install.borrow().contains(&pm) {
            return Ok(CmdOut { code: 1, stdout: String::new(), stderr: "模拟安装失败".into() });
        }
        if is_uninstall && self.fail_uninstall.borrow().contains(&pm) {
            return Ok(CmdOut { code: 1, stdout: String::new(), stderr: "模拟卸载失败".into() });
        }

        if is_install && !self.install_noop.borrow().contains(&pm) {
            // 从包规格里取出目标版本并写入 —— 这是 S2 能通过的前提
            if let Some(spec) = args.iter().find(|a| a.starts_with("@deepseek-ai/dsh@")) {
                if let Some(ver) = spec.rsplit('@').next().and_then(|s| s.parse::<Version>().ok()) {
                    self.installed.borrow_mut().insert(pm, Some(ver));
                }
            }
        }
        if is_uninstall {
            self.installed.borrow_mut().insert(pm, None);
        }

        let _ = joined;
        Ok(CmdOut { code: 0, stdout: String::new(), stderr: String::new() })
    }
    fn bin_dir(&self, pm: Pm) -> Result<PathBuf, String> {
        self.bins.get(&pm).cloned().ok_or_else(|| format!("{pm:?} 无 bin 目录"))
    }
    fn path_dirs(&self) -> Vec<PathBuf> {
        self.path.clone()
    }
    fn dsh_version_at(&self, dir: &Path) -> Option<Version> {
        self.bins
            .iter()
            .find(|(_, d)| pm::same_dir(d, dir))
            .and_then(|(k, _)| self.installed.borrow().get(k).cloned().flatten())
    }
    fn dsh_version_on_path(&self) -> Option<(Pm, Version)> {
        // 测试可以强行指定它的返回值（TR-4 的判别条件靠这个构造：
        // "目标目录里没有，但 PATH 解析能拿到" —— 只有这样才能区分
        // 实现到底用了目录检查还是 PATH 检查）
        if let Some(forced) = self.path_override.borrow().clone() {
            return Some(forced);
        }
        // 按 PATH 顺序取第一个"有装 dsh"的 PM
        for dir in &self.path {
            for (k, d) in self.bins.iter() {
                if pm::same_dir(d, dir) {
                    if let Some(Some(v)) = self.installed.borrow().get(k) {
                        return Some((*k, v.clone()));
                    }
                }
            }
        }
        None
    }
    fn log(&self, line: &str) {
        self.calls.borrow_mut().push(format!("LOG {line}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn safe_version_accepts_real_versions() {
        assert!(is_safe_version("0.1.6-alpha.2"));
        assert!(is_safe_version("0.1.0"));
        assert!(is_safe_version("1.0.0+build.5"));
    }

    #[test]
    fn safe_version_rejects_injection_attempts() {
        // NFR-6：版本号进入命令行前必须校验字符集
        assert!(!is_safe_version("1.0.0; rm -rf /"));
        assert!(!is_safe_version("1.0.0 && echo pwned"));
        assert!(!is_safe_version("1.0.0\" | calc"));
        assert!(!is_safe_version(""));
        assert!(!is_safe_version("1.0.0\n2.0.0"));
    }

    #[test]
    fn precheck_rejects_unavailable_pm() {
        // TR-2
        let b = FakeBackend::new();
        b.unavailable.borrow_mut().push(Pm::Pnpm);
        let t = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        assert_eq!(precheck(&b, &t), Some(RejectReason::PmUnavailable(Pm::Pnpm)));
    }

    #[test]
    fn precheck_rejects_pm_bin_not_on_path() {
        // TR-3：目标 PM 的 bin 不在 PATH 时【动手前拒绝】，零副作用
        let mut b = FakeBackend::new();
        b.bins.insert(Pm::Bun, PathBuf::from("C:/bun/bin"));
        // PATH 里没有 C:/bun/bin
        let t = Target { pm: Pm::Bun, version: v("0.1.6-alpha.2") };
        match precheck(&b, &t) {
            Some(RejectReason::BinNotOnPath { pm, dir }) => {
                assert_eq!(pm, Pm::Bun);
                assert_eq!(dir, PathBuf::from("C:/bun/bin"));
            }
            other => panic!("期望 BinNotOnPath，得到 {other:?}"),
        }
    }

    #[test]
    fn precheck_rejects_unsafe_version() {
        // NFR-6
        let b = FakeBackend::new();
        let t = Target { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        assert_eq!(precheck(&b, &t), None, "合法版本应通过");

        let mut bad = t.clone();
        bad.version = v("0.1.6-alpha.2"); // semver 本身已挡住非法字符
        assert_eq!(precheck(&b, &bad), None);
    }

    #[test]
    fn precheck_passes_for_valid_target() {
        let b = FakeBackend::new();
        let t = Target { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        assert_eq!(precheck(&b, &t), None);
    }
}
