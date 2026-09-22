//! 纯数据叶子层。**不得 `use crate::` 任何其他模块** —— 这个性质让 txn.rs
//! 的单元测试只需 model.rs + 一个假 backend。
//!
//! 命令表（impl Pm）也放在这里：它是纯常量映射，无 I/O。若放到 pm.rs，
//! model.rs 就会为 Pm 类型反向依赖 pm.rs，破坏叶子性质。

// ⚠ 只有 `Catalog::tags` 用它，而那个字段只在测试构建里存在（理由见 `Catalog`）——
// 不加门的话 `cargo build` 会报 unused_imports（那是独立 lint）。
#[cfg(test)]
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

    /// FR-3 第 3 步：查询**全局包列表**的命令参数（`dsh` 不在 PATH 时的退化路径）。
    ///
    /// ⚠ 退出码**不能**当判据：`npm ls -g --depth=0` 在有 peer 依赖告警时会以非零码
    /// 退出，而 stdout 依然是完整列表。因此调用方（`pm::version_in_global_list`）
    /// 只看 stdout，不看退出码 —— 与 `probe_pm` / `version_from_shim` 的判据刻意相反，
    /// 那两处要的是"命令真的可用"，这里要的只是"列表打印出来了"。
    pub fn list_args(self) -> &'static [&'static str] {
        match self {
            Pm::Npm => &["ls", "-g", "--depth=0"],
            Pm::Pnpm => &["list", "-g", "--depth=0"],
            Pm::Bun => &["pm", "ls", "-g"],
            Pm::Yarn => &["global", "list"],
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
    /// dist-tags（`latest` / `next` / `alpha`）。
    ///
    /// ⚠ **只在测试构建里存在**，这不是笔误：生产代码**刻意不读**它 ——
    /// GC-14 的全部意义就是"绝不拿 registry 的 tag 当最新版本"
    /// （`AppState::newest_in_channel` 只从 `versions` 里取通道内最新）。
    /// 唯一消费者是 dsh.rs 的 GC-14 回归测试：它需要**真实的** tag 数据才能
    /// 证明"标签在场也不会被采用"。因此字段与它的解析都加了 `#[cfg(test)]` ——
    /// 去掉门就是死代码，而加回 `allow(dead_code)` 会把真实死代码一起盖住。
    #[cfg(test)]
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

/// 事务步骤。
///
/// ⚠ 这里【只有主流程】的四个步骤（Ruling 53）。早先还有 `Precheck` 与四个补偿
/// 步骤（`C1Probe` / `C2Restore` / `C2Cleanup` / `C3Confirm`），它们全仓**零构造点**：
/// - `Degraded.failed` / `RolledBack.failed` 在 UI 里渲染成「在「X」阶段失败」，
///   语义是**主流程**在哪一步失败（补偿失败的原因由 `reason` 承载）。
///   把补偿步骤塞进 `failed` 会显示成"降级：在「确认已恢复」阶段失败"，语义错位。
/// - 补偿进度**结构上无法上报**：`UiMsg::TxProgress` 只能由 `main.rs` 发送，
///   而 `main.rs` 看不到 `compensate` 内部。
/// 为骗过 lint 而补构造点是本末倒置，所以删除。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TxStep {
    S1Install,
    S2Verify,
    S3Uninstall,
    S4VerifyFinal,
}

impl TxStep {
    pub fn label(self) -> &'static str {
        match self {
            TxStep::S1Install => "安装新版本",
            TxStep::S2Verify => "验证新安装",
            TxStep::S3Uninstall => "卸载旧版本",
            TxStep::S4VerifyFinal => "最终验证",
        }
    }
}

