//! 纯数据叶子层。**不得 `use crate::` 任何其他模块** —— 这个性质让 txn.rs
//! 的单元测试只需 model.rs + 一个假 backend。
//!
//! 命令表（impl Pm）也放在这里：它是纯常量映射，无 I/O。若放到 pm.rs，
//! model.rs 就会为 Pm 类型反向依赖 pm.rs，破坏叶子性质。

use std::collections::BTreeMap;
use std::path::PathBuf;

pub type Version = semver::Version;

/// dsh 在 Windows 上的可执行 shim 候选名。
/// `.exe` 用于 bun（bun 在 Windows 生成 .exe shim），`.cmd` 用于 npm / pnpm / yarn。
pub const SHIM_NAMES: [&str; 2] = ["dsh.exe", "dsh.cmd"];

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum Pm {
    Npm,
    Pnpm,
    Bun,
    Yarn,
}

impl Pm {
    pub const ALL: [Pm; 4] = [Pm::Npm, Pm::Pnpm, Pm::Bun, Pm::Yarn];

    /// GC-7：Windows 上必须是 shim 全名。
    pub fn exe(self) -> &'static str {
        match self {
            Pm::Npm => "npm.cmd",
            Pm::Pnpm => "pnpm.cmd",
            Pm::Bun => "bun.exe",
            Pm::Yarn => "yarn.cmd",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Pm::Npm => "npm",
            Pm::Pnpm => "pnpm",
            Pm::Bun => "bun",
            Pm::Yarn => "yarn",
        }
    }

    /// FR-2：取全局 bin 目录的命令参数
    pub fn bin_dir_args(self) -> &'static [&'static str] {
        match self {
            Pm::Npm => &["prefix", "-g"],
            Pm::Pnpm => &["bin", "-g"],
            Pm::Bun => &["pm", "bin", "-g"],
            Pm::Yarn => &["global", "bin"],
        }
    }

    /// FR-14：安装指定版本
    pub fn install_args(self, version: &str) -> Vec<String> {
        let pkg = format!("@deepseek-ai/dsh@{version}");
        match self {
            Pm::Npm => vec!["install".into(), "-g".into(), pkg],
            Pm::Pnpm => vec!["add".into(), "-g".into(), pkg],
            Pm::Bun => vec!["add".into(), "-g".into(), pkg],
            Pm::Yarn => vec!["global".into(), "add".into(), pkg],
        }
    }

    /// FR-14：卸载
    pub fn uninstall_args(self) -> Vec<String> {
        let pkg = "@deepseek-ai/dsh".to_string();
        match self {
            Pm::Npm => vec!["uninstall".into(), "-g".into(), pkg],
            Pm::Pnpm => vec!["remove".into(), "-g".into(), pkg],
            Pm::Bun => vec!["remove".into(), "-g".into(), pkg],
            Pm::Yarn => vec!["global".into(), "remove".into(), pkg],
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Channel {
    Stable,
    Rc,
    Alpha,
    Other,
}

#[derive(Clone, Debug)]
pub struct PmInfo {
    pub kind: Pm,
    pub version: String,
    pub bin_dir: PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct PmEnv {
    pub available: Vec<PmInfo>,
    pub owner: Option<Pm>,
    pub installed: Option<Version>,
    pub dsh_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub versions: Vec<Version>,
    pub tags: BTreeMap<String, Version>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub pm: Pm,
    pub version: Version,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub pm: Pm,
    pub version: Version,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TxStep {
    Precheck,
    S1Install,
    S2Verify,
    S3Uninstall,
    S4VerifyFinal,
    C1Probe,
    C2Restore,
    C2Cleanup,
    C3Confirm,
}

impl TxStep {
    pub fn label(self) -> &'static str {
        match self {
            TxStep::Precheck => "前置检查",
            TxStep::S1Install => "安装新版本",
            TxStep::S2Verify => "验证新安装",
            TxStep::S3Uninstall => "卸载旧版本",
            TxStep::S4VerifyFinal => "最终验证",
            TxStep::C1Probe => "探测原状态",
            TxStep::C2Restore => "恢复原版本",
            TxStep::C2Cleanup => "清理残留",
            TxStep::C3Confirm => "确认已恢复",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    PmUnavailable(Pm),
    BinNotOnPath { pm: Pm, dir: PathBuf },
    InvalidVersion(String),
    NoOriginInstalled,
}

#[derive(Clone, Debug)]
pub enum TxOutcome {
    Committed { pm: Pm, version: Version },
    Rejected { reason: RejectReason },
    RolledBack { failed: TxStep, restored: Origin, detail: String },
    Degraded { failed: TxStep, reason: String, manual: Vec<String> },
}

#[derive(Clone, Debug)]
pub enum WebState {
    Stopped,
    Starting { port: u16 },
    Running { port: u16, pid: u32 },
    External { port: u16 },
    Failed { reason: String },
}

impl WebState {
    pub fn is_running(&self) -> bool {
        matches!(self, WebState::Running { .. } | WebState::External { .. })
    }
    pub fn port(&self) -> Option<u16> {
        match self {
            WebState::Starting { port }
            | WebState::Running { port, .. }
            | WebState::External { port } => Some(*port),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum NotesStatus {
    Loading,
    Ok,
    Missing,
    Failed,
}

#[derive(Clone, Debug)]
pub enum NotesError {
    Missing,
    Net(String),
}

#[derive(Clone, Debug)]
pub enum Job {
    Probe,
    FetchCatalog,
    FetchNotes { version: Version },
    Transact { origin: Origin, target: Target, port: u16 },
    StartWeb { port: u16 },
    StopWeb { pid: u32 },
    OpenUrl { url: String },
}

#[derive(Debug)]
pub enum UiMsg {
    Probed(PmEnv),
    Catalog(Catalog),
    Notes { version: Version, result: Result<String, NotesError> },
    Log(String),
    TxProgress(TxStep),
    TxDone(TxOutcome),
    WebState(WebState),
    WebExited { code: Option<i32> },
    Failed { context: &'static str, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pm_command_table_is_complete() {
        for pm in Pm::ALL {
            assert!(!pm.exe().is_empty(), "{pm:?} 缺少 exe");
            assert!(!pm.install_args("1.2.3").is_empty(), "{pm:?} 缺少安装命令");
            assert!(!pm.uninstall_args().is_empty(), "{pm:?} 缺少卸载命令");
        }
    }
}
