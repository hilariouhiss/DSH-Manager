//! 事务引擎：SRS §4 的 saga 实现。全项目唯一有破坏性后果的模块。
//!
//! 原子性契约（SRS §4.1）：要么成功，要么回到操作前状态；若连补偿都失败，
//! 明确报告降级状态并给出可复制的手动命令。绝不静默停在半成品状态。

use std::path::{Path, PathBuf};

use crate::model::*;
use crate::pm::CmdOut;

// ⚠ 下面【两个】只被 `#[cfg(test)]` 的 FakeBackend 使用。不加 gate 的话，
// `cargo build`（非 test 构建）会因它们未被引用而报 unused_imports ——
// 那是独立 lint，`allow(dead_code)` 不覆盖它。
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
// ⚠ `use crate::pm;` 【不能】加 #[cfg(test)] 门 ——
// precheck 是生产代码，它调用 pm::same_dir（TR-3 的 PATH 检查）。
// 早先把它 gate 掉会让 `cargo build` 直接 E0433（unresolved module）。
// RefCell / HashMap 不同：它们确实只被 FakeBackend 使用，必须保留门。
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
    // TR-2：目标 PM 可用。判据与 `pm::probe_pm` **逐字一致**：命令跑不起来（Err）
    // **或退出码非零**都算不可用。
    //
    // ⚠ 早先这里只判 `is_err()`，于是 `precheck` 与 `probe_pm` 对同一个 PM 可能给出
    // 相反结论：一个 `--version` 会失败（退出码非零）的 PM 在 UI 里根本不出现，
    // 却能被 `Target` 手工选中并通过前置检查 —— TR-2 在两处含义不同。
    // 现在两处都要求 `code == 0`，"PM 可用"只有一种含义。
    match b.run(target.pm, &["--version".to_string()]) {
        Ok(out) if out.code == 0 => {}
        _ => return Some(RejectReason::PmUnavailable(target.pm)),
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
    /// 哪些 PM 的 `--version` 会以**非零码**退出（命令跑得起来，但 PM 不可用）。
    ///
    /// TR-2 的判别性开关：只有 `Err` 才算不可用的实现会在
    /// `tr2_precheck_rejects_pm_whose_version_command_exits_nonzero` 上失败。
    pub fail_version: RefCell<Vec<Pm>>,
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
            fail_version: RefCell::new(vec![]),
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
        if args.iter().any(|a| a == "--version") && self.fail_version.borrow().contains(&pm) {
            // "命令跑起来了，但 PM 不工作" —— 真实世界对应一个损坏的 PM 安装。
            return Ok(CmdOut { code: 1, stdout: String::new(), stderr: "模拟 PM 不可用".into() });
        }

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

pub fn install<B: Backend>(b: &B, pm: Pm, version: &Version) -> Result<(), String> {
    let args = pm.install_args(&version.to_string());
    b.log(&format!("$ {} {}", pm.exe(), args.join(" ")));
    let out = b.run(pm, &args)?;
    if out.code != 0 {
        return Err(format!("{} 退出码 {}: {}", pm.label(), out.code, out.stderr.trim()));
    }
    Ok(())
}

pub fn uninstall<B: Backend>(b: &B, pm: Pm) -> Result<(), String> {
    let args = pm.uninstall_args();
    b.log(&format!("$ {} {}", pm.exe(), args.join(" ")));
    let out = b.run(pm, &args)?;
    if out.code != 0 {
        return Err(format!("{} 退出码 {}: {}", pm.label(), out.code, out.stderr.trim()));
    }
    Ok(())
}

/// SRS §4.2.2 主流程。
pub fn run<B: Backend>(b: &B, origin: Origin, target: Target) -> TxOutcome {
    // ═══ 前置检查：任一条不通过即拒绝，零副作用 ═══
    if let Some(reason) = precheck(b, &target) {
        return TxOutcome::Rejected { reason };
    }

    // ═══ S1 安装新版本 ═══
    // 顺序关键：先装后卸（TR-6）。最坏结果是"多一份"，而不是"一份都没有"。
    if let Err(e) = install(b, target.pm, &target.version) {
        return compensate(b, origin, target, TxStep::S1Install, e);
    }

    // ═══ S2 验证新安装 ═══
    // TR-4：**绝不走 PATH**。用路径上的版本验证会在 pnpm→npm 迁移时假通过。
    let target_dir = match b.bin_dir(target.pm) {
        Ok(d) => d,
        Err(e) => return compensate(b, origin, target, TxStep::S2Verify, e),
    };
    // ⚠ 只读一次：第二次读数会再执行一遍 shim，且两者若不一致，
    // detail 里报的"实际"就不是触发本次失败的那个值。
    let got = b.dsh_version_at(&target_dir);
    if got.as_ref() != Some(&target.version) {
        // ⚠ format! 也不能直接写在实参位置：实参从左到右求值，`target` 会先被
        // 移动进 compensate（E0382），而这里还要读它的 version。故先求值再传。
        let detail = format!("新安装的版本不符：期望 {}，实际 {got:?}", target.version);
        return compensate(b, origin, target, TxStep::S2Verify, detail);
    }

    // ═══ S3 卸载旧 PM（同 PM 时跳过 —— FR-13）═══
    if target.pm != origin.pm {
        if let Err(e) = uninstall(b, origin.pm) {
            return compensate(b, origin, target, TxStep::S3Uninstall, e);
        }
    }

    // ═══ S4 最终验证（经 PATH）═══
    match b.dsh_version_on_path() {
        Some((pm, ver)) if pm == target.pm && ver == target.version => {
            TxOutcome::Committed { pm: target.pm, version: target.version }
        }
        other => compensate(
            b,
            origin,
            target,
            TxStep::S4VerifyFinal,
            format!("最终验证不符：{other:?}"),
        ),
    }
}

/// 手动恢复命令。SRS §4.1 / §4.2.3 C3 要求降级报告给出**可直接复制执行**的命令。
///
/// ⚠ 同 PM（`origin.pm == target.pm`）时**只给重装那一条**。此时两条命令作用于
/// 同一个包：照做会先把 origin 版本装回去、再把它删掉，结果是"一份都没有" ——
/// 正是 TR-5 / TR-6 存在的意义所在。
pub fn manual_commands(origin: &Origin, target: &Target) -> Vec<String> {
    let mut cmds = vec![format!(
        "{} {}",
        origin.pm.exe(),
        origin.pm.install_args(&origin.version.to_string()).join(" ")
    )];
    if target.pm != origin.pm {
        cmds.push(format!("{} {}", target.pm.exe(), target.pm.uninstall_args().join(" ")));
    }
    cmds
}

/// SRS §4.2.3 补偿流程。
///
/// 契约（§4.1）：要么回到操作前状态，要么明确报告降级并给出手动命令。
fn compensate<B: Backend>(
    b: &B,
    origin: Origin,
    target: Target,
    failed: TxStep,
    detail: String,
) -> TxOutcome {
    let origin_dir = b.bin_dir(origin.pm).ok();

    // ═══ C1 先探测 origin 是否完好（TR-5）═══
    // 关键：在动任何东西之前先看现状。若 origin 已被 S3 破坏，盲目卸载
    // target 会把两边都毁掉 —— 那是比"操作失败"严重得多的事故。
    let origin_ok = origin_dir
        .as_ref()
        .and_then(|d| b.dsh_version_at(d))
        .is_some_and(|ver| ver == origin.version);

    // ═══ C2b origin 损坏 → 先修复 origin ═══
    if !origin_ok {
        b.log("补偿：origin 已损坏，尝试重装");
        // ⚠ 不能只 `is_err()`：`install` 的 Err 里带着退出码与 stderr，
        // 那是"为什么降级"唯一的诊断信息。丢掉它，用户只会看到主流程的原因。
        if let Err(e) = install(b, origin.pm, &origin.version) {
            b.log(&format!("补偿：重装 origin 失败（{e}），进入降级"));
            return degraded(&origin, &target, failed, format!("{detail}；补偿失败：{e}"));
        }
    }

    // ═══ C2a 清理 target 残留 —— 【先探测再动作】（TR-11）═══
    // 通用规则：补偿中的每个动作都先确认目标状态是否存在。
    // 若 target 上根本没有 dsh（S1 失败时会自然发生），卸载命令会非零退出，
    // 从而被误判为补偿失败并错误报告 Degraded —— 让用户以为环境坏了。
    if target.pm != origin.pm {
        let present = b
            .bin_dir(target.pm)
            .ok()
            .and_then(|d| b.dsh_version_at(&d))
            .is_some();
        if present {
            b.log("补偿：清理 target 残留");
            // 同上：清理失败的原因（退出码 + stderr）必须带进降级报告。
            if let Err(e) = uninstall(b, target.pm) {
                b.log(&format!("补偿：清理失败（{e}），进入降级"));
                return degraded(&origin, &target, failed, format!("{detail}；补偿失败：{e}"));
            }
        } else {
            b.log("补偿：target 上无残留，跳过卸载");
        }
    }

    // ═══ C3 确认确实回到了 origin ═══
    let restored = origin_dir
        .as_ref()
        .and_then(|d| b.dsh_version_at(d))
        .is_some_and(|ver| ver == origin.version);

    if restored {
        TxOutcome::RolledBack { failed, restored: origin, detail }
    } else {
        degraded(&origin, &target, failed, detail)
    }
}

fn degraded(origin: &Origin, target: &Target, failed: TxStep, reason: String) -> TxOutcome {
    TxOutcome::Degraded {
        failed,
        reason,
        manual: manual_commands(origin, target),
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
        // ⚠ 必须 `let mut b` —— bins 是普通 HashMap（不是 RefCell），就地插入需要可变绑定
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

    /// ★ TR-2：`precheck` 的"PM 可用"判据必须与 `pm::probe_pm` **相同** ——
    /// 退出码非零也算不可用。
    ///
    /// 判别性：把 `precheck` 退回 `b.run(..).is_err()`，本测试立刻失败 ——
    /// 那时一个"命令在但会失败"的 PM 能通过前置检查，却在 UI 的下拉里根本不存在。
    #[test]
    fn tr2_precheck_rejects_pm_whose_version_command_exits_nonzero() {
        let b = FakeBackend::new();
        b.fail_version.borrow_mut().push(Pm::Pnpm);
        let t = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        assert_eq!(
            precheck(&b, &t),
            Some(RejectReason::PmUnavailable(Pm::Pnpm)),
            "退出码非零 → 不可用（与 pm::probe_pm 同判据）"
        );
        // 同一次运行里，正常的 PM 不受影响
        assert_eq!(precheck(&b, &Target { pm: Pm::Npm, version: v("0.1.6-alpha.2") }), None);
    }

    /// ⚠ 本测试**不覆盖** NFR-6 的拒绝路径 —— 真正覆盖它的是
    /// `safe_version_rejects_injection_attempts`（`is_safe_version` 的字符集校验）。
    ///
    /// 这里钉住的是**另一件事**：`Target.version` 的类型是 `semver::Version`，
    /// 它**在类型上就不可能**表示带注入字符的版本号，因此 `precheck` 里那条
    /// `RejectReason::InvalidVersion` 分支经 `Target` **不可达**。
    /// （早先这条测试叫 `precheck_rejects_unsafe_version`，却把同一个合法版本断了两次 ——
    /// 名字承诺了它没有的覆盖，`docs/VERIFICATION.md` 的 V-16 还据此宣称"NFR-6 逐条覆盖"。）
    ///
    /// `is_safe_version` 与 `InvalidVersion` 都**照旧保留**：它们是 NFR-6 的具名实现
    /// （Ruling 39），只是其运行期不可达这件事必须写在测试与文档里，而不是假装测到了。
    #[test]
    fn precheck_invalid_version_branch_is_unreachable_by_type() {
        let b = FakeBackend::new();
        let t = Target { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        assert_eq!(precheck(&b, &t), None, "合法版本应通过");

        // 本测试能钉住的上限：`Version` 的 Display 形式**必然**满足字符集校验 ——
        // 两处判据不会互相矛盾。若有人把 is_safe_version 改严（例如禁掉 '+' 或预发布段），
        // 这条断言会失败：那时 precheck 的 InvalidVersion 分支就不再是"类型上不可达"，
        // 而是会真的拒绝合法的 semver。
        for s in ["0.1.6-alpha.2", "1.0.0+build.5", "0.1.0", "1.2.3-rc.1+b.2"] {
            let parsed = v(s);
            assert!(
                is_safe_version(&parsed.to_string()),
                "{parsed} 是合法 semver，字符串形式必须通过 is_safe_version"
            );
        }
    }

    #[test]
    fn precheck_passes_for_valid_target() {
        let b = FakeBackend::new();
        let t = Target { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        assert_eq!(precheck(&b, &t), None);
    }

    /// ★ 假 backend 的语义本身必须被钉住 —— 整张事务测试网都建在它上面。
    ///
    /// 实测依据（Task 8 实现者的变异测试）：把 `run` 里的安装副作用删掉
    /// （即"安装返回成功但什么也没写" —— **正是本计划最初的那个 bug**），
    /// Task 8 的其余 6 个测试**全部照样通过**。只有 Task 9 的
    /// `cross_pm_migration_installs_then_uninstalls` 会变红。
    ///
    /// 这很危险：假 backend 若被后来者"简化"掉副作用，依赖它的**八个以上**
    /// 事务测试会**集体变成空转**（`Committed` 这条成功路径更是永远走不到），
    /// 而没有一个测试会报错。所以语义必须在这里直接断言，而不是靠下游间接覆盖。
    #[test]
    fn fake_backend_semantics_are_modeled() {
        // 1) 安装成功 → 写入从包规格解析出的版本
        let b = FakeBackend::new();
        b.run(Pm::Pnpm, &Pm::Pnpm.install_args("9.9.9-tr4probe")).unwrap();
        assert_eq!(
            b.dsh_version_at(&PathBuf::from("C:/pnpm")),
            Some(v("9.9.9-tr4probe")),
            "安装成功后必须写入 installed —— 否则 S2 永远无法通过，Committed 路径不可测"
        );

        // 2) 卸载成功 → 清空
        b.run(Pm::Pnpm, &Pm::Pnpm.uninstall_args()).unwrap();
        assert_eq!(b.dsh_version_at(&PathBuf::from("C:/pnpm")), None, "卸载后必须清空");

        // 3) install_noop：返回成功但什么也不写
        let c = FakeBackend::new();
        c.install_noop.borrow_mut().push(Pm::Pnpm);
        let out = c.run(Pm::Pnpm, &Pm::Pnpm.install_args("9.9.9-tr4probe")).unwrap();
        assert_eq!(out.code, 0, "install_noop 必须【成功】返回");
        assert_eq!(c.dsh_version_at(&PathBuf::from("C:/pnpm")), None, "install_noop 不得写入");

        // 4) fail_install：**Ok 但非零码**，且不写入。
        //    注意是 Ok 不是 Err —— 这正是 TR-11 依赖的区分（命令跑起来了 vs 没跑起来）。
        let d = FakeBackend::new();
        d.fail_install.borrow_mut().push(Pm::Pnpm);
        let out = d.run(Pm::Pnpm, &Pm::Pnpm.install_args("9.9.9-tr4probe")).unwrap();
        assert_ne!(out.code, 0, "fail_install 必须以非零码返回");
        assert_eq!(d.dsh_version_at(&PathBuf::from("C:/pnpm")), None, "失败不得写入");

        // 5) path_override 只改 PATH 的答案，不影响目录读取 —— 这是 TR-4 唯一的判别构造
        let e = FakeBackend::new();
        *e.path_override.borrow_mut() = Some((Pm::Pnpm, v("9.9.9-tr4probe")));
        assert_eq!(e.dsh_version_on_path(), Some((Pm::Pnpm, v("9.9.9-tr4probe"))));
        assert_eq!(
            e.dsh_version_at(&PathBuf::from("C:/pnpm")),
            None,
            "path_override 不得影响 dsh_version_at —— 否则 TR-4 的判别性就没了"
        );

        // 6) unavailable → Err（命令没跑起来），与"非零码"是两回事
        let f = FakeBackend::new();
        f.unavailable.borrow_mut().push(Pm::Pnpm);
        assert!(f.run(Pm::Pnpm, &["--version".to_string()]).is_err(), "不可用必须是 Err");
    }

    #[test]
    fn same_pm_version_change_commits_and_skips_uninstall() {
        // FR-13：同 PM 换版本时 S3 必须自动跳过
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.1") };
        let target = Target { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(matches!(out, TxOutcome::Committed { pm: Pm::Npm, .. }), "得到 {out:?}");
        assert!(!b.called("uninstall"), "同 PM 时不得调用卸载");
    }

    #[test]
    fn cross_pm_migration_installs_then_uninstalls() {
        // TR-6：先装后卸
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(matches!(out, TxOutcome::Committed { pm: Pm::Pnpm, .. }), "得到 {out:?}");

        let calls = b.calls.borrow().clone();
        let i_install = calls.iter().position(|c| c.contains("Pnpm") && c.contains("add"))
            .expect("应有 pnpm add");
        let i_uninstall = calls.iter().position(|c| c.contains("Npm") && c.contains("uninstall"))
            .expect("应有 npm uninstall");
        // 注意：install 之前 precheck 也会调 run(--version)，用 add/uninstall 关键字区分
        assert!(i_install < i_uninstall, "TR-6：必须先装后卸，实际顺序 {calls:?}");
    }

    #[test]
    fn rejected_target_produces_no_side_effects() {
        // TR-3 拒绝时零副作用
        // ⚠ brief 原文是 `let b` —— bins 是普通 HashMap（非 RefCell），
        // 就地 insert 需要可变绑定，否则 E0596。与既有 precheck 测试同因。
        let mut b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        b.bins.insert(Pm::Bun, PathBuf::from("C:/bun/bin"));
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Bun, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(matches!(out, TxOutcome::Rejected { .. }), "得到 {out:?}");
        assert!(!b.called("add"), "被拒绝时不得执行任何安装");
        assert!(!b.called("uninstall"), "被拒绝时不得执行任何卸载");
    }

    /// ★ SRS V-20：S1 失败时零副作用
    #[test]
    fn v20_s1_failure_touches_nothing() {
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        b.fail_install.borrow_mut().push(Pm::Pnpm);
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(matches!(out, TxOutcome::RolledBack { failed: TxStep::S1Install, .. }),
                "得到 {out:?}");
        assert!(!b.called("uninstall"), "S1 失败时不得触碰旧的安装");
    }

    /// ★ SRS V-19 / TR-4：S2 验证**绝不经 PATH**
    ///
    /// 构造方式（两个开关缺一不可）：
    /// - `install_noop = [Pnpm]` —— 安装返回成功但**什么也没装上**，
    ///   所以 `dsh_version_at(Pnpm目录)` 是 None
    /// - `path_override = Some((Pnpm, 目标版本))` —— 强行让 PATH 解析**报告成功**
    ///
    /// 于是：走目录检查 → S2 失败（正确）；走 PATH 检查 → S2 通过（错误）。
    /// **这就是判别条件** —— 只有真正独立于 PATH 的实现才会让本测试通过。
    #[test]
    fn tr4_verify_does_not_use_path() {
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        b.install_noop.borrow_mut().push(Pm::Pnpm);
        *b.path_override.borrow_mut() = Some((Pm::Pnpm, v("0.1.6-alpha.2")));

        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(
            matches!(out, TxOutcome::RolledBack { failed: TxStep::S2Verify, .. }),
            "S2 必须走目录检查并发现自己没装上；若这里得到 Committed，说明实现误用了 PATH 验证。得到 {out:?}"
        );
    }

    /// ★ SRS V-17：S3 失败时补偿生效，状态回到 origin
    #[test]
    fn v17_s3_failure_rolls_back_to_origin() {
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        // 迁移到 pnpm，但 pnpm 装完后旧版本 npm 卸载失败。
        // 关键：npm 上的 origin 版本仍然完好 → 应卸掉 pnpm 残留并回到 npm。
        b.fail_uninstall.borrow_mut().push(Pm::Npm);
        // 让 pnpm 的安装"成功"（写进 installed），这样 S2 能过、走到 S3
        {
            let mut inst = b.installed.borrow_mut();
            inst.insert(Pm::Pnpm, Some(v("0.1.6-alpha.2")));
        }
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        match out {
            TxOutcome::RolledBack { failed, restored, .. } => {
                assert_eq!(failed, TxStep::S3Uninstall);
                assert_eq!(restored.pm, Pm::Npm);
            }
            other => panic!("期望 RolledBack，得到 {other:?}"),
        }
        // 补偿必须清掉 pnpm 上的残留。
        // ⚠ 必须在**同一条记录**里同时出现 "Pnpm" 与 "remove"：分开断言的话
        // `called("Pnpm")` 会被 precheck 的 "Pnpm --version" 恒真满足，等于没断言。
        let calls = b.calls.borrow().clone();
        let pnpm_uninstall = calls.iter().any(|c| c.contains("Pnpm") && c.contains("remove"));
        assert!(pnpm_uninstall, "应清理 pnpm 残留，实际调用 {calls:?}");
    }

    /// ★ SRS V-21：origin 已损坏时【不得】盲目卸载 target —— 否则两边都没了
    #[test]
    fn v21_origin_broken_does_not_blindly_uninstall_target() {
        let b = FakeBackend::new();
        // origin(npm) 上【没有】dsh —— 模拟 S3 已把 origin 弄坏
        // target(pnpm) 装成功了
        {
            let mut inst = b.installed.borrow_mut();
            inst.insert(Pm::Npm, None);
            inst.insert(Pm::Pnpm, Some(v("0.1.6-alpha.2")));
        }
        // 重装 origin 也失败
        b.fail_install.borrow_mut().push(Pm::Npm);
        // 让它走到 S3：S2 对 pnpm 会成功
        b.fail_uninstall.borrow_mut().push(Pm::Npm);

        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(matches!(out, TxOutcome::Degraded { .. }), "得到 {out:?}");

        // 关键断言：不得对 pnpm 执行卸载 —— 否则旧的坏了、新的也没了
        let calls = b.calls.borrow().clone();
        let pnpm_uninstall = calls.iter().any(|c| c.contains("Pnpm") && c.contains("remove"));
        assert!(!pnpm_uninstall, "origin 已损坏时不得卸载 target，实际调用 {calls:?}");
    }

    /// ★ TR-11：补偿中对【不存在】的包执行卸载，其非零退出不得被误判为补偿失败
    #[test]
    fn tr11_compensation_probes_before_acting() {
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        // 关键：让 pnpm 的安装"返回成功但什么也没装上"。
        // 若不用这个开关，安装会真的写入 installed，S2 就会通过、
        // 事务会走到 S3 并成功提交 —— 那时根本不会进入补偿流程，
        // 本测试就测不到 TR-11。
        b.install_noop.borrow_mut().push(Pm::Pnpm);

        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        // S1 成功但 S2 失败（pnpm 上没装成）→ 补偿时 pnpm 上没东西，
        // 应【跳过】卸载而不是执行它并因非零退出而 Degraded
        assert!(matches!(out, TxOutcome::RolledBack { .. }), "得到 {out:?}");
        let calls = b.calls.borrow().clone();
        let pnpm_uninstall = calls.iter().any(|c| c.contains("Pnpm") && c.contains("remove"));
        assert!(!pnpm_uninstall, "TR-11：对不存在的包不应执行卸载，实际 {calls:?}");
    }

    #[test]
    fn degraded_outcome_carries_runnable_manual_commands() {
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let cmds = manual_commands(&origin, &target);
        assert_eq!(cmds.len(), 2);
        assert!(cmds[0].contains("npm.cmd") && cmds[0].contains("install"));
        assert!(cmds[0].contains("@deepseek-ai/dsh@0.1.6-alpha.2"));
        assert!(cmds[1].contains("pnpm.cmd") && cmds[1].contains("remove"));
    }

    /// ★ 同 PM 时手动命令**绝不能**包含删包 —— 否则照做就是自毁。
    ///
    /// 场景可达：同 PM 换版本时 S1 "成功"但把 shim 弄坏了 → S2 失败 →
    /// C1 探测的是**同一个目录**（所以必然也是坏的）→ C2b 重装失败 → Degraded。
    /// 此时若给出"装回 V0 + 卸载 target 包"两条命令，用户照着执行会先把 dsh
    /// 装回来、再把它删干净 —— 正是 TR-5 / TR-6 要避免的"一份都没有"。
    #[test]
    fn manual_commands_same_pm_never_removes_the_only_copy() {
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let cmds = manual_commands(&origin, &target);
        assert_eq!(cmds.len(), 1, "同 PM 时只该给重装那一条，实际 {cmds:?}");
        assert!(cmds[0].contains("npm.cmd") && cmds[0].contains("install"));
        assert!(cmds[0].contains("@deepseek-ai/dsh@0.1.6-alpha.2"));
        assert!(!cmds[0].contains("remove"), "同 PM 时不得给出删包命令，实际 {cmds:?}");
    }

    /// ★ SRS §4.2.3 C3：恢复确认必须**真的探测**，不能假定"补偿动作跑完 = 已恢复"。
    ///
    /// 此前把 C3 换成 `let restored = true;` 全量 49 个测试**照样全绿** ——
    /// 一个未恢复的环境会被报成 `RolledBack`（"已恢复"），而契约要求明确降级。
    ///
    /// 构造：origin(npm) 上没有 dsh，且对 npm 的"重装"返回成功但什么也没装上
    /// （`install_noop`）—— 于是重装这一步**看起来成功了**，只有 C3 的探测
    /// 能发现 origin 依然是空的，从而必须降级。
    #[test]
    fn c3_detects_incomplete_recovery_and_degrades() {
        let b = FakeBackend::new();
        {
            let mut inst = b.installed.borrow_mut();
            inst.insert(Pm::Npm, None);
            inst.insert(Pm::Pnpm, Some(v("0.1.6-alpha.2")));
        }
        // 重装 origin "成功"但什么也没装上 —— 这正是 C3 存在的理由
        b.install_noop.borrow_mut().push(Pm::Npm);
        // 主流程在卸载旧的 npm 时失败，从而进入补偿
        b.fail_uninstall.borrow_mut().push(Pm::Npm);

        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(
            matches!(out, TxOutcome::Degraded { failed: TxStep::S3Uninstall, .. }),
            "C3 必须发现自己并未恢复（重装是 no-op）并明确降级，得到 {out:?}"
        );
    }

    /// ★ C2a 的失败分支此前**无任何测试到达** —— 文件里所有 `fail_uninstall`
    /// 推的都是 origin 的 Npm，所以"清理 target 残留失败"这条路从未被走过。
    ///
    /// 该守卫若被删除，清理失败会穿透到 C3：此时 origin 完好、C3 为真，于是返回
    /// `RolledBack`（界面显示"已恢复"），而 target 上的残留还在 —— 一份**假成功报告**。
    ///
    /// 本测试同时钉住 F1：降级 `reason` 必须真的带上补偿失败的诊断（失败命令的
    /// label + 退出码 + stderr）。把 `if let Err(e)` 退回 `.is_err()` 并原样传
    /// `detail`，这两条 reason 断言就会失败。
    #[test]
    fn c2a_cleanup_failure_is_reported_as_degraded_with_cause() {
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        // S3 卸载 origin 失败 → 进入补偿
        b.fail_uninstall.borrow_mut().push(Pm::Npm);
        // C2a 清理 target 残留也失败 → 必须降级，且带上失败原因
        b.fail_uninstall.borrow_mut().push(Pm::Pnpm);

        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        match out {
            TxOutcome::Degraded { failed, reason, .. } => {
                assert_eq!(failed, TxStep::S3Uninstall);
                assert!(
                    reason.contains("补偿失败"),
                    "降级原因必须说明补偿为何失败，而不是只说主流程的失败；实际 {reason:?}"
                );
                assert!(
                    reason.contains("pnpm"),
                    "降级原因必须带上失败命令的身份（pnpm），实际 {reason:?}"
                );
            }
            other => panic!("期望 Degraded，得到 {other:?}"),
        }
    }

    #[test]
    fn rejected_outcome_is_not_confused_with_rolled_back() {
        let b = FakeBackend::new();
        b.unavailable.borrow_mut().push(Pm::Pnpm);
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        assert!(matches!(run(&b, origin, target), TxOutcome::Rejected { .. }));
    }
}