/// 前置检查的拒绝原因。
///
/// ⚠ 没有 `NoOriginInstalled` 变体（Task 20 删除）：那一条从**未被构造过** ——
/// "未检测到已安装的 dsh"在 UI 层就被拦下了（`on_install_clicked` 拿不到
/// `owner`/`installed` 时直接写状态并返回，不派发 `Job::Transact`），
/// 事务引擎根本走不到那里。留一个永不构造的变体只会让匹配臂看起来像在兜底。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RejectReason {
    PmUnavailable(Pm),
    BinNotOnPath { pm: Pm, dir: PathBuf },
    /// NFR-6 的具名实现。`precheck` 里**有**构造点（只是运行期不可达）——
    /// 按已记录在案的裁决（Ruling 39）保留，与那批零构造点的变体不同。
    InvalidVersion(String),
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
    /// ⚠ `pid` 不是装饰：从 `spawn_web` 成功到就绪为止（最长 `wait_port_ready`
    /// 的 20 秒超时窗口）这是**唯一**持有子进程 pid 的地方。带上它，退出路径才能
    /// 在这段时间里把子进程收掉 —— 否则就是 FR-21 的静默孤儿（没有 job object
    /// 兜底，而 `running_port` 要就绪后才写，FR-31 同样恢复不了）。
    Starting { port: u16, pid: u32 },
    Running { port: u16, pid: u32 },
    External { port: u16 },
    /// 启动失败（端口被非 node 进程占用 / 启动超时 / spawn 失败）。
    ///
    /// ⚠ 曾是 `Failed { reason: String }`，那个字段**从未被读过**（Task 20 删除）：
    /// 三个构造点旁边都另发了一条更具体的 `UiMsg::Failed`，界面渲染的是那一条
    /// （状态栏 + 日志面板），这里的信息只是重复。`app.slint` 也没有承载它的属性。
    Failed,
}

impl WebState {
    pub fn is_running(&self) -> bool {
        matches!(self, WebState::Running { .. } | WebState::External { .. })
    }
    pub fn port(&self) -> Option<u16> {
        match self {
            WebState::Starting { port, .. }
            | WebState::Running { port, .. }
            | WebState::External { port } => Some(*port),
            _ => None,
        }
    }
}

// ⚠ `NotesStatus` 【刻意不在此处定义】。
// app.slint 的 `export enum NotesStatus` 会让 Slint 在 crate 根生成同名的
// Rust 枚举，那才是唯一使用者（`NotesState.status` 与 `set_notes_status`）。
// 若此处也定义一份，main.rs 的 `use model::*` 是【glob 导入】，会被 crate 根
// 的本地定义遮蔽 —— 结果是这份副本永远没有消费者，Task 20 删除
// `#![allow(dead_code)]` 后必然报 dead_code。
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
    /// ⚠ 载荷是 `port` + `own_pid`，**不是** pid。理由有两条：
    /// - GC-16：定位监听者要跑 `netstat.exe`、校验要跑 `tasklist.exe` —— 都属
    ///   "起子进程"，不得发生在 UI 线程。改成发端口后，定位与校验全在 worker 上。
    /// - NFR-7：`own_pid` 优先，否则现场定位 + `is_node` 守卫 —— 守卫因此紧挨着
    ///   `stop_by_pid`，任何未来的发送方都绕不过它。
    StopWeb { port: u16, own_pid: Option<u32> },
    OpenUrl { url: String },
}

/// I-4：把一队已入队的任务折叠成"只保留**最后一个** `FetchNotes`"。
///
/// ⚠ 为什么丢掉更早的请求是安全的：说明区只认**当前选中版本**的回复
/// （`drain` 的 `UiMsg::Notes` 臂按 `selected_version` 过滤），所以从用户改选的那一刻起，
/// 早先那些请求的回复就已经确定会被丢弃 —— 它们唯一的作用是让 worker 多阻塞几次
/// `fetch_notes`（每次最长 15 秒，NFR-3），并多烧几次 GitHub 的小时配额
/// （实测：一次 22 步版本遍历把本机配额打光）。
///
/// **保序保证**：结果是原队列的**子序列** —— 除被取代的 `FetchNotes` 外**一个任务都不丢**，
/// 且相对顺序与入队顺序完全一致（存活的最后一个说明请求也留在它原本的位置上）。
/// 因此本函数是纯函数，可直接单测（`coalesce_notes_*`）。
pub fn coalesce_notes(jobs: Vec<Job>) -> Vec<Job> {
    let last = jobs.iter().rposition(|j| matches!(j, Job::FetchNotes { .. }));
    jobs.into_iter()
        .enumerate()
        .filter(|(i, j)| !matches!(j, Job::FetchNotes { .. }) || Some(*i) == last)
        .map(|(_, j)| j)
        .collect()
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
    /// ⚠ 必须带 `pid`：worker 是串行 FIFO，但旧实例的退出通知可能**迟到**到新实例
    /// 已经开始之后。没有身份就无法区分"当前实例退出了"与"上一个实例的迟到消息"，
    /// 后者会把新实例的 `web_pid` 清掉 —— 退出路径随即拿不到 pid（孤儿），
    /// 或者更糟：拿着被系统复用的旧 pid 去 taskkill。
    WebExited { pid: u32, code: Option<i32> },
    /// 系统主题变了。⚠ 只带"现在是不是暗色"，不带模式 ——
    /// 模式归 Rust 的 AppState 管，与系统态在这里是正交的两件事。
    SystemThemeChanged(bool),
    Failed { context: &'static str, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FR-14 / FR-2 / GC-7 的**精确**断言。
    ///
    /// ⚠ 必须断言确切的参数序列，**不能只断言"非空"**。只查非空的话，
    /// 把安装子命令误写成 `uninstall`、或写错 `-g` 的位置，测试仍会通过 ——
    /// 而那恰恰是这张表唯一可能出错的方式。FR-14 的全部价值就在这些确切
    /// 字符串上，断言必须钉到同一粒度。
    #[test]
    fn pm_command_table_is_exact() {
        let cases: [(Pm, &str, Vec<&str>, Vec<&str>, Vec<&str>, Vec<&str>); 4] = [
            (
                Pm::Npm,
                "npm.cmd",
                vec!["prefix", "-g"],
                vec!["ls", "-g", "--depth=0"],
                vec!["install", "-g", "@deepseek-ai/dsh@1.2.3"],
                vec!["uninstall", "-g", "@deepseek-ai/dsh"],
            ),
            (
                Pm::Pnpm,
                "pnpm.cmd",
                vec!["bin", "-g"],
                vec!["list", "-g", "--depth=0"],
                vec!["add", "-g", "@deepseek-ai/dsh@1.2.3"],
                vec!["remove", "-g", "@deepseek-ai/dsh"],
            ),
            (
                Pm::Bun,
                "bun.exe",
                vec!["pm", "bin", "-g"],
                vec!["pm", "ls", "-g"],
                vec!["add", "-g", "@deepseek-ai/dsh@1.2.3"],
                vec!["remove", "-g", "@deepseek-ai/dsh"],
            ),
            (
                Pm::Yarn,
                "yarn.cmd",
                vec!["global", "bin"],
                vec!["global", "list"],
                vec!["global", "add", "@deepseek-ai/dsh@1.2.3"],
                vec!["global", "remove", "@deepseek-ai/dsh"],
            ),
        ];

        assert_eq!(
            cases.len(),
            Pm::ALL.len(),
            "每个 Pm 变体都必须有断言用例（新增变体时同步更新）"
        );

        for (pm, exe, bin_dir, list, install, uninstall) in cases {
            assert_eq!(pm.exe(), exe, "{pm:?} 的 exe 名不对（GC-7）");

            assert_eq!(
                pm.bin_dir_args().to_vec(),
                bin_dir,
                "{pm:?} 的全局 bin 目录参数不对（FR-2）"
            );

            assert_eq!(
                pm.list_args().to_vec(),
                list,
                "{pm:?} 的全局包列表参数不对（FR-3 第 3 步）"
            );

            let got: Vec<String> = pm.install_args("1.2.3");
            let want: Vec<String> = install.iter().map(|s| s.to_string()).collect();
            assert_eq!(got, want, "{pm:?} 的安装命令不对（FR-14）");

            let got = pm.uninstall_args();
            let want: Vec<String> = uninstall.iter().map(|s| s.to_string()).collect();
            assert_eq!(got, want, "{pm:?} 的卸载命令不对（FR-14）");
        }

        // label() 被 UI 与日志使用，一并钉住
        assert_eq!(Pm::Npm.label(), "npm");
        assert_eq!(Pm::Pnpm.label(), "pnpm");
        assert_eq!(Pm::Bun.label(), "bun");
        assert_eq!(Pm::Yarn.label(), "yarn");
    }

    fn notes(v: &str) -> Job {
        Job::FetchNotes { version: v.parse().unwrap() }
    }

    /// ★ I-4 的核心：连续切换版本时，队列里只该留下**最后一个**说明请求。
    ///
    /// 判别性：把 `coalesce_notes` 换成 `jobs`（即不合并），本测试立刻失败。
    #[test]
    fn coalesce_notes_keeps_only_the_last_fetch() {
        let jobs = vec![notes("0.1.1"), notes("0.1.2"), notes("0.1.3")];
        let merged = coalesce_notes(jobs);
        assert_eq!(merged.len(), 1, "只该剩最后一个请求，实际 {merged:?}");
        assert!(
            matches!(&merged[0], Job::FetchNotes { version } if version.to_string() == "0.1.3"),
            "剩下的必须是【最后】那个选择（最新选择赢），实际 {merged:?}"
        );
    }

    /// ★ 保序：非 `FetchNotes` 的任务一个都不能丢，相对顺序也不能变。
    ///
    /// 这条断言是"合并不会打乱安装/启动/停止的执行次序"的唯一保证 ——
    /// 一个"顺手把所有任务都去重"的实现会在第 2 条断言上失败。
    #[test]
    fn coalesce_notes_preserves_every_other_job_in_order() {
        let jobs = vec![
            notes("0.1.1"),
            Job::StopWeb { port: 3099, own_pid: None },
            notes("0.1.2"),
            Job::StartWeb { port: 3099 },
            notes("0.1.3"),
            Job::OpenUrl { url: "http://127.0.0.1:3099".into() },
        ];
        let merged = coalesce_notes(jobs);
        assert_eq!(merged.len(), 4, "6 个任务里只该丢掉 2 个过期的说明请求，实际 {merged:?}");
        // ★ 合并结果是原队列的**子序列**：非 notes 任务一个不丢、顺序不变，
        // 唯一存活的 notes 请求也留在**它原本的位置**上（不是被挪到队尾）。
        assert!(matches!(merged[0], Job::StopWeb { .. }), "实际 {merged:?}");
        assert!(matches!(merged[1], Job::StartWeb { .. }), "实际 {merged:?}");
        assert!(
            matches!(&merged[2], Job::FetchNotes { version } if version.to_string() == "0.1.3"),
            "存活的说明请求必须留在原位（在 OpenUrl 之前），实际 {merged:?}"
        );
        assert!(matches!(merged[3], Job::OpenUrl { .. }), "实际 {merged:?}");
    }

    /// 队列里本来就没有说明请求时，合并必须是恒等变换（含空队列）。
    #[test]
    fn coalesce_notes_is_identity_without_fetch_notes() {
        let jobs = vec![Job::Probe, Job::FetchCatalog, Job::OpenUrl { url: "x".into() }];
        assert_eq!(coalesce_notes(jobs.clone()).len(), 3);
        assert!(matches!(coalesce_notes(jobs)[0], Job::Probe));
        assert!(coalesce_notes(vec![]).is_empty());
    }
}
