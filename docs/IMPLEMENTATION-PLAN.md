# DSH Manager 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 构建一个 Windows 桌面 GUI 程序，用于查看 `dsh` 版本、安装/更新/迁移包管理器、并以后台无窗口方式托管 `dsh web`。

**Architecture:** 单进程双实例 Slint 应用（主窗口 + 托盘，二者不共享 global）。Rust 侧 UI 线程独占 `AppState` 作为唯一状态源，无锁；一个无状态 worker 线程串行执行 `Job`；UI 用 80ms `Timer` 排空 `UiMsg` 并推送到两个组件实例。所有变更操作走统一的 saga 事务模型（先装后卸 + 探测式补偿）。

**Tech Stack:** Rust 2024 edition / Slint 1.18（默认 feature 集）/ ureq 3.4 / serde_json 1 / semver 1

**Spec:**
- 需求：[docs/SRS.md](SRS.md) v1.2
- 设计：[docs/ARCHITECTURE.md](ARCHITECTURE.md) v1.2

> 执行者**必须同时阅读这两份文档**。本计划的任务从它们推导而来；当计划与 spec 冲突时以 spec 为准并上报。

## Global Constraints

以下约束适用于**每一个任务**，不再逐任务重复。数值均逐字取自 SRS。

| GC | 对应 SRS | 约束 |
|---|---|---|
| **GC-1** | CON-1 | 目标平台 **仅 Windows**。可保持可移植性，但只在 Windows 上验证。 |
| **GC-2** | ARCH §7.1/§7.2 | 依赖**只允许 5 个**：`slint`、`slint-build`、`ureq`、`serde_json`、`semver`。**不得新增任何其他 crate** —— 包括 `tokio`、`reqwest`、`dirs`、`anyhow`、`thiserror`、`open`、`tray-icon`、`serde`(derive)、`windows`。 |
| **GC-3** | CON-2 | Slint 使用**默认 feature 集，不裁剪**。 |
| **GC-4** | CON-3 | 后台并发只用 `std::thread` + `std::sync::mpsc`。**禁止 async 运行时**。 |
| **GC-5** | CON-4 | 网络访问**只允许** `registry.npmjs.org` 与 `api.github.com`。**禁止** `github.com` 网页、`git ls-remote`、git 协议。 |
| **GC-6** | CON-5 | **不得**以任何方式干预 `dsh` profile 的插件管理。 |
| **GC-7** | CON-6 | 所有外部命令必须用 Windows shim **全名**。`Command::new("dsh")` 在 Windows 上**无法解析**。 |
| **GC-8** | CON-7 | 子进程必须携带 `CREATE_NO_WINDOW` = `0x08000000`；二进制必须声明 windows 子系统。 |
| **GC-9** | NFR-5, NFR-6 | 外部命令一律用**参数数组**，禁止拼接 shell 字符串；版本号使用前校验字符集 `[0-9A-Za-z.\-+]`。 |
| **GC-10** | CON-8, NFR-9, NFR-10, FR-29 | "关于"必须含 **`AboutSlint`** 组件（免版税许可的强制署名义务）；不得移除 Slint 源码中的许可声明。 |
| **GC-11** | CON-9 | **不嵌入任何字体文件**。Slint 自动从系统获取字体。 |
| **GC-12** | NFR-11 | 日志缓冲**上限 2000 行**，超出丢弃最旧行。 |
| **GC-13** | 实测（§2.4.3） | std-widgets 必须显式 import，它们不是语言内建元素。 |
| **GC-14** | FR-8, §2.2.5 | **绝对禁止**把"最新版本"等同于 npm 的 `latest` tag。 |
| **GC-15** | 实测（§2.4.1） | `slint::Timer` 及其回调捕获的所有 `Rc` **永不离开 UI 线程**。 |
| **GC-16** | NFR-1, NFR-2, NFR-4 | 网络访问、PM 命令、子进程管理**必须在后台线程**执行；UI 线程不得出现可感知阻塞；启动后 1 秒内可交互；托盘常驻期间空闲 CPU 接近零。 |
| **GC-17** | NFR-3 | 网络请求超时上限 15 秒，超时后进入失败路径而非无限等待。 |
| **GC-18** | NFR-8 | 程序不读取、不存储、不传输任何凭据。 |

### 未派生任务的需求（说明其去向）

以下 SRS 需求**刻意没有对应任务**，原因如下：

| SRS 需求 | 去向 |
|---|---|
| **AS-1 ~ AS-4**（假设与依赖） | 属前提假设而非可交付需求。AS-1 / AS-2 的失败路径由 Task 7（PM 探测返回 `None`）与 Task 11/12（网络错误分支）覆盖；AS-3 由 Task 14 的"用 TCP 探测而非解析 stdout"规避；AS-4 由 Task 7 的 `read_dsh_version_at` 直接满足。 |
| **TR-8**（不做崩溃恢复日志） | 明确的**不做**事项。其后果由 Task 18 的孤儿恢复（FR-31）缓解。 |
| **TR-9**（不做无限重试） | 不做。Task 10 的补偿只尝试一次。 |
| **TR-10**（不做并行安装/断点续传） | 不做。Task 18 的 worker 单线程串行天然满足。 |

**这是完整的覆盖账目**：68 条 SRS 需求中，**42 条有直接任务**（FR 全 33 条 + TR-1~7/TR-11 共 8 条 + NFR-7），**7 条有明确去向**（AS-1~4 + TR-8~10），其余 **19 条为表格型约束**（CON 全 9 条 + NFR 10 条）已折入 GC-1 ~ GC-18。42 + 7 + 19 = 68 ✓

## 已实测确认的平台事实

执行时**不要重新试探**这些结论，它们已在目标机器上验证过（ARCHITECTURE §2.4）：

| 事实 | 结论 |
|---|---|
| `Timer::start` 回调的 `Send` 约束 | **无**。可捕获 `Rc<RefCell<..>>` / `Rc<VecModel<..>>` / `mpsc::Receiver<..>` |
| Timer 精度 | 平均 50.5ms，抖动 ±1.2ms。**未被节流** |
| 启动延迟 | 窗口 12.5ms 就绪；事件循环启动延迟约 278ms（数据到达前的空窗期必须显示加载态） |
| `close-requested` | **在 Slint 语言中不存在**（slint#6401 仍 open）。Rust API 是 `slint::Window::on_close_requested(..) -> CloseRequestResponse` |
| 关闭语义 | `HideWindow` = 接受关闭并隐藏；`KeepWindowShown` = **取消**关闭（用错会导致点 X 毫无反应） |
| 冷编译 | 约 2 分钟；增量（仅改 `.slint`）4.3–8.2 秒 |

## File Structure

| 文件 | 职责 | 阶段 |
|---|---|---|
| `Cargo.toml` | 5 个依赖 + release profile | 1 |
| `build.rs` | Slint 编译期集成 | 1 |
| `ui/app.slint` | `MainWindow` + `AppTray` 两个顶层组件、`NotesStatus` 枚举 | 1, 4 |
| `src/model.rs` | **纯数据叶子层**：`Pm` 枚举 + 命令表、`PmEnv`、`Catalog`、`Origin`/`Target`、`TxStep`/`RejectReason`/`TxOutcome`、`WebState`、`NotesStatus`/`NotesError`、`Job`、`UiMsg`。**不依赖任何模块** | 2 |
| `src/config.rs` | `state.json` 读写：路径解析、读取与缺省回退、原子写入。**纯 I/O，无 UI 逻辑** | 2 |
| `src/pm.rs` | PM 探测（扫描 / bin 目录 / owner 判定）+ 通道判定 + 命令执行helper | 2 |
| `src/txn.rs` | 事务引擎 + `Backend` trait + `SystemBackend` 实现 | 2 |
| `src/dsh.rs` | 版本读取、registry/GitHub 拉取、说明预处理、`dsh web` 进程监督 | 3 |
| `src/main.rs` | 入口、`AppState`、Timer 排空、`project()`、worker 循环、回调接线 | 4 |

**依赖方向（唯一硬约束）**：`model.rs` 是叶子，不依赖任何模块。`config.rs` 只依赖 `model.rs`。`pm.rs` / `dsh.rs` 依赖 `model.rs`。`txn.rs` 依赖 `model.rs` + `pm.rs` + `dsh.rs`。`main.rs` 依赖全部。

> **本计划对 SRS §9.1 的一处细化**（不改变文件清单，只明确归属）：`Pm` 枚举与其命令表放在 `model.rs`（纯数据，无 I/O），`pm.rs` 只放探测逻辑。理由：若 `Pm` 放 `pm.rs`、`Origin`/`Target` 放 `txn.rs`，`model.rs` 就会反向依赖二者，破坏其叶子模块性质 —— 而这个性质正是让事务引擎可被单元测试的前提。

---

# 阶段 1：可运行的骨架

**阶段目标**：证明工具链、依赖与 Slint 集成全部可用，得到一个能打开空窗口且**没有黑窗口**的最小程序。

---

### Task 1: 工具链骨架与空窗口

**Files:**
- Create: `Cargo.toml`
- Create: `build.rs`
- Create: `ui/app.slint`
- Modify: `src/main.rs`（当前是 cargo init 的 hello world）

**Interfaces:**
- Consumes: 无
- Produces: 可编译的 crate；`ui/app.slint` 中的 `MainWindow` 组件；`slint::include_modules!()` 可用

- [ ] **Step 1: 写 `Cargo.toml`**

```toml
[package]
name = "dsh-manager"
version = "0.1.0"
edition = "2024"

[dependencies]
slint = "1.18"
ureq = "3.4"
serde_json = "1"
semver = "1"

[build-dependencies]
slint-build = "1.18"

[profile.release]
opt-level = "s"
lto = true
strip = true
```

（GC-2：这 5 个是**全部**依赖，后续任务不得新增。）

- [ ] **Step 2: 写 `build.rs`**

```rust
fn main() {
    slint_build::compile("ui/app.slint").expect("Slint 编译失败");
}
```

- [ ] **Step 3: 写最小 `ui/app.slint`**

```slint
export component MainWindow inherits Window {
    title: "DSH Manager";
    width: 520px;
    height: 640px;

    Text {
        text: "DSH Manager";
        horizontal-alignment: center;
        vertical-alignment: center;
    }
}
```

- [ ] **Step 4: 写 `src/main.rs`**

```rust
// GC-8：不加这行，GUI 背后会挂一个黑窗口 —— 正是本项目要消灭的东西。
// 用 cfg_attr(not(test)) 而非裸属性：避免测试构建也被标为 GUI 子系统而吞掉
// 测试输出。Task 2 的 Step 2 会验证这个写法确实让 `cargo test` 有输出。
#![cfg_attr(not(test), windows_subsystem = "windows")]

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    ui.run()
}
```

- [ ] **Step 5: 编译**

Run: `cargo build`
Expected: 成功。**首次冷编译约 2 分钟**（依赖树含 winit / femtovg / accesskit / swash / resvg 等），属正常。

- [ ] **Step 6: 运行并确认无黑窗口**

Run: `cargo run`
Expected:
1. 出现一个标题为 "DSH Manager" 的窗口，居中显示 "DSH Manager"
2. **任务栏中不出现额外的控制台窗口**（GC-8 的验证）
3. 关闭窗口后进程退出

- [ ] **Step 7: 提交**

```bash
git add Cargo.toml build.rs ui/app.slint src/main.rs Cargo.lock
git commit -m "feat: 工具链骨架与空窗口

- Cargo.toml：5 个依赖（slint / ureq / serde_json / semver + slint-build）
- build.rs：Slint 编译期集成
- ui/app.slint：MainWindow 最小组件
- src/main.rs：windows_subsystem 属性 + 空窗口

验证：cargo run 弹出窗口且任务栏无控制台窗口"
```

> **注意提交 `Cargo.lock`**：本项目是二进制程序，锁文件应当入库。首次提交时 `Cargo.lock` 会被生成。

---

# 阶段 2：纯逻辑层（单元测试驱动）

**阶段目标**：把全部**高风险逻辑**实现并用单元测试穷尽，**完全不涉及 GUI、网络与真实进程**。这是 SRS §8.4 要求的可测逻辑所在，也是全项目错误代价最高的部分（SRS §4）。

每个任务都以 `cargo test` 全绿为完成标志。

---

### Task 2: `src/model.rs` — 纯数据叶子层

**Files:**
- Create: `src/model.rs`
- Modify: `src/main.rs`（加 `mod model;`）

**Interfaces:**
- Consumes: 无（叶子模块）
- Produces:
  - `Pm { Npm, Pnpm, Bun, Yarn }` + 方法 `exe() -> &'static str`、`label() -> &'static str`、`bin_dir_args() -> &'static [&'static str]`、`install_args(&str) -> Vec<String>`、`uninstall_args() -> Vec<String>`、`ALL: [Pm; 4]`
  - `Channel { Stable, Rc, Alpha, Other }`
  - `PmInfo { kind: Pm, version: String, bin_dir: PathBuf }`
  - `PmEnv { available: Vec<PmInfo>, owner: Option<Pm>, installed: Option<Version>, dsh_path: Option<PathBuf> }`
  - `Catalog { versions: Vec<Version>, tags: BTreeMap<String, Version> }`
  - `Origin { pm: Pm, version: Version }` / `Target { pm: Pm, version: Version }`
  - `TxStep`、`RejectReason`、`TxOutcome`
  - `WebState`、`NotesStatus`、`NotesError`
  - `Job`、`UiMsg`
  - `Version`（`semver::Version` 的别名）
  - `SHIM_NAMES: [&str; 2]`

- [ ] **Step 1: 写 `src/model.rs`**

```rust
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
```

- [ ] **Step 2: 在 `src/main.rs` 中加 crate 属性与模块声明**

在 `src/main.rs` 的 `#![cfg_attr(...)]` 行**之后**、`slint::include_modules!();` **之前**插入：

```rust
// ⚠ 临时（Task 2 ~ Task 16 期间存在）
//
// 各模块按依赖顺序逐步落地，先定义的类型/函数要到很晚才被消费
// （model.rs 的类型直到 Task 17 才接上 UI），在【二进制 crate】中
// 未使用的 pub 项会触发 dead_code 警告。实测确认：bin crate 不会
// 因为是 pub 就豁免这个 lint。
//
// 若不抑制，Task 2~16 的构建输出会持续带着十几个无关警告 ——
// 既让"输出必须干净"的检查失效，也会掩盖真实警告。
//
// 【Task 20 必须删除本行，并确认 cargo build 零警告】
// 本行会掩盖真实死代码，只应短期存在。
#![allow(dead_code)]

mod model;
```

> **`mod model;` 的插入位置**：必须在 `slint::include_modules!()` **之前**。`include_modules!` 展开为一个模块，放它会打乱顶层声明顺序（虽不报错但不清晰）。

然后加一个**临时**冒烟测试，确认 `cfg_attr(not(test), windows_subsystem)` 没有吞掉测试输出。把下面这段**追加到 `src/model.rs` 末尾**：

```rust
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
```

Run: `cargo test pm_command_table_is_complete`
Expected: 看到 `test result: ok. 1 passed`。

> **若看不到任何输出**：说明 `cfg_attr(not(test), ..)` 未生效，需把 `src/main.rs` 的模块与逻辑迁到 `src/lib.rs`，把测试目标与 GUI 目标分开。**此时停下来上报**，不要继续后续任务。

- [ ] **Step 3: 提交**

```bash
git add src/model.rs src/main.rs
git commit -m "feat(model): 纯数据叶子层

集中定义全部共享类型：Pm 枚举与命令表、PmEnv、Catalog、
Origin/Target、TxStep/RejectReason/TxOutcome、WebState、
Job/UiMsg。

Pm 的命令表放在此处而非 pm.rs：它是纯常量映射，若放 pm.rs
会让 model.rs 为 Pm 类型反向依赖，破坏叶子性质 —— 而该性质
是 txn.rs 可单元测试的前提。"
```

---

### Task 3: `src/config.rs` — 路径解析与读取

**Files:**
- Create: `src/config.rs`
- Modify: `src/main.rs`（加 `mod config;`）

**Interfaces:**
- Consumes: `model::*`
- Produces:
  - `StateFile { preferred_port: Option<u16>, running_port: Option<u16> }`（`Default` + `Clone` + `Debug` + `PartialEq`）
  - `Loaded { Ok(StateFile), Missing, Corrupt(String), NoLocation }`
  - `state_path() -> Option<PathBuf>`
  - `load() -> Loaded`
  - `save(&StateFile) -> Result<(), String>`
  - `update(impl FnOnce(&mut StateFile)) -> Result<(), String>`
  - **测试缝**：`load_from(Option<&Path>) -> Loaded` 与 `save_to(&Path, &StateFile) -> Result<(), String>`（供单元测试注入临时路径，避免污染真实 `%APPDATA%`）

- [ ] **Step 1: 写失败测试**

创建 `src/config.rs`，**只写测试与类型骨架**：

```rust
//! state.json 的 I/O 层。覆盖 SRS FR-30 / FR-31 / FR-32。
//! 只做 I/O，**不含"何时该写"的决策** —— 写入时机由调用方决定。

use std::path::{Path, PathBuf};

#[derive(Default, Clone, Debug, PartialEq)]
pub struct StateFile {
    pub preferred_port: Option<u16>,
    pub running_port: Option<u16>,
}

#[derive(Debug)]
pub enum Loaded {
    Ok(StateFile),
    Missing,
    Corrupt(String),
    NoLocation,
}

pub fn state_path() -> Option<PathBuf> {
    todo!()
}

pub fn load() -> Loaded {
    todo!()
}

pub fn save(_s: &StateFile) -> Result<(), String> {
    todo!()
}

pub fn update(_f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dsh-mgr-test-{name}-{}", std::process::id()))
    }

    #[test]
    fn missing_file_yields_missing_not_error() {
        let p = tmp("missing.json");
        let _ = std::fs::remove_file(&p);
        assert!(matches!(load_from(Some(&p)), Loaded::Missing));
    }

    #[test]
    fn corrupt_file_yields_corrupt_with_reason() {
        let p = tmp("corrupt.json");
        std::fs::write(&p, "{ this is not json").unwrap();
        match load_from(Some(&p)) {
            Loaded::Corrupt(msg) => assert!(!msg.is_empty(), "应带上原因"),
            other => panic!("期望 Corrupt，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn parses_both_fields() {
        let p = tmp("both.json");
        std::fs::write(&p, r#"{"preferred_port":8080,"running_port":9090}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.preferred_port, Some(8080));
                assert_eq!(s.running_port, Some(9090));
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn absent_fields_are_none_not_default() {
        // FR-31：running_port 为 None 本身就是有意义的信息（没有本程序启动的实例）
        let p = tmp("empty-obj.json");
        std::fs::write(&p, "{}").unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.preferred_port, None);
                assert_eq!(s.running_port, None);
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn no_location_when_path_unavailable() {
        assert!(matches!(load_from(None), Loaded::NoLocation));
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test --lib` 或 `cargo test config`
Expected: 编译失败 —— `load_from` 未定义。

- [ ] **Step 3: 实现读取**

在 `src/config.rs` 中，把 `load()` 的 `todo!()` 替换为：

```rust
/// 读取的测试缝：`None` 表示路径不可用。
pub fn load_from(path: Option<&Path>) -> Loaded {
    let Some(p) = path else {
        return Loaded::NoLocation;
    };
    let text = match std::fs::read_to_string(p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(e) => return Loaded::Corrupt(format!("读取失败: {e}")),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Loaded::Corrupt(format!("JSON 解析失败: {e}")),
    };
    let read_port = |key: &str| -> Option<u16> {
        v.get(key)?.as_u64().and_then(|n| u16::try_from(n).ok())
    };
    Loaded::Ok(StateFile {
        preferred_port: read_port("preferred_port"),
        running_port: read_port("running_port"),
    })
}

pub fn load() -> Loaded {
    load_from(state_path().as_deref())
}

pub fn state_path() -> Option<PathBuf> {
    // GC-2：不引入 dirs crate，一个 APPDATA 路径不值一个依赖
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("dsh-manager").join("state.json"))
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test config`
Expected: 5 passed（`save`/`update` 尚未被测试触及，`todo!()` 不会被调用）

- [ ] **Step 5: 提交**

```bash
git add src/config.rs src/main.rs
git commit -m "feat(config): state.json 路径解析与读取

用 Loaded 枚举而非 Result：'文件不存在'（首次运行的正常状态，
必须静默）与'文件损坏'（异常，须记日志）对用户意义完全不同，
压成同一个 Err 会让调用方漏掉静默分支。

load_from 作为测试缝，让测试注入临时路径而不污染真实 %APPDATA%。"
```

---

### Task 4: `src/config.rs` — 原子写入

**Files:**
- Modify: `src/config.rs`

**Interfaces:**
- Consumes: Task 3 的 `StateFile` / `Loaded` / `load_from` / `state_path`
- Produces: `save_to(&Path, &StateFile) -> Result<(), String>`、`save()`、`update()`

- [ ] **Step 1: 写失败测试**

在 `src/config.rs` 的 `mod tests` 内追加：

```rust
    #[test]
    fn save_then_load_roundtrip() {
        let p = tmp("roundtrip.json");
        let s = StateFile { preferred_port: Some(8080), running_port: Some(9090) };
        save_to(&p, &s).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(got) => assert_eq!(got, s),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = tmp("mkdir");
        let p = dir.join("nested").join("state.json");
        let _ = std::fs::remove_dir_all(&dir);
        save_to(&p, &StateFile::default()).unwrap();
        assert!(p.is_file(), "应自动创建父目录");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_leaves_no_tmp_file_behind() {
        let p = tmp("notmp.json");
        save_to(&p, &StateFile { preferred_port: Some(1), running_port: None }).unwrap();
        let leftover = p.with_extension("json.tmp");
        assert!(!leftover.exists(), "原子写入不得残留 .tmp 文件");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_preserves_other_field() {
        let p = tmp("update.json");
        save_to(&p, &StateFile { preferred_port: Some(3080), running_port: None }).unwrap();
        update_at(&p, |s| s.running_port = Some(8080)).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.preferred_port, Some(3080), "改 running 不得丢失 preferred");
                assert_eq!(s.running_port, Some(8080));
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_on_corrupt_file_starts_from_default() {
        // FR-32：损坏文件不应让写入路径也失败
        let p = tmp("update-corrupt.json");
        std::fs::write(&p, "garbage").unwrap();
        update_at(&p, |s| s.preferred_port = Some(3080)).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.preferred_port, Some(3080)),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test config`
Expected: 编译失败 —— `save_to` / `update_at` 未定义。

- [ ] **Step 3: 实现原子写入**

把 `save` 与 `update` 的 `todo!()` 替换为：

```rust
pub fn save(s: &StateFile) -> Result<(), String> {
    match state_path() {
        Some(p) => save_to(&p, s),
        None => Err("APPDATA 不可用".into()),
    }
}

/// 原子写入的测试缝。
///
/// **必须**走"写 .tmp → rename"，不得直接截断目标文件。
/// 依据：FR-31 要应对的核心场景是管理器被强杀，而直接截断写入时进程若在
/// 写入中途终止，state.json 会变成半截 JSON。也就是说 —— 最需要持久化生效
/// 的场景，恰恰是朴素写入最容易毁掉数据的场景。文件一坏，下次启动回落缺省
/// 端口，孤儿就失联了。
///
/// fs::rename 的覆盖语义已核实：Rust 文档明确 "replacing the original file
/// if `to` already exists"，Windows 上通过 MoveFileExW 实现。
pub fn save_to(path: &Path, s: &StateFile) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let json = serde_json::json!({
        "preferred_port": s.preferred_port,
        "running_port": s.running_port,
    });
    let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;

    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("写临时文件失败: {e}")
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("替换失败: {e}")
    })
}

pub fn update(f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
    let mut s = match load() {
        Loaded::Ok(s) => s,
        // FR-32：损坏或缺失都不应让写入路径失败，从缺省值开始
        _ => StateFile::default(),
    };
    f(&mut s);
    save(&s)
}

/// 读—改—写的测试缝。
pub fn update_at(path: &Path, f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
    let mut s = match load_from(Some(path)) {
        Loaded::Ok(s) => s,
        _ => StateFile::default(),
    };
    f(&mut s);
    save_to(path, &s)
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test config`
Expected: 10 passed

- [ ] **Step 5: 提交**

```bash
git add src/config.rs
git commit -m "feat(config): 原子写入 state.json

必须走'写 .tmp → rename'而非直接截断：FR-31 应对的核心场景是
管理器被强杀，而截断写入时进程若中途终止，state.json 会变成
半截 JSON —— 最需要持久化生效的场景恰是朴素写入最易毁数据的
场景。文件一坏，下次启动回落缺省端口，孤儿即失联。

fs::rename 覆盖语义已核实（Windows 走 MoveFileExW）。"
```

---

### Task 5: `src/pm.rs` — 通道判定（FR-7 / FR-8）

> 从这条最容易出错的需求开始：**防误降级**（GC-14）。

**Files:**
- Create: `src/pm.rs`
- Modify: `src/main.rs`（加 `mod pm;`）

**Interfaces:**
- Consumes: `model::{Version, Channel}`
- Produces:
  - `channel_of(&Version) -> Channel`
  - `latest_in(&[Version], Channel) -> Option<&Version>`
  - `sorted_desc(Vec<Version>) -> Vec<Version>`

- [ ] **Step 1: 写失败测试**

创建 `src/pm.rs`：

```rust
//! 包管理器探测（FR-1 ~ FR-5）、通道判定（FR-7 / FR-8）与命令执行 helper。

use crate::model::*;

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn channel_of_classifies_correctly() {
        assert_eq!(channel_of(&v("0.1.6-alpha.2")), Channel::Alpha);
        assert_eq!(channel_of(&v("0.1.5-rc.2")), Channel::Rc);
        assert_eq!(channel_of(&v("0.1.0")), Channel::Stable);
        assert_eq!(channel_of(&v("0.1.0-beta.1")), Channel::Other);
    }

    /// ★ GC-14 的核心回归测试。
    /// npm 的 latest tag 是 0.1.5-rc.2，比已装的 0.1.6-alpha.2 更旧。
    /// 在 alpha 通道内求"最新"必须得到 0.1.6-alpha.2，绝不能是 0.1.5-rc.2。
    #[test]
    fn latest_in_channel_never_downgrades() {
        let all = vec![v("0.1.5-rc.2"), v("0.1.6-alpha.1"), v("0.1.6-alpha.2")];
        assert_eq!(latest_in(&all, Channel::Alpha), Some(&v("0.1.6-alpha.2")));
        assert_eq!(latest_in(&all, Channel::Rc), Some(&v("0.1.5-rc.2")));
        assert_eq!(latest_in(&all, Channel::Stable), None);
    }

    #[test]
    fn latest_in_rc_ordering_is_semver_not_string() {
        // 字符串序会把 "0.1.10-rc.1" 排在 "0.1.5-rc.2" 之前；semver 不会
        let all = vec![v("0.1.5-rc.2"), v("0.1.10-rc.1")];
        assert_eq!(latest_in(&all, Channel::Rc), Some(&v("0.1.10-rc.1")));
    }

    #[test]
    fn prerelease_sorts_before_release_of_same_version() {
        // 0.1.6-alpha.2 < 0.1.6 —— 语义正确性
        let all = vec![v("0.1.6-alpha.2"), v("0.1.6")];
        assert_eq!(latest_in(&all, Channel::Stable), Some(&v("0.1.6")));
        assert_eq!(latest_in(&all, Channel::Alpha), Some(&v("0.1.6-alpha.2")));
    }

    #[test]
    fn sorted_desc_puts_newest_first() {
        let s = sorted_desc(vec![v("0.1.5-rc.2"), v("0.1.6-alpha.2"), v("0.1.0")]);
        assert_eq!(s[0], v("0.1.6-alpha.2"));
        assert_eq!(s[2], v("0.1.0"));
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test pm`
Expected: 编译失败 —— `channel_of` / `latest_in` / `sorted_desc` 未定义。

- [ ] **Step 3: 实现**

在 `src/pm.rs` 的 `mod tests` **之前**插入：

```rust
/// FR-7：由 semver 的 pre 字段推导发布通道。
pub fn channel_of(v: &Version) -> Channel {
    if v.pre.is_empty() {
        return Channel::Stable;
    }
    let s = v.pre.as_str();
    if s.starts_with("alpha") {
        Channel::Alpha
    } else if s.starts_with("rc") {
        Channel::Rc
    } else {
        Channel::Other
    }
}

/// FR-8 / GC-14：**只在指定通道内**求最新。
/// 绝不使用 npm 的 `latest` tag —— 它可能比已安装版本更旧。
pub fn latest_in<'a>(versions: &'a [Version], ch: Channel) -> Option<&'a Version> {
    versions.iter().filter(|v| channel_of(v) == ch).max()
}

/// 版本列表降序（最新在前）。FR-10 要求。
pub fn sorted_desc(mut versions: Vec<Version>) -> Vec<Version> {
    versions.sort_by(|a, b| b.cmp(a));
    versions
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test pm`
Expected: 5 passed

- [ ] **Step 5: 提交**

```bash
git add src/pm.rs src/main.rs
git commit -m "feat(pm): 通道判定与通道内最新版（FR-7/FR-8）

含 GC-14 的核心回归测试：npm 的 latest tag 是 0.1.5-rc.2，
比已安装的 0.1.6-alpha.2 更旧。在 alpha 通道内求最新必须得到
0.1.6-alpha.2，否则会导致误降级。"
```

---

### Task 6: `src/pm.rs` — owner 判定（FR-3）

**Files:**
- Modify: `src/pm.rs`

**Interfaces:**
- Consumes: `model::{Pm, PmInfo, SHIM_NAMES}`
- Produces:
  - `shim_in(dir: &Path) -> Option<PathBuf>`
  - `find_dsh_on_path(path_dirs: &[PathBuf], exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf>`
  - `same_dir(a: &Path, b: &Path) -> bool`
  - `owner_of(shim: &Path, bins: &[PmInfo]) -> Option<Pm>`

- [ ] **Step 1: 写失败测试**

在 `src/pm.rs` 的 `mod tests` 内追加：

```rust
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// 造一个假文件系统：给定存在的路径集合。
    fn exists_fn(set: HashSet<PathBuf>) -> impl Fn(&Path) -> bool {
        move |p: &Path| set.contains(p)
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn find_dsh_takes_the_first_hit_in_path_order() {
        // ★ 关键语义：PATH 顺序决定谁生效。
        // 本机实测 PATH 中 pnpm\bin 排在 npm 之前，若两者都有 dsh，pnpm 的会赢。
        let dirs = vec![p("C:/first"), p("C:/second")];
        let set: HashSet<PathBuf> = [p("C:/first/dsh.cmd"), p("C:/second/dsh.cmd")]
            .into_iter()
            .collect();
        assert_eq!(
            find_dsh_on_path(&dirs, &exists_fn(set)),
            Some(p("C:/first/dsh.cmd"))
        );
    }

    #[test]
    fn find_dsh_recognizes_exe_shim_for_bun() {
        // SHIM_NAMES 必须同时含 .exe 与 .cmd：bun 在 Windows 生成 .exe shim
        let dirs = vec![p("C:/bun/bin")];
        let set: HashSet<PathBuf> = [p("C:/bun/bin/dsh.exe")].into_iter().collect();
        assert_eq!(
            find_dsh_on_path(&dirs, &exists_fn(set)),
            Some(p("C:/bun/bin/dsh.exe"))
        );
    }

    #[test]
    fn find_dsh_returns_none_when_absent() {
        let dirs = vec![p("C:/a"), p("C:/b")];
        assert_eq!(find_dsh_on_path(&dirs, &exists_fn(HashSet::new())), None);
    }

    #[test]
    fn owner_of_matches_by_parent_directory() {
        let bins = vec![
            PmInfo { kind: Pm::Npm, version: "1".into(), bin_dir: p("C:/Users/x/AppData/Roaming/npm") },
            PmInfo { kind: Pm::Pnpm, version: "1".into(), bin_dir: p("C:/Users/x/AppData/Local/pnpm/bin") },
        ];
        assert_eq!(
            owner_of(&p("C:/Users/x/AppData/Roaming/npm/dsh.cmd"), &bins),
            Some(Pm::Npm)
        );
        assert_eq!(
            owner_of(&p("C:/Users/x/AppData/Local/pnpm/bin/dsh.exe"), &bins),
            Some(Pm::Pnpm)
        );
    }

    #[test]
    fn owner_of_is_case_insensitive_on_windows() {
        // Windows 路径大小写不敏感，必须归一化比较
        let bins = vec![PmInfo {
            kind: Pm::Npm,
            version: "1".into(),
            bin_dir: p("C:/Users/X/AppData/Roaming/NPM"),
        }];
        assert_eq!(
            owner_of(&p("c:/users/x/appdata/roaming/npm/dsh.cmd"), &bins),
            Some(Pm::Npm)
        );
    }

    #[test]
    fn owner_of_tolerates_trailing_separator() {
        let bins = vec![PmInfo {
            kind: Pm::Npm,
            version: "1".into(),
            bin_dir: p("C:/npm/"),
        }];
        assert_eq!(owner_of(&p("C:/npm/dsh.cmd"), &bins), Some(Pm::Npm));
    }

    #[test]
    fn owner_of_returns_none_for_unknown_location() {
        let bins = vec![PmInfo { kind: Pm::Npm, version: "1".into(), bin_dir: p("C:/npm") }];
        assert_eq!(owner_of(&p("D:/elsewhere/dsh.cmd"), &bins), None);
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test pm`
Expected: 编译失败 —— `find_dsh_on_path` / `owner_of` / `same_dir` 未定义。

- [ ] **Step 3: 实现**

在 `src/pm.rs` 的 `mod tests` 之前插入：

```rust
use std::path::{Path, PathBuf};

/// 在目录中查找 dsh 的可执行 shim。
pub fn shim_in(dir: &Path) -> Option<PathBuf> {
    SHIM_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|candidate| candidate.is_file())
}

/// 路径归一化比较：Windows 大小写不敏感，且需容忍尾部分隔符。
pub fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// FR-3 第 1~2 步：按 PATH 顺序找出第一个含 dsh shim 的完整路径。
///
/// **顺序即语义** —— 先命中的就是用户敲 `dsh` 时真正执行的那个。
/// 自己扫 PATH 而不用 `where.exe`：无外部进程依赖，且可用假 `exists`
/// 闭包做单元测试（SRS §8.4 要求）。
pub fn find_dsh_on_path(
    path_dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    for dir in path_dirs {
        for name in SHIM_NAMES {
            let candidate = dir.join(name);
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// FR-3 第 2 步：由 dsh shim 所在目录判断 owner PM。
pub fn owner_of(shim: &Path, bins: &[PmInfo]) -> Option<Pm> {
    let parent = shim.parent()?;
    bins.iter()
        .find(|info| same_dir(&info.bin_dir, parent))
        .map(|info| info.kind)
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test pm`
Expected: 12 passed

- [ ] **Step 5: 提交**

```bash
git add src/pm.rs
git commit -m "feat(pm): owner PM 判定（FR-3）

按 PATH 顺序取第一个 dsh shim，其所在目录决定 owner —— 顺序即
语义，先命中的就是用户敲 dsh 时真正执行的那个。本机实测
pnpm\\bin 排在 npm 之前，所以这个顺序是真有影响的。

自己扫 PATH 而非调 where.exe：无外部进程依赖，且可用假 exists
闭包单元测试。SHIM_NAMES 含 .exe（bun 在 Windows 生成 .exe shim）
与 .cmd（npm/pnpm/yarn）。"
```

---

### Task 7: `src/pm.rs` — 命令执行与探测编排（FR-1 / FR-2 / FR-4 / FR-5）

**Files:**
- Modify: `src/pm.rs`
- Create: `src/dsh.rs`（仅 `shim_in` 相关的版本读取部分；完整实现在 Task 11）

**Interfaces:**
- Consumes: Task 6 的 `shim_in`、`find_dsh_on_path`、`owner_of`；`model::{Pm, PmInfo, PmEnv, Version}`
- Produces:
  - `CmdOut { code: i32, stdout: String, stderr: String }`
  - `run_cmd(exe: &str, args: &[String]) -> Result<CmdOut, String>` —— **带 `CREATE_NO_WINDOW`**（GC-8）
  - `path_dirs() -> Vec<PathBuf>`
  - `probe_pm(pm: Pm) -> Option<PmInfo>`
  - `read_dsh_version_at(dir: &Path) -> Option<Version>`
  - `read_dsh_version_on_path() -> Option<(Pm, Version)>`
  - `probe_env() -> PmEnv`

- [ ] **Step 1: 写失败测试**

在 `src/pm.rs` 的 `mod tests` 内追加：

```rust
    #[test]
    fn path_dirs_is_nonempty_on_windows() {
        let dirs = path_dirs();
        assert!(!dirs.is_empty(), "PATH 不应为空");
        assert!(dirs.iter().all(|d| !d.as_os_str().is_empty()));
    }

    #[test]
    fn run_cmd_reports_nonzero_exit_without_erroring() {
        // 退出码非零是【正常结果】而不是 Err —— 事务补偿依赖这个区分
        // （SRS TR-11：对不存在的包执行卸载会非零退出，不该当成执行失败）
        let out = run_cmd("cmd.exe", &["/c".into(), "exit 3".into()]);
        assert!(out.is_ok(), "非零退出不应返回 Err");
        assert_eq!(out.unwrap().code, 3);
    }

    #[test]
    fn run_cmd_errors_only_when_exe_missing() {
        let out = run_cmd("definitely-not-a-real-exe-xyz.exe", &[]);
        assert!(out.is_err(), "可执行文件不存在才应是 Err");
    }

    #[test]
    fn run_cmd_captures_stdout() {
        let out = run_cmd("cmd.exe", &["/c".into(), "echo hello".into()]).unwrap();
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("hello"));
    }

    #[test]
    fn read_dsh_version_at_missing_dir_is_none() {
        assert_eq!(read_dsh_version_at(&p("C:/definitely/not/here")), None);
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test pm`
Expected: 编译失败 —— `path_dirs` / `run_cmd` / `read_dsh_version_at` 未定义。

- [ ] **Step 3: 实现**

在 `src/pm.rs` 的 `mod tests` 之前插入：

```rust
use std::process::{Command, Stdio};

/// CREATE_NO_WINDOW —— GC-8。缺了它每次调外部命令都会闪一个黑窗口。
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub struct CmdOut {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 执行外部命令。**参数以数组传递，绝不拼接 shell 字符串**（GC-9）。
///
/// 返回 `Err` 只表示"命令没能跑起来"（可执行文件不存在、权限不足等）。
/// **退出码非零是 `Ok`** —— 这个区分是必需的：SRS TR-11 要求补偿流程
/// 能识别"对不存在的包执行卸载"这类非零退出，若把它当成执行失败，
/// 就会误判为补偿失败并错误报告 Degraded。
pub fn run_cmd(exe: &str, args: &[String]) -> Result<CmdOut, String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd
        .output()
        .map_err(|e| format!("无法执行 {exe}: {e}"))?;
    Ok(CmdOut {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// 当前进程的 PATH 目录序列，保持顺序（顺序即语义，见 find_dsh_on_path）。
pub fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|v| std::env::split_paths(&v).collect())
        .unwrap_or_default()
}

/// FR-1 + FR-2：探测单个 PM。未安装返回 None。
pub fn probe_pm(pm: Pm) -> Option<PmInfo> {
    let ver = run_cmd(pm.exe(), &["--version".to_string()]).ok()?;
    if ver.code != 0 {
        return None;
    }
    let dir = run_cmd(pm.exe(), &pm.bin_dir_args().iter().map(|s| s.to_string()).collect())
        .ok()?;
    if dir.code != 0 {
        return None;
    }
    Some(PmInfo {
        kind: pm,
        version: ver.stdout.trim().to_string(),
        bin_dir: PathBuf::from(dir.stdout.trim()),
    })
}

/// FR-4 + TR-4：直接执行**指定目录下**的 dsh shim 读版本，**不经 PATH**。
///
/// TR-4 的强制要求：迁移验证绝不能用 PATH 解析。本机实测 PATH 中
/// pnpm\bin 排在 npm 之前，`pnpm → npm` 迁移时装完 npm 那份后
/// `where dsh` 仍指向 pnpm 的旧文件，验证会【假通过】。
pub fn read_dsh_version_at(dir: &Path) -> Option<Version> {
    let shim = shim_in(dir)?;
    let out = run_cmd(&shim.to_string_lossy(), &["--version".to_string()]).ok()?;
    if out.code != 0 {
        return None;
    }
    out.stdout.trim().parse().ok()
}

/// 经 PATH 解析后读版本。**仅用于事务的 S4 最终验证**。
pub fn read_dsh_version_on_path() -> Option<(Pm, Version)> {
    let shim = find_dsh_on_path(&path_dirs(), &|p| p.is_file())?;
    let ver: Version = run_cmd(&shim.to_string_lossy(), &["--version".to_string()])
        .ok()?
        .stdout
        .trim()
        .parse()
        .ok()?;
    let bins: Vec<PmInfo> = Pm::ALL.iter().filter_map(|pm| probe_pm(*pm)).collect();
    let owner = owner_of(&shim, &bins)?;
    Some((owner, ver))
}

/// FR-1 ~ FR-5：完整环境探测。
pub fn probe_env() -> PmEnv {
    let available: Vec<PmInfo> = Pm::ALL.iter().filter_map(|pm| probe_pm(*pm)).collect();
    let dsh_path = find_dsh_on_path(&path_dirs(), &|p| p.is_file());
    let owner = dsh_path.as_deref().and_then(|s| owner_of(s, &available));
    let installed = read_dsh_version_on_path().map(|(_, v)| v);
    PmEnv { available, owner, installed, dsh_path }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test pm`
Expected: 17 passed

> 若 `path_dirs_is_nonempty_on_windows` 失败，说明测试环境没有 PATH —— 属环境问题，不是代码问题。

- [ ] **Step 5: 提交**

```bash
git add src/pm.rs
git commit -m "feat(pm): 命令执行与完整环境探测（FR-1~FR-5）

run_cmd 的退出码区分是关键：非零退出返回 Ok，只有'命令没跑起来'
才是 Err。SRS TR-11 依赖这个区分 —— 对不存在的包执行卸载会非零
退出，若当成执行失败就会误判为补偿失败并错误报告 Degraded。

read_dsh_version_at 直接执行指定目录的 shim，不经 PATH（TR-4）。
read_dsh_version_on_path 仅用于 S4 最终验证。"
```

---

### Task 8: `src/txn.rs` — 类型、Backend trait 与前置检查

**Files:**
- Create: `src/txn.rs`
- Modify: `src/main.rs`（加 `mod txn;`）

**Interfaces:**
- Consumes: `model::*`、`pm::{read_dsh_version_at, read_dsh_version_on_path, run_cmd, path_dirs, same_dir}`
- Produces:
  - `trait Backend { fn run(&self, pm: Pm, args: &[String]) -> Result<CmdOut, String>; fn bin_dir(&self, pm: Pm) -> Result<PathBuf, String>; fn path_dirs(&self) -> Vec<PathBuf>; fn dsh_version_at(&self, dir: &Path) -> Option<Version>; fn dsh_version_on_path(&self) -> Option<(Pm, Version)>; fn log(&self, line: &str); }`
  - `is_safe_version(&str) -> bool`
  - `precheck<B: Backend>(b: &B, target: &Target) -> Option<RejectReason>`
  - `FakeBackend`（测试用，见 Step 1）

> **为什么这里用 trait**：这不是"未来可能有别的实现"的投机抽象，而是 SRS §8.4 强制要求的可测试性 —— V-17 / V-20 / V-21 三条关键验证必须能在假 backend 上构造失败路径。**这是全项目唯一的 trait。**

- [ ] **Step 1: 写失败测试（含假 backend，后续任务复用）**

创建 `src/txn.rs`：

```rust
//! 事务引擎：SRS §4 的 saga 实现。全项目唯一有破坏性后果的模块。
//!
//! 原子性契约（SRS §4.1）：要么成功，要么回到操作前状态；若连补偿都失败，
//! 明确报告降级状态并给出可复制的手动命令。绝不静默停在半成品状态。

use std::path::{Path, PathBuf};

use crate::model::*;
use crate::pm::CmdOut;

// ⚠ 下面三个只被 `#[cfg(test)]` 的 FakeBackend 使用。不加 gate 的话，
// `cargo build`（非 test 构建）会因它们未被引用而报 unused_imports ——
// 那是独立 lint，`allow(dead_code)` 不覆盖它。
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
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

#[cfg(test)]
pub struct FakeBackend {
    pub bins: HashMap<Pm, PathBuf>,
    pub path: Vec<PathBuf>,
    /// 各 PM 目录当前的 dsh 版本（None = 该 PM 上没装）
    pub installed: RefCell<HashMap<Pm, Option<Version>>>,
    /// 哪些 PM 的命令会失败
    pub fail_install: RefCell<Vec<Pm>>,
    pub fail_uninstall: RefCell<Vec<Pm>>,
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
    fn run(&self, pm: Pm, args: &[String]) -> Result<CmdOut, String> {
        self.calls.borrow_mut().push(format!("{pm:?} {}", args.join(" ")));
        if self.unavailable.borrow().contains(&pm) {
            return Err(format!("{pm:?} 不可用"));
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
        let b = FakeBackend::new();
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
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test txn`
Expected: 编译失败 —— `precheck` 未定义。

- [ ] **Step 3: 实现 `precheck`**

在 `src/txn.rs` 的 `#[cfg(test)] pub struct FakeBackend` **之前**插入：

```rust
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
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test txn`
Expected: 6 passed

- [ ] **Step 5: 提交**

```bash
git add src/txn.rs src/main.rs
git commit -m "feat(txn): 类型、Backend trait 与前置检查（TR-2/TR-3/NFR-6）

Backend trait 不是投机抽象，而是 SRS §8.4 强制要求的可测试性：
V-17/V-20/V-21 必须能在假 backend 上构造失败路径。这是全项目
唯一的 trait。

TR-3（目标 PM 的 bin 必须在 PATH 中）是阻断式的：不在 PATH 时
事务完成后 dsh 会从 PATH 消失，而这在动手前就能发现。"
```

---

### Task 9: `src/txn.rs` — 事务主流程

**Files:**
- Modify: `src/txn.rs`

**Interfaces:**
- Consumes: Task 8 的 `Backend` / `precheck` / `is_safe_version` / `FakeBackend`
- Produces:
  - `install<B: Backend>(b: &B, pm: Pm, v: &Version) -> Result<(), String>`
  - `uninstall<B: Backend>(b: &B, pm: Pm) -> Result<(), String>`
  - `run<B: Backend>(b: &B, origin: Origin, target: Target) -> TxOutcome`

- [ ] **Step 1: 写失败测试**

在 `src/txn.rs` 的 `mod tests` 内追加：

```rust
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
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
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

    /// ★ SRS V-19 / TR-4：S2 验证绝不经 PATH
    #[test]
    fn tr4_verify_does_not_use_path() {
        // 构造：装完后目标目录里【没有】dsh（模拟装坏），
        // 但 PATH 解析仍能拿到旧版本 —— 若实现误用 PATH 验证就会假通过。
        let b = FakeBackend::new().with_installed(Pm::Npm, "0.1.6-alpha.2");
        // Pnpm 不在 installed 里 → dsh_version_at(Pnpm) = None
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        let out = run(&b, origin, target);
        assert!(matches!(out, TxOutcome::RolledBack { failed: TxStep::S2Verify, .. }),
                "S2 必须发现自己装的版本不对，得到 {out:?}");
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test txn`
Expected: 编译失败 —— `run` / `install` / `uninstall` 未定义。

- [ ] **Step 3: 实现**

在 `src/txn.rs` 的 `mod tests` 之前插入：

```rust
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
    if b.dsh_version_at(&target_dir).as_ref() != Some(&target.version) {
        let got = b.dsh_version_at(&target_dir);
        return compensate(
            b,
            origin,
            target,
            TxStep::S2Verify,
            format!("新安装的版本不符：期望 {}，实际 {got:?}", target.version),
        );
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
```

**注意**：`compensate` 在 Task 10 实现。为让本任务可编译，先加占位实现 —— 但**不要提交带占位符的版本**，正确做法是**把 Task 9 与 Task 10 合并执行**。因此本步骤实际要求：先写下面这个最简 `compensate`（仅返回 `RolledBack`），跑通 Task 9 的测试后立即进入 Task 10 替换为完整实现。

```rust
/// ← Task 10 会用完整实现替换。Task 9 阶段先让编译通过并跑通主流程测试。
fn compensate<B: Backend>(
    _b: &B,
    origin: Origin,
    _target: Target,
    failed: TxStep,
    detail: String,
) -> TxOutcome {
    TxOutcome::RolledBack { failed, restored: origin, detail }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test txn`
Expected: 11 passed

> 若 `v20_s1_failure_touches_nothing` 失败：检查 `FakeBackend::run` 是否真的对 `fail_install` 里的 PM 返回了非零退出码。当前 FakeBackend 的 `fail_install` 字段尚未被 `run` 使用 —— **需要先在 `FakeBackend::run` 里补上**：
>
> ```rust
> if args.iter().any(|a| a == "add" || a == "install")
>     && self.fail_install.borrow().contains(&pm) {
>     return Ok(CmdOut { code: 1, stdout: String::new(), stderr: "模拟安装失败".into() });
> }
> if args.iter().any(|a| a == "remove" || a == "uninstall")
>     && self.fail_uninstall.borrow().contains(&pm) {
>     return Ok(CmdOut { code: 1, stdout: String::new(), stderr: "模拟卸载失败".into() });
> }
> ```
> 放在 `run` 中 `unavailable` 检查之后、返回 `code: 0` 之前。

- [ ] **Step 5: 提交**（与 Task 10 合并提交，见 Task 10 Step 5）

---

### Task 10: `src/txn.rs` — 补偿流程

**Files:**
- Modify: `src/txn.rs`

**Interfaces:**
- Consumes: Task 9 的 `run` / `install` / `uninstall`
- Produces: 完整的 `compensate<B: Backend>(...) -> TxOutcome`、`manual_commands(...) -> Vec<String>`

- [ ] **Step 1: 写失败测试**

在 `src/txn.rs` 的 `mod tests` 内追加：

```rust
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
        // 补偿必须清掉 pnpm 上的残留
        assert!(b.called("Pnpm") && b.called("remove"), "应清理 pnpm 残留");
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
        // pnpm 上从来没有 dsh（不是 None，而是压根不在 installed 里）
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

    #[test]
    fn rejected_outcome_is_not_confused_with_rolled_back() {
        let b = FakeBackend::new();
        b.unavailable.borrow_mut().push(Pm::Pnpm);
        let origin = Origin { pm: Pm::Npm, version: v("0.1.6-alpha.2") };
        let target = Target { pm: Pm::Pnpm, version: v("0.1.6-alpha.2") };
        assert!(matches!(run(&b, origin, target), TxOutcome::Rejected { .. }));
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test txn`
Expected: FAIL —— `v17` 与 `tr11` 失败（当前简版 `compensate` 无条件返回 `RolledBack`，不会做 C1 探测与 C2 清理）；`degraded_outcome_carries_runnable_manual_commands` 编译失败（`manual_commands` 未定义）。

- [ ] **Step 3: 用完整实现替换 Task 9 的简版 `compensate`**

```rust
/// 手动恢复命令。FR-32 要求降级报告给出**可直接复制执行**的命令。
pub fn manual_commands(origin: &Origin, target: &Target) -> Vec<String> {
    vec![
        format!(
            "{} {}",
            origin.pm.exe(),
            origin.pm.install_args(&origin.version.to_string()).join(" ")
        ),
        format!("{} {}", target.pm.exe(), target.pm.uninstall_args().join(" ")),
    ]
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
        if install(b, origin.pm, &origin.version).is_err() {
            b.log("补偿：重装 origin 失败，进入降级");
            return degraded(&origin, &target, failed, detail);
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
            if uninstall(b, target.pm).is_err() {
                b.log("补偿：清理失败，进入降级");
                return degraded(&origin, &target, failed, detail);
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
```

- [ ] **Step 4: 运行全部测试，确认通过**

Run: `cargo test`
Expected: 全部通过（含此前各任务的测试）

- [ ] **Step 5: 提交**（合并 Task 9 + Task 10）

```bash
git add src/txn.rs
git commit -m "feat(txn): 事务主流程与补偿（SRS §4 完整实现）

主流程：前置检查 → S1 装新的 → S2 验证（不走 PATH）→ S3 卸旧的
（同 PM 跳过）→ S4 最终验证。

补偿：C1 先探测 origin 完好性 → C2b 坏了就重装 origin → C2a 先探测
再清理 target 残留 → C3 确认已恢复。

三条关键约束的实现与验证：
- TR-4  S2 绝不走 PATH（V-19 测试：构造装坏但 PATH 仍能拿到旧版本的
        场景，误用 PATH 会假通过）
- TR-6  先装后卸（V-20 测试：S1 失败时断言卸载从未被调用）
- TR-5  C1 先探测再动（V-21 测试：origin 已损坏时断言绝不卸载 target，
        否则旧的坏了新的也没了）
- TR-11 补偿先探测再动作（测试：target 无残留时断言跳过卸载，
        否则非零退出会被误判为补偿失败）"
```

---

# 阶段 3：系统交互层

**阶段目标**：网络拉取与 `dsh web` 进程监督。这些是 I/O，单元测试覆盖有限，因此每个任务都要求**手动验证步骤**。

> **已核实的 ureq 3.x API**（与 2.x 有破坏性差异，勿凭记忆写）：
> ```rust
> // 请求头用 .header()，不是 2.x 的 .set()
> // 响应体用 .body_mut().read_to_string()
> let body: String = agent.get(url).header("Accept", v).call()?.body_mut().read_to_string()?;
> // 超时
> let agent: Agent = Agent::config_builder().timeout_global(Some(d)).build().into();
> // 4xx/5xx 默认变成 Err(Error::StatusCode(code)) —— 404 判定依赖此行为
> ```
> **默认 features = `['rustls', 'gzip']`**，TLS 走 rustls + webpki-roots（内置 Mozilla 根证书），**HTTPS 开箱可用**，无需 `platform-verifier`。（这关闭了 ARCHITECTURE §9.1 的风险 R-5）

---

### Task 11: `src/dsh.rs` — registry 拉取（FR-6）

**Files:**
- Create: `src/dsh.rs`
- Modify: `src/main.rs`（加 `mod dsh;`）

**Interfaces:**
- Consumes: `model::{Catalog, Version}`、`pm::sorted_desc`
- Produces:
  - `REGISTRY_URL: &str`、`GITHUB_REPO: &str`、`USER_AGENT: &str`
  - `agent() -> ureq::Agent`（全局超时 15s）
  - `parse_catalog(&str) -> Result<Catalog, String>` —— **纯函数**
  - `fetch_catalog() -> Result<Catalog, String>`

- [ ] **Step 1: 写失败测试**

创建 `src/dsh.rs`：

```rust
//! 版本拉取、更新说明与 `dsh web` 进程监督。

use std::collections::BTreeMap;
use std::time::Duration;

use crate::model::*;
use crate::pm;

pub const REGISTRY_URL: &str = "https://registry.npmjs.org/@deepseek-ai/dsh";
pub const GITHUB_REPO: &str = "deepseek-ai/deepseek-harness";
/// GC-5：只允许 registry.npmjs.org 与 api.github.com。github.com 实测不可达。
pub const USER_AGENT: &str = "dsh-manager";

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    /// 取自 registry 真实响应的精简形态
    const FIXTURE: &str = r#"{
      "name": "@deepseek-ai/dsh",
      "dist-tags": { "latest": "0.1.5-rc.2", "next": "0.1.5-rc.2", "alpha": "0.1.6-alpha.2" },
      "versions": {
        "0.1.5-rc.2": { "name": "@deepseek-ai/dsh", "version": "0.1.5-rc.2" },
        "0.1.6-alpha.1": { "name": "@deepseek-ai/dsh", "version": "0.1.6-alpha.1" },
        "0.1.6-alpha.2": { "name": "@deepseek-ai/dsh", "version": "0.1.6-alpha.2" }
      }
    }"#;

    #[test]
    fn parse_catalog_extracts_versions_descending() {
        let c = parse_catalog(FIXTURE).unwrap();
        assert_eq!(c.versions.len(), 3);
        assert_eq!(c.versions[0], v("0.1.6-alpha.2"), "最新应在最前");
        assert_eq!(c.versions[2], v("0.1.5-rc.2"));
    }

    #[test]
    fn parse_catalog_extracts_dist_tags() {
        let c = parse_catalog(FIXTURE).unwrap();
        assert_eq!(c.tags.get("latest"), Some(&v("0.1.5-rc.2")));
        assert_eq!(c.tags.get("alpha"), Some(&v("0.1.6-alpha.2")));
    }

    /// GC-14 的端到端回归：从真实形状的响应出发，
    /// 走完 parse → channel_of → latest_in，必须得到 0.1.6-alpha.2 而非 latest tag。
    #[test]
    fn gc14_latest_tag_must_not_be_used_as_newest() {
        let c = parse_catalog(FIXTURE).unwrap();
        let installed = v("0.1.6-alpha.2");
        let ch = pm::channel_of(&installed);
        let newest = pm::latest_in(&c.versions, ch).unwrap();
        assert_eq!(*newest, installed, "已是最新");
        assert_ne!(
            newest,
            c.tags.get("latest").unwrap(),
            "若这里相等，说明实现误用了 latest tag"
        );
    }

    #[test]
    fn parse_catalog_tolerates_missing_fields() {
        let c = parse_catalog("{}").unwrap();
        assert!(c.versions.is_empty());
        assert!(c.tags.is_empty());
    }

    #[test]
    fn parse_catalog_rejects_invalid_json() {
        assert!(parse_catalog("not json").is_err());
    }

    #[test]
    fn parse_catalog_skips_unparseable_version_keys() {
        let c = parse_catalog(r#"{"versions":{"not-a-version":{},"1.2.3":{}}}"#).unwrap();
        assert_eq!(c.versions, vec![v("1.2.3")]);
    }
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test dsh`
Expected: 编译失败 —— `parse_catalog` 未定义。

- [ ] **Step 3: 实现**

在 `src/dsh.rs` 的 `#[cfg(test)] mod tests` 之前插入：

```rust
/// NFR-3：全局超时上限，超时后进入失败路径而非无限等待。
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// 纯函数，可用 fixture 单元测试。
pub fn parse_catalog(body: &str) -> Result<Catalog, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("registry 响应解析失败: {e}"))?;

    let mut versions: Vec<Version> = Vec::new();
    if let Some(obj) = v.get("versions").and_then(|x| x.as_object()) {
        for key in obj.keys() {
            if let Ok(ver) = key.parse::<Version>() {
                versions.push(ver);
            }
        }
    }

    let mut tags: BTreeMap<String, Version> = BTreeMap::new();
    if let Some(obj) = v.get("dist-tags").and_then(|x| x.as_object()) {
        for (key, val) in obj {
            if let Some(s) = val.as_str() {
                if let Ok(ver) = s.parse::<Version>() {
                    tags.insert(key.clone(), ver);
                }
            }
        }
    }

    Ok(Catalog { versions: pm::sorted_desc(versions), tags })
}

/// FR-6。精简 packument 请求头可显著减小响应体积 —— 本需求只需要
/// `versions` 的键集合与 `dist-tags`，不需要每个版本的完整元数据。
pub fn fetch_catalog() -> Result<Catalog, String> {
    let body = agent()
        .get(REGISTRY_URL)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("registry 请求失败: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("registry 响应读取失败: {e}"))?;
    parse_catalog(&body)
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test dsh`
Expected: 6 passed

- [ ] **Step 5: 手动验证真实网络拉取**

临时把 `src/main.rs` 的 `main` 改为打印 catalog 再退出（验证完**记得改回**）：

```rust
fn main() -> Result<(), slint::PlatformError> {
    match dsh::fetch_catalog() {
        Ok(c) => {
            println!("版本数 = {}", c.versions.len());
            println!("最新(降序首个) = {}", c.versions.first().unwrap());
            println!("dist-tags = {:?}", c.tags);
        }
        Err(e) => println!("失败: {e}"),
    }
    Ok(())
}
```

Run: `cargo run`
Expected（对照 SRS §2.2.5 的实测值）：
- `版本数 = 22`
- `dist-tags` 含 `latest: 0.1.5-rc.2`、`alpha: 0.1.6-alpha.2`
- 耗时约 4 秒（SRS §2.2.7 实测 registry 响应 4.1s）

**验证完把 `main` 改回 Task 1 的空窗口版本。**

- [ ] **Step 6: 提交**

```bash
git add src/dsh.rs src/main.rs
git commit -m "feat(dsh): registry 版本拉取（FR-6）

parse_catalog 是可单测的纯函数，fetch_catalog 只负责 HTTP。

含 GC-14 的端到端回归测试：从真实形状的响应走完
parse → channel_of → latest_in，断言结果不等于 latest tag。
若实现误用 latest tag，这个测试会直接红。

请求带 Accept: application/vnd.npm.install-v1+json 取精简
packument —— 只需 versions 键集合与 dist-tags。"
```

---

### Task 12: `src/dsh.rs` — 更新说明拉取与预处理（FR-26 / FR-27）

**Files:**
- Modify: `src/dsh.rs`

**Interfaces:**
- Consumes: Task 11 的 `agent()` / `GITHUB_REPO` / `USER_AGENT`
- Produces:
  - `strip_html(&str) -> String` —— 纯函数
  - `heading_body(&str) -> Option<&str>` —— 纯函数
  - `preprocess_notes(&str) -> String` —— 纯函数
  - `parse_release_body(&str) -> Result<String, NotesError>` —— 纯函数
  - `fetch_notes(&Version) -> Result<String, NotesError>`

- [ ] **Step 1: 写失败测试**

在 `src/dsh.rs` 的 `mod tests` 内追加：

```rust
    #[test]
    fn strip_html_removes_tags_but_keeps_inner_text() {
        // DSH 的 release notes 正文混有裸 HTML，如 <h3 id="cn-...">新增功能</h3>
        assert_eq!(strip_html(r#"<h3 id="cn-v0.1.6-alpha.2">新增功能</h3>"#), "新增功能");
        assert_eq!(strip_html("无标签"), "无标签");
        assert_eq!(strip_html("<b>粗</b>体"), "粗体");
    }

    #[test]
    fn heading_body_detects_atx_headings() {
        assert_eq!(heading_body("### 新增功能"), Some(" 新增功能"));
        assert_eq!(heading_body("# 一级"), Some(" 一级"));
        assert_eq!(heading_body("###### 六级"), Some(" 六级"));
        // 非标题
        assert_eq!(heading_body("普通文本"), None);
        assert_eq!(heading_body("####### 七个井号不是标题"), None);
        assert_eq!(heading_body("#没空格不是标题"), None);
        assert_eq!(heading_body("### "), None, "空标题按普通文本处理");
    }

    /// FR-27 的核心：StyledText 不支持标题与 HTML 标签。
    /// 不预处理的话，release notes 会显示成 `### 新增功能` 和
    /// `<h3 id="...">` 这样的垃圾文本。
    #[test]
    fn preprocess_converts_headings_to_bold_and_strips_html() {
        let input = "<h3 id=\"cn-x\">新增功能</h3>\n\n### 体验优化\n\n- 某条目\n";
        let out = preprocess_notes(input);
        assert!(!out.contains('<'), "HTML 标签应被剥离，得到 {out:?}");
        assert!(!out.contains("###"), "ATX 标题应被转换，得到 {out:?}");
        assert!(out.contains("**新增功能**"), "得到 {out:?}");
        assert!(out.contains("**体验优化**"), "得到 {out:?}");
        assert!(out.contains("- 某条目"), "列表语法应保持原样交给 StyledText");
    }

    #[test]
    fn preprocess_preserves_bilingual_nav_and_links() {
        // StyledText 原生支持链接，不该破坏它们
        let input = "[中文](#cn-x) | [English](#en-x)\n";
        let out = preprocess_notes(input);
        assert!(out.contains("[中文](#cn-x)"), "链接应原样保留，得到 {out:?}");
    }

    #[test]
    fn preprocess_handles_empty_input() {
        assert_eq!(preprocess_notes(""), "");
    }

    #[test]
    fn parse_release_body_extracts_body_field() {
        let json = r#"{"tag_name":"dsh-v0.1.6-alpha.2","body":"## 标题\n内容"}"#;
        assert_eq!(parse_release_body(json).unwrap(), "## 标题\n内容");
    }

    #[test]
    fn parse_release_body_treats_null_body_as_missing() {
        // 有些 release 的 body 是 null
        let json = r#"{"tag_name":"dsh-v0.0.1-rc.1","body":null}"#;
        assert!(matches!(parse_release_body(json), Err(NotesError::Missing)));
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test dsh`
Expected: 编译失败 —— `strip_html` / `heading_body` / `preprocess_notes` / `parse_release_body` 未定义。

- [ ] **Step 3: 实现**

在 `src/dsh.rs` 的 `#[cfg(test)] mod tests` 之前插入：

```rust
/// 删除裸 HTML 标签，**保留标签内的文本**。
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// 识别 ATX 标题，返回井号之后的原文（含前导空格）。空标题返回 None。
fn heading_body(s: &str) -> Option<&str> {
    let hashes = s.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    // '#' 是 ASCII，字节索引等于字符数，安全
    let rest = &s[hashes..];
    if !rest.starts_with(' ') || rest.trim().is_empty() {
        return None;
    }
    Some(rest)
}

/// FR-27 的强制预处理。
///
/// Slint 的 `StyledText` 官方 Currently Unsupported 列表包含 **Headings** 与
/// **Other HTML tags**。而 DSH 的 release notes 通篇是 `### 新增功能` 这类 ATX
/// 标题，且混有 `<h3 id="...">` 裸 HTML。不预处理就会原样显示成垃圾文本。
///
/// 只做两件事：剥离 HTML 标签、ATX 标题降级为粗体。
/// 其余语法（粗体 / 斜体 / 行内代码 / **链接** / 列表）由 StyledText 原生支持，
/// **不做干预** —— 尤其不要破坏链接。
pub fn preprocess_notes(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 32);
    for line in md.lines() {
        let stripped = strip_html(line);
        match heading_body(stripped.trim_start()) {
            Some(rest) => {
                out.push_str("**");
                out.push_str(rest.trim());
                out.push_str("**\n");
            }
            None => {
                out.push_str(&stripped);
                out.push('\n');
            }
        }
    }
    out
}

/// 从 GitHub release 响应中取 `body`。纯函数，可单测。
pub fn parse_release_body(json: &str) -> Result<String, NotesError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| NotesError::Net(e.to_string()))?;
    match v.get("body").and_then(|b| b.as_str()) {
        Some(b) if !b.trim().is_empty() => Ok(b.to_string()),
        // body 为 null 或空 —— 与"该版本无 release"对用户是同一件事
        _ => Err(NotesError::Missing),
    }
}

/// FR-26。404 是**正常情况**：npm 有 22 个版本，GitHub 只有 18 个 release，
/// 有 6 个 npm 版本根本没有更新说明（SRS §2.2.6）。
pub fn fetch_notes(version: &Version) -> Result<String, NotesError> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/tags/dsh-v{version}");
    let mut resp = match agent()
        .get(&url)
        .header("User-Agent", USER_AGENT) // GitHub API 对无 UA 的请求返回 403
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        // ureq 3 默认把 4xx/5xx 变成 Err(StatusCode)
        Err(ureq::Error::StatusCode(404)) => return Err(NotesError::Missing),
        Err(e) => return Err(NotesError::Net(e.to_string())),
    };
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| NotesError::Net(e.to_string()))?;
    parse_release_body(&body)
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test dsh`
Expected: 14 passed

- [ ] **Step 5: 手动验证真实拉取**

临时改 `main`（验证后改回）：

```rust
fn main() {
    let v: dsh::Version = "0.1.6-alpha.2".parse().unwrap();
    println!("--- 有说明的版本 ---");
    println!("{:?}", dsh::fetch_notes(&v).map(|s| s.chars().take(120).collect::<String>()));

    let missing: dsh::Version = "0.0.1-rc.1".parse().unwrap();
    println!("--- 无说明的版本（SRS §2.2.6 的 6 个之一）---");
    println!("{:?}", dsh::fetch_notes(&missing));
}
```

Run: `cargo run`
Expected:
- 第一个版本返回 `Ok(...)`，内容含 `新增功能`（中英双语正文的开头）
- 第二个版本返回 `Err(Missing)` —— **这是正常结果，不是 bug**

- [ ] **Step 6: 提交**

```bash
git add src/dsh.rs src/main.rs
git commit -m "feat(dsh): 更新说明拉取与预处理（FR-26/FR-27）

预处理是 FR-27 的强制要求：StyledText 官方 Unsupported 列表含
Headings 与 Other HTML tags，而 DSH 的 release notes 通篇是
'### 新增功能' 加 <h3 id=...> 裸 HTML，不处理会显示成垃圾文本。

只做两件事（剥离标签、标题转粗体），链接与列表原样透传给
StyledText —— 尤其不破坏 [中文](#cn-..) 这类导航链接。

404 映射为 NotesError::Missing 而非错误：npm 有 22 个版本但
GitHub 只有 18 个 release，6 个版本本就无说明。"
```

---

### Task 13: `src/dsh.rs` — 端口探测与外部进程定位（FR-17 / FR-19 / NFR-7）

**Files:**
- Modify: `src/dsh.rs`

**Interfaces:**
- Consumes: `pm::run_cmd`
- Produces:
  - `port_in_use(port: u16) -> bool`
  - `parse_netstat_pid(text: &str, port: u16) -> Option<u32>` —— 纯函数
  - `find_listener_pid(port: u16) -> Result<u32, String>`
  - `is_node(pid: u32) -> bool`

- [ ] **Step 1: 写失败测试**

在 `src/dsh.rs` 的 `mod tests` 内追加：

```rust
    /// 取自 `netstat -ano` 的真实输出形状（本机实测 3080 被 dsh web 占用）
    const NETSTAT: &str = "\
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1768
  TCP    127.0.0.1:3080         0.0.0.0:0              LISTENING       13432
  TCP    127.0.0.1:3080         127.0.0.1:1716         TIME_WAIT       0
  TCP    [::]:445                [::]:0                 LISTENING       4
";

    #[test]
    fn parse_netstat_finds_listening_pid() {
        assert_eq!(parse_netstat_pid(NETSTAT, 3080), Some(13432));
    }

    #[test]
    fn parse_netstat_ignores_time_wait_rows() {
        // TIME_WAIT 那行的 PID 是 0，必须靠 LISTENING 过滤掉
        assert_ne!(parse_netstat_pid(NETSTAT, 3080), Some(0));
    }

    #[test]
    fn parse_netstat_does_not_match_port_suffix() {
        // ":3080" 不得匹配 ":13080" —— 必须按 ':' 切分后比较末段
        let text = "  TCP    127.0.0.1:13080        0.0.0.0:0              LISTENING       999\n";
        assert_eq!(parse_netstat_pid(text, 3080), None);
    }

    #[test]
    fn parse_netstat_returns_none_when_absent() {
        assert_eq!(parse_netstat_pid(NETSTAT, 9999), None);
    }

    #[test]
    fn parse_netstat_handles_empty_input() {
        assert_eq!(parse_netstat_pid("", 3080), None);
    }
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test dsh`
Expected: 编译失败 —— `parse_netstat_pid` 未定义。

- [ ] **Step 3: 实现**

在 `src/dsh.rs` 的 `#[cfg(test)] mod tests` 之前插入：

```rust
use std::net::{SocketAddr, TcpStream};

/// FR-17：端口占用探测。stdlib 实现，无依赖。
pub fn port_in_use(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// 解析 `netstat -ano`，找出监听指定端口的 PID。纯函数。
///
/// 三个必须处理的细节：
/// 1. 必须用 LISTENING 过滤 —— TIME_WAIT 行也含该端口，但其 PID 列是 0
/// 2. 必须按 ':' 切分比较端口末段 —— 否则 ":3080" 会匹配 ":13080"
/// 3. 本地地址可能是 `127.0.0.1:3080` 或 `[::]:3080`，都按末段处理
pub fn parse_netstat_pid(text: &str, port: u16) -> Option<u32> {
    let target = port.to_string();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 5 {
            continue;
        }
        if !cols[0].eq_ignore_ascii_case("TCP") {
            continue;
        }
        if !cols[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        if cols[1].rsplit(':').next() != Some(target.as_str()) {
            continue;
        }
        if let Ok(pid) = cols[4].parse::<u32>() {
            return Some(pid);
        }
    }
    None
}

/// FR-19：定位监听指定端口的进程。
pub fn find_listener_pid(port: u16) -> Result<u32, String> {
    let out = pm::run_cmd("netstat.exe", &["-ano".to_string()])?;
    parse_netstat_pid(&out.stdout, port)
        .ok_or_else(|| format!("未找到监听端口 {port} 的进程"))
}

/// NFR-7：终止前必须校验进程名，避免误杀占用同端口的其他程序。
pub fn is_node(pid: u32) -> bool {
    let args = vec![
        "/FI".to_string(),
        format!("PID eq {pid}"),
        "/FO".to_string(),
        "CSV".to_string(),
        "/NH".to_string(),
    ];
    match pm::run_cmd("tasklist.exe", &args) {
        Ok(out) => out.stdout.to_ascii_lowercase().contains("node.exe"),
        Err(_) => false,
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test dsh`
Expected: 19 passed

- [ ] **Step 5: 提交**

```bash
git add src/dsh.rs
git commit -m "feat(dsh): 端口探测与外部进程定位（FR-17/FR-19/NFR-7）

parse_netstat_pid 抽成纯函数以便单测，覆盖三个真实陷阱：
- 必须用 LISTENING 过滤（TIME_WAIT 行也含该端口但 PID 为 0）
- 必须按 ':' 切分比较末段（否则 :3080 会匹配 :13080）
- 本地地址可能是 127.0.0.1:3080 或 [::]:3080

is_node 实现 NFR-7：终止外部进程前校验进程名，避免误杀占用
同端口的其他程序。"
```

---

### Task 14: `src/dsh.rs` — `dsh web` 进程监督（FR-16 / FR-20 / FR-21）

**Files:**
- Modify: `src/dsh.rs`

**Interfaces:**
- Consumes: Task 13 的 `port_in_use`；`pm::{run_cmd, CREATE_NO_WINDOW}`；`model::{Job, UiMsg}`
- Produces:
  - `spawn_reader<R: Read + Send + 'static>(Option<R>, Sender<UiMsg>) -> JoinHandle<()>`
  - `spawn_web(port: u16, tx: Sender<UiMsg>) -> Result<u32, String>`
  - `wait_port_ready(port: u16, alive: impl Fn() -> bool, timeout: Duration) -> bool`
  - `stop_by_pid(pid: u32) -> Result<(), String>`
  - `open_url(url: &str) -> Result<(), String>`

- [ ] **Step 1: 实现（本任务为 I/O，无可单测的纯逻辑）**

在 `src/dsh.rs` 顶部补充 use，并在 `mod tests` 之前插入：

```rust
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::Instant;

/// 读一个管道到 EOF，逐行投递为 `UiMsg::Log`。
///
/// 泛型化是因为 `ChildStdout` 与 `ChildStderr` 是两个不同的类型 —— 它们都
/// 实现了 `Read + Send + 'static`。
fn spawn_reader<R: Read + Send + 'static>(
    src: Option<R>,
    tx: Sender<UiMsg>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let Some(src) = src else { return };
        for line in BufReader::new(src).lines().map_while(Result::ok) {
            if tx.send(UiMsg::Log(line)).is_err() {
                break; // UI 侧已关闭
            }
        }
    })
}

/// FR-16：启动 `dsh web`。
///
/// 返回 pid。`Child` 句柄被 move 进内部的 waiter 线程，**不跨线程共享** ——
/// 因此没有任何锁。停止只依赖 pid（见 `stop_by_pid`）。
///
/// **不传 `--no-open`**：让 `dsh web` 自己打开浏览器，本项目零代码实现 FR-20
/// 的自动打开。
pub fn spawn_web(port: u16, tx: Sender<UiMsg>) -> Result<u32, String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new("dsh.cmd"); // GC-7：必须是 shim 全名
    cmd.args(["web", "--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(pm::CREATE_NO_WINDOW); // GC-8

    let mut child = cmd.spawn().map_err(|e| format!("启动 dsh web 失败: {e}"))?;
    let pid = child.id();

    let out = child.stdout.take();
    let err = child.stderr.take();

    // 必须两个独立线程读：管道缓冲区满时写端会阻塞，单线程串行读另一个
    // 管道会被写满，导致子进程卡死 —— 经典死锁。
    let h_out = spawn_reader(out, tx.clone());
    let h_err = spawn_reader(err, tx.clone());

    // waiter 先 join 两个 reader 再 wait()：保证子进程退出时输出已被完整读取，
    // 否则日志会截尾。
    thread::spawn(move || {
        let _ = h_out.join();
        let _ = h_err.join();
        let code = child.wait().ok().and_then(|s| s.code());
        let _ = tx.send(UiMsg::WebExited { code });
    });

    Ok(pid)
}

/// FR-16：等待端口就绪。
///
/// 用 TCP 探测而不解析 `dsh web` 的 stdout —— 后者依赖 dsh 的输出格式
/// （SRS AS-3 已将其列为假设），而"端口最终会监听"是稳定事实。
pub fn wait_port_ready(
    port: u16,
    alive: impl Fn() -> bool,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if port_in_use(port) {
            return true;
        }
        if !alive() {
            return false; // 进程已退出，不会再就绪
        }
        thread::sleep(Duration::from_millis(200));
    }
    false
}

/// FR-19 / FR-21：统一停止入口。
///
/// 自己启动的与外部启动的走**同一条路径** —— 区别仅在于 pid 从
/// `child.id()` 来还是从 `find_listener_pid()` 来。
///
/// 用 `taskkill /T` 而非 `Child::kill()`：后者底层是 `TerminateProcess`，
/// 只杀单个进程；`/T` 终止整棵进程树，避免残留子进程继续占着端口。
pub fn stop_by_pid(pid: u32) -> Result<(), String> {
    let args = vec![
        "/PID".to_string(),
        pid.to_string(),
        "/T".to_string(),
        "/F".to_string(),
    ];
    let out = pm::run_cmd("taskkill.exe", &args)?;
    if out.code == 0 {
        return Ok(());
    }
    // 进程已不存在视为成功（幂等）
    let msg = format!("{}{}", out.stdout, out.stderr);
    if msg.contains("not found") || msg.contains("没有找到") || msg.contains("找不到") {
        return Ok(());
    }
    Err(format!("taskkill 退出码 {}: {}", out.code, msg.trim()))
}

/// FR-20：在默认浏览器打开 URL。
/// `start` 的第一个参数是窗口标题占位符，缺了它带引号的 URL 会被当成标题。
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new("cmd.exe");
    cmd.args(["/c", "start", "", url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(pm::CREATE_NO_WINDOW);
    cmd.spawn().map_err(|e| format!("打开浏览器失败: {e}"))?;
    Ok(())
}
```

- [ ] **Step 2: 编译并跑全部单元测试**

Run: `cargo test`
Expected: 全部通过（本任务未新增测试，但不得破坏既有测试）

- [ ] **Step 3: 手动验证 —— 无黑窗口启动（GC-8 的关键验证）**

临时改 `main`：

```rust
fn main() {
    let (tx, rx) = std::sync::mpsc::channel();
    let port = 3099; // 用非默认端口，避免干扰本机正在跑的 3080
    let pid = dsh::spawn_web(port, tx).unwrap();
    println!("已启动 pid={pid}");
    let ready = dsh::wait_port_ready(port, || true, std::time::Duration::from_secs(30));
    println!("端口就绪 = {ready}");
    std::thread::spawn(move || {
        for msg in rx {
            println!("{msg:?}");
        }
    });
    std::thread::sleep(std::time::Duration::from_secs(5));
    dsh::stop_by_pid(pid).unwrap();
    println!("已停止");
}
```

Run: `cargo run`
Expected:
1. 浏览器自动打开 `http://127.0.0.1:3099`（证明不传 `--no-open` 是对的）
2. `端口就绪 = true`
3. **任务栏不出现任何新的控制台窗口**（GC-8 的验证）
4. 日志行被打印（子进程 stdout 流）
5. `已停止`，且 `netstat -ano | findstr :3099` 无 LISTENING 行

> **注意**：临时给 `main` 加了 `println!`，但 `#![cfg_attr(not(test), windows_subsystem = "windows")]` 会让 stdout 不可见。验证时**临时注释掉该属性**，验证完恢复。

- [ ] **Step 4: 手动验证 —— 外部进程定位与误杀防护**

```bash
# 终端 A：手动起一个 dsh web（模拟"非本程序启动"）
dsh web --port 3098 --no-open
# 终端 B：确认能定位到它
netstat -ano | findstr :3098
tasklist /FI "PID eq <上面查到的PID>" /FO CSV /NH
```

Expected: `tasklist` 输出 `"node.exe"` —— 证明 `is_node` 的判据成立。

- [ ] **Step 5: 提交**

```bash
git add src/dsh.rs src/main.rs
git commit -m "feat(dsh): dsh web 进程监督（FR-16/FR-20/FR-21）

Child 句柄被 move 进 waiter 线程，不跨线程共享 —— 因此无锁。
停止统一走 taskkill /PID /T /F，只需 pid：自己启动的与外部启动的
因此合并成同一条路径。

两个管道必须独立线程读：单线程串行读时，先读的管道阻塞会让另一个
管道缓冲区写满，子进程卡死（经典死锁）。waiter 先 join 两个 reader
再 wait()，否则日志会截尾。

不传 --no-open，让 dsh web 自己开浏览器，零代码实现 FR-20。"
```

---

# 阶段 4：界面与集成

**阶段目标**：把阶段 2、3 的能力接到 GUI 上，得到完整可用的程序。

---

### Task 15: `ui/app.slint` — MainWindow 完整界面

**Files:**
- Modify: `ui/app.slint`（替换 Task 1 的最小版本）

**Interfaces:**
- Consumes: 无
- Produces: `MainWindow` 组件及其完整属性/回调契约（见下方 markup）

- [ ] **Step 1: 写 `ui/app.slint`**

```slint
// GC-13：std-widgets 不是语言内建元素，必须显式 import。
// 漏掉会报 "Unknown element 'ListView'"，且错误发生在 build.rs 阶段。
import { Button, ComboBox, LineEdit, ListView, ScrollView, AboutSlint } from "std-widgets.slint";

export enum NotesStatus { loading, ok, missing, failed }

export component MainWindow inherits Window {
    title: "DSH Manager";
    width: 520px;
    height: 660px;

    // ── 版本信息（FR-9）──
    in property <string> installed-version: "检测中…";
    in property <string> installed-channel: "";
    in property <string> latest-version: "";
    in property <bool>   version-known: false;
    in property <bool>   up-to-date: false;

    // ── 选择器（FR-10 / FR-11）──
    in property <[string]> pm-options: [];
    in property <int>      pm-index: 0;
    in property <[string]> version-options: [];
    in property <int>      version-index: 0;

    // ── dsh web（FR-18）──
    in property <bool>   web-running: false;
    in property <string> web-url: "";
    in property <string> port-text: "3080";

    // ── 更新说明（FR-27）──
    // styled-text 是 Slint 的内建属性类型，对应 Rust 的 slint::StyledText。
    in property <styled-text> notes-text;
    in property <NotesStatus> notes-status: NotesStatus.loading;

    // ── 日志与忙碌态（FR-15 / FR-28）──
    in property <[string]> log-lines: [];
    in property <bool>     busy: false;
    in property <string>   busy-label: "";
    in property <string>   status-text: "";

    // 纯窗口本地 UI 状态，不参与 Rust 侧共享状态
    in-out property <bool> about-visible: false;

    callback install-clicked();
    callback pm-changed(int);
    callback version-changed(int);
    callback refresh-clicked();
    callback start-web();
    callback stop-web();
    callback open-web();
    callback port-changed(string);
    callback hide-to-tray();
    callback about-clicked();
    callback link-clicked(string);

    VerticalLayout {
        padding: 12px;
        spacing: 10px;

        // ═══ 版本信息 ═══
        Rectangle {
            background: #f5f5f5;
            border-radius: 6px;
            height: 76px;
            HorizontalLayout {
                padding: 10px;
                spacing: 8px;
                VerticalLayout {
                    spacing: 4px;
                    HorizontalLayout {
                        spacing: 8px;
                        Text { text: "已安装"; width: 56px; color: #666; }
                        Text { text: root.installed-version; font-weight: 700; }
                        Text {
                            text: root.installed-channel;
                            color: #0a7; font-size: 11px;
                            vertical-alignment: center;
                            visible: root.installed-channel != "";
                        }
                    }
                    HorizontalLayout {
                        spacing: 8px;
                        Text { text: "最新"; width: 56px; color: #666; }
                        Text { text: root.latest-version; visible: root.version-known; }
                        Text {
                            text: "✓ 已是最新";
                            color: #0a7;
                            visible: root.version-known && root.up-to-date;
                        }
                        Text {
                            text: "↓ 可更新";
                            color: #c60;
                            visible: root.version-known && !root.up-to-date;
                        }
                    }
                }
            }
        }

        // ═══ 选择器 + 安装 ═══
        HorizontalLayout {
            spacing: 8px;
            Text { text: "包管理器"; width: 64px; vertical-alignment: center; color: #666; }
            ComboBox {
                model: root.pm-options;
                current-index: root.pm-index;
                enabled: !root.busy;
                selected(v) => { root.pm-changed(self.current-index); }
            }
        }
        HorizontalLayout {
            spacing: 8px;
            Text { text: "目标版本"; width: 64px; vertical-alignment: center; color: #666; }
            ComboBox {
                model: root.version-options;
                current-index: root.version-index;
                enabled: !root.busy;
                selected(v) => { root.version-changed(self.current-index); }
            }
            Button {
                text: "安装";
                enabled: !root.busy;
                clicked => { root.install-clicked(); }
            }
            Button {
                text: "刷新";
                enabled: !root.busy;
                clicked => { root.refresh-clicked(); }
            }
        }

        // ═══ dsh web ═══
        Rectangle {
            background: #fafafa;
            border-radius: 6px;
            border-width: 1px;
            border-color: #e0e0e0;
            VerticalLayout {
                padding: 10px;
                spacing: 8px;
                HorizontalLayout {
                    spacing: 8px;
                    Rectangle {
                        width: 8px; height: 8px; border-radius: 4px;
                        vertical-alignment: center;
                        background: root.web-running ? #0c0 : #bbb;
                    }
                    Text {
                        text: root.web-running ? "运行中" : "已停止";
                        font-weight: 600;
                    }
                    Text {
                        text: root.web-url;
                        color: #06c;
                        visible: root.web-running;
                        vertical-alignment: center;
                    }
                }
                HorizontalLayout {
                    spacing: 8px;
                    Text { text: "端口"; vertical-alignment: center; color: #666; }
                    LineEdit {
                        text: root.port-text;
                        width: 80px;
                        enabled: !root.busy;
                        edited(t) => { root.port-changed(t); }
                    }
                    Button {
                        text: "启动";
                        enabled: !root.web-running && !root.busy;
                        clicked => { root.start-web(); }
                    }
                    Button {
                        text: "停止";
                        enabled: root.web-running && !root.busy;
                        clicked => { root.stop-web(); }
                    }
                    Button {
                        text: "打开网页";
                        enabled: root.web-running;
                        clicked => { root.open-web(); }
                    }
                }
            }
        }

        // ═══ 更新说明 ═══
        Text {
            text: "更新说明" + (
                root.notes-status == NotesStatus.missing ? " · 该版本无更新说明" :
                root.notes-status == NotesStatus.loading ? " · 加载中…" :
                root.notes-status == NotesStatus.failed  ? " · 加载失败" : ""
            );
            color: #666;
        }
        Rectangle {
            background: white;
            border-radius: 6px;
            border-width: 1px;
            border-color: #e0e0e0;
            min-height: 130px;
            ScrollView {
                StyledText {
                    text: root.notes-text;
                    font-size: 12px;
                    link-clicked(link) => { root.link-clicked(link); }
                }
            }
        }

        // ═══ 日志（FR-28）═══
        Text { text: "日志"; color: #666; }
        Rectangle {
            background: #1e1e1e;
            border-radius: 6px;
            min-height: 120px;
            ScrollView {
                ListView {
                    for line in root.log-lines : Text {
                        text: line;
                        color: #d4d4d4;
                        font-family: "Consolas";
                        font-size: 11px;
                        wrap: no-wrap;
                    }
                }
            }
        }

        // ═══ 底部状态栏 ═══
        HorizontalLayout {
            spacing: 8px;
            Text {
                text: root.busy ? root.busy-label : root.status-text;
                color: root.busy ? #c60 : #666;
                vertical-alignment: center;
                font-size: 12px;
            }
            Rectangle { horizontal-stretch: 1; }
            Button { text: "关于"; clicked => { root.about-visible = true; root.about-clicked(); } }
            Button { text: "隐藏到托盘"; clicked => { root.hide-to-tray(); } }
        }
    }

    // ═══ 关于对话框（GC-10：AboutSlint 是许可的强制署名义务）═══
    if root.about-visible : Rectangle {
        background: #00000066;
        VerticalLayout {
            alignment: center;
            Rectangle {
                background: white;
                border-radius: 8px;
                width: 360px;
                VerticalLayout {
                    padding: 16px;
                    spacing: 10px;
                    Text { text: "DSH Manager"; font-size: 18px; font-weight: 700; }
                    Text { text: "管理 DeepSeek Harness 的版本与 Web 服务"; font-size: 12px; color: #666; }
                    AboutSlint { }
                    Button { text: "关闭"; clicked => { root.about-visible = false; } }
                }
            }
        }
    }
}
```

- [ ] **Step 2: 编译**

Run: `cargo build`
Expected: 成功（增量约 5–9 秒）

> **若报 `styled-text` 相关错误**：把 `in property <styled-text> notes-text;` 改为
> `in property <styled-text> notes-text: @markdown("");`
>
> **若报 `AboutSlint` 未找到**：确认第 3 行的 import 列表含 `AboutSlint`（GC-13）。

- [ ] **Step 3: 运行并肉眼检查布局**

临时把 `main` 写成只显示窗口：

```rust
fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    ui.run()
}
```

Run: `cargo run`
Expected: 界面分区清晰，中文字体显示正常（**不是方框** —— GC-11 依赖系统字体）。此时数据区为空/占位，属正常。

- [ ] **Step 4: 提交**

```bash
git add ui/app.slint src/main.rs
git commit -m "feat(ui): MainWindow 完整界面

属性与回调契约对应 SRS §5.1.1 的界面设计。下拉列表用格式化
字符串而非自定义 delegate —— ComboBox 接受 [string]，把通道与
'当前' 标记拼进文案即可满足 FR-10 的要求。

关于对话框含 AboutSlint（GC-10，Slint 免版税许可的强制署名义务）。
中文依赖系统字体自动获取，未嵌入任何字体文件（GC-11）。"
```

---

### Task 16: `ui/app.slint` — AppTray 托盘组件

**Files:**
- Modify: `ui/app.slint`（追加组件）

**Interfaces:**
- Consumes: 无
- Produces: `AppTray` 组件（顶层 `inherits SystemTrayIcon`）

- [ ] **Step 1: 准备托盘图标资源**

托盘图标**必须是非空图片** —— Slint 在 `icon` 为空时不创建图标（ARCHITECTURE §9.1 R-3）。

创建 `ui/tray-icon.png`：一个 32×32 的 PNG。可用任意方式生成，例如：

```bash
# 用 Python 生成一个简单的纯色圆角方块占位图（无需 Pillow，手写 PNG）
python -c "
import struct, zlib
W=H=32
raw=b''
for y in range(H):
    raw+=b'\x00'
    for x in range(W):
        inside = 6<=x<26 and 6<=y<26
        raw += bytes([0x2b,0x7a,0xd5,0xff]) if inside else bytes([0,0,0,0])
def chunk(t,d):
    c=t+d
    return struct.pack('>I',len(d))+c+struct.pack('>I',zlib.crc32(c)&0xffffffff)
png=b'\x89PNG\r\n\x1a\n'
png+=chunk(b'IHDR',struct.pack('>IIBBBBB',W,H,8,6,0,0,0))
png+=chunk(b'IDAT',zlib.compress(raw))
png+=chunk(b'IEND',b'')
open('ui/tray-icon.png','wb').write(png)
print('已生成 ui/tray-icon.png')
"
```

- [ ] **Step 2: 追加 `AppTray` 组件到 `ui/app.slint`**

```slint
// 托盘组件是【顶层】组件：inherits SystemTrayIcon，不能放进 Window 里，
// 且必须恰好包含一个 Menu 子元素。
//
// ⚠ 它与 MainWindow 各自持有 global 的独立副本，二者不共享任何状态 ——
// 所有同步必须由 Rust 侧推送（SRS FR-25）。
export component AppTray inherits SystemTrayIcon {
    in property <bool> web-running: false;
    in property <bool> busy: false;

    icon: @image-url("tray-icon.png");
    tooltip: "DSH Manager";

    callback show-window();
    callback open-web();
    callback start-web();
    callback stop-web();
    callback quit-app();

    Menu {
        MenuItem {
            title: "显示主窗口";
            activated => { show-window(); }
        }
        MenuItem {
            title: "打开 DSH 网页";
            enabled: root.web-running && !root.busy;
            activated => { open-web(); }
        }
        MenuSeparator { }
        MenuItem {
            title: "启动 dsh web";
            enabled: !root.web-running && !root.busy;
            activated => { start-web(); }
        }
        MenuItem {
            title: "停止 dsh web";
            enabled: root.web-running && !root.busy;
            activated => { stop-web(); }
        }
        MenuSeparator { }
        MenuItem {
            title: "退出";
            activated => { quit-app(); }
        }
    }
}
```

- [ ] **Step 3: 编译并验证托盘出现**

临时 `main`：

```rust
fn main() -> Result<(), slint::PlatformError> {
    let win = MainWindow::new()?;
    let tray = AppTray::new()?;
    win.show()?;
    slint::run_event_loop_until_quit()?;   // FR-23：不是 run_event_loop()
    drop(tray);
    Ok(())
}
```

Run: `cargo run`
Expected:
1. 窗口出现，**系统托盘中出现图标**（若图标未出现在可见区域，点任务栏的 `^` 展开隐藏图标）
2. 鼠标悬停显示 "DSH Manager"
3. **右键**托盘图标 → 弹出菜单，含 5 个条目与 2 条分隔线
4. **左键**单击托盘图标 → 触发 `clicked()`（本步骤无需可见反应，Task 19 接上）
5. 关闭窗口后进程仍在运行（事件循环未退出）—— 这就是 FR-23 的基础

> **若托盘图标不出现**：检查 `ui/tray-icon.png` 是否真的非空且能解码。Slint 在 `icon` 为空时**静默不创建**图标，不报错。

- [ ] **Step 4: 提交**

```bash
git add ui/app.slint ui/tray-icon.png src/main.rs
git commit -m "feat(ui): AppTray 托盘组件（FR-24）

顶层组件 inherits SystemTrayIcon，恰好一个 Menu 子元素。
菜单项的 enabled 绑定 web-running 与 busy —— Slint 会在绑定
变化时自动重建平台菜单，无需手动刷新。

托盘图标为非空 PNG：Slint 在 icon 为空时静默不创建图标（不报错），
这是 ARCHITECTURE §9.1 R-3 记录的风险点。

平台行为（Slint 既定，不可配）：左键触发 clicked()，右键弹菜单。"
```

---

### Task 17: `src/main.rs` — 入口、AppState、Timer 排空与状态推送

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: `model::*`、`config`、`pm`、`dsh`、`txn`
- Produces:
  - `struct AppState`
  - `drain(rx: &Receiver<UiMsg>, state: &Rc<RefCell<AppState>>) -> bool`
  - `project(state: &AppState, win: &MainWindow, tray: &AppTray)`
  - `push_log(state: &AppState, line: impl Into<String>)`

- [ ] **Step 1: 写 `src/main.rs` 的状态与推送层**

```rust
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod dsh;
mod model;
mod pm;
mod txn;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};

use model::*;

slint::include_modules!();

/// GC-12 / NFR-11：日志缓冲上限。超出时丢弃最旧行 ——
/// 排查问题时最新的输出远比最旧的输出有价值。
const LOG_CAP: usize = 2000;

/// UI 线程【独占】。不用 Arc<Mutex<..>>：worker 不持有任何共享状态，
/// 它需要的一切由 UI 打包进 Job 载荷。这把"并发正确性"问题转化成了
/// "消息传递正确性"问题。
struct AppState {
    env: PmEnv,
    catalog: Option<Catalog>,
    notes: NotesState,
    web: WebState,
    busy: bool,
    busy_label: String,
    status: String,
    preferred_port: u16,
    /// 由本程序启动的 dsh web 的 pid。停止时优先用它，
    /// 避免 find_listener_pid 拿到外部进程。
    web_pid: Option<u32>,
    /// 用户在下拉里选中的目标 PM 与版本。
    /// **必须存在状态里** —— 回调闭包拿不到 MainWindow（它已被 move 进
    /// 其它闭包），而"全局最新"和"owner PM"都【不是】用户的选择。
    selected_pm: Option<Pm>,
    selected_version: Option<Version>,
    log: Rc<VecModel<slint::SharedString>>,
}

#[derive(Default)]
struct NotesState {
    version: Option<Version>,
    status: Option<NotesStatus>,
    /// 已预处理的 markdown 原文（尚未转 StyledText）
    body: String,
}

impl AppState {
    fn new(preferred_port: u16) -> Self {
        Self {
            env: PmEnv::default(),
            catalog: None,
            notes: NotesState::default(),
            web: WebState::Stopped,
            busy: false,
            busy_label: String::new(),
            status: String::new(),
            preferred_port,
            web_pid: None,
            selected_pm: None,
            selected_version: None,
            log: Rc::new(VecModel::default()),
        }
    }

    /// 下拉的选中下标。未手动选过时回落到 owner PM。
    fn selected_pm_index(&self) -> i32 {
        self.selected_pm
            .or(self.env.owner)
            .and_then(|pm| self.env.available.iter().position(|i| i.kind == pm))
            .map(|i| i as i32)
            .unwrap_or(0)
    }

    /// 版本下拉的选中下标。
    fn selected_version_index(&self) -> i32 {
        match (&self.selected_version, &self.catalog) {
            (Some(v), Some(c)) => c
                .versions
                .iter()
                .position(|x| x == v)
                .map(|i| i as i32)
                .unwrap_or(0),
            _ => 0,
        }
    }

    fn channel(&self) -> Option<Channel> {
        self.env.installed.as_ref().map(pm::channel_of)
    }

    /// GC-14：只用通道内最新，**绝不使用 npm 的 latest tag**。
    fn newest_in_channel(&self) -> Option<Version> {
        let catalog = self.catalog.as_ref()?;
        let ch = self.channel()?;
        pm::latest_in(&catalog.versions, ch).cloned()
    }

    fn is_up_to_date(&self) -> bool {
        match (self.env.installed.as_ref(), self.newest_in_channel()) {
            (Some(cur), Some(newest)) => *cur == newest,
            _ => false,
        }
    }

    fn pm_labels(&self) -> Vec<slint::SharedString> {
        self.env
            .available
            .iter()
            .map(|info| {
                let mark = if self.env.owner == Some(info.kind) {
                    "  ·  dsh 安装于此"
                } else {
                    ""
                };
                slint::SharedString::from(format!("{}{mark}", info.kind.label()))
            })
            .collect()
    }

    fn version_labels(&self) -> Vec<slint::SharedString> {
        let Some(catalog) = self.catalog.as_ref() else {
            return vec![];
        };
        let cur = self.env.installed.clone();
        catalog
            .versions
            .iter()
            .map(|v| {
                let ch = match pm::channel_of(v) {
                    Channel::Stable => "stable",
                    Channel::Rc => "rc",
                    Channel::Alpha => "alpha",
                    Channel::Other => "other",
                };
                let here = if cur.as_ref() == Some(v) { "  ← 当前" } else { "" };
                slint::SharedString::from(format!("{v}  ({ch}){here}"))
            })
            .collect()
    }

    fn notes_status(&self) -> NotesStatus {
        self.notes.status.unwrap_or(NotesStatus::Loading)
    }
}

/// 追加一行日志，带 GC-12 的上限裁剪。
fn push_log(state: &AppState, line: impl Into<String>) {
    let model = &state.log;
    model.push(slint::SharedString::from(line.into()));
    let len = model.row_count();
    if len > LOG_CAP {
        model.remove(0, len - LOG_CAP);
    }
}

/// 唯一的状态投影点。**所有** UI 更新必须经过此函数。
///
/// FR-25：托盘实例与窗口实例不共享 global，因此必须推两份。
/// 这也是把两处赋值放在同一个函数里的原因 —— 分开写迟早会漏掉一处。
fn project(state: &AppState, win: &MainWindow, tray: &AppTray) {
    // ── 推给窗口 ──
    win.set_installed_version(
        state
            .env
            .installed
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "未检测到 dsh".into())
            .into(),
    );
    win.set_installed_channel(
        state
            .channel()
            .map(|c| match c {
                Channel::Alpha => "alpha 通道",
                Channel::Rc => "rc 通道",
                Channel::Stable => "stable 通道",
                Channel::Other => "其他通道",
            })
            .unwrap_or("")
            .into(),
    );
    win.set_version_known(state.catalog.is_some() && state.env.installed.is_some());
    win.set_latest_version(
        state
            .newest_in_channel()
            .map(|v| v.to_string())
            .unwrap_or_default()
            .into(),
    );
    win.set_up_to_date(state.is_up_to_date());

    win.set_pm_options(ModelRc::from(Rc::new(VecModel::from(state.pm_labels()))));
    // ⚠ 必须反映【用户的选择】，不能硬编码 owner 下标 —— 否则用户切到
    // pnpm 后界面会被下一帧弹回 npm。
    win.set_pm_index(state.selected_pm_index());
    win.set_version_options(ModelRc::from(Rc::new(VecModel::from(state.version_labels()))));
    win.set_version_index(state.selected_version_index());

    win.set_web_running(state.web.is_running());
    win.set_web_url(
        state
            .web
            .port()
            .map(|p| format!("http://127.0.0.1:{p}"))
            .unwrap_or_default()
            .into(),
    );
    win.set_port_text(state.preferred_port.to_string().into());

    win.set_notes_status(state.notes_status());
    win.set_notes_text(slint::StyledText::from_markdown(&state.notes.body));

    win.set_log_lines(ModelRc::from(state.log.clone()));
    win.set_busy(state.busy);
    win.set_busy_label(state.busy_label.clone().into());
    win.set_status_text(state.status.clone().into());

    // ── 推给托盘（独立实例，必须再推一次）──
    tray.set_web_running(state.web.is_running());
    tray.set_busy(state.busy);
}

/// 排空 worker 消息。返回是否发生了状态变化（决定要不要 project）。
fn drain(rx: &Receiver<UiMsg>, state: &Rc<RefCell<AppState>>) -> bool {
    let mut changed = false;
    loop {
        match rx.try_recv() {
            Ok(msg) => {
                changed = true;
                let mut s = state.borrow_mut();
                match msg {
                    UiMsg::Probed(env) => {
                        s.env = env;
                        s.status = "环境探测完成".into();
                        // 首次探测后，默认选中当前通道的最新版
                        let newest = s.newest_in_channel();
                        if s.selected_version.is_none() {
                            s.selected_version = newest;
                        }
                    }
                    UiMsg::Catalog(c) => {
                        s.catalog = Some(c);
                        let newest = s.newest_in_channel();
                        if s.selected_version.is_none() {
                            s.selected_version = newest;
                        }
                    }
                    UiMsg::Notes { version, result } => {
                        s.notes.version = Some(version);
                        match result {
                            Ok(body) => {
                                s.notes.body = dsh::preprocess_notes(&body);
                                s.notes.status = Some(NotesStatus::Ok);
                            }
                            Err(NotesError::Missing) => {
                                s.notes.body.clear();
                                s.notes.status = Some(NotesStatus::Missing);
                            }
                            Err(NotesError::Net(e)) => {
                                s.notes.body.clear();
                                s.notes.status = Some(NotesStatus::Failed);
                                push_log(&s, format!("更新说明拉取失败: {e}"));
                            }
                        }
                    }
                    UiMsg::Log(line) => push_log(&s, line),
                    UiMsg::TxProgress(step) => {
                        s.busy_label = format!("正在{}…", step.label());
                    }
                    UiMsg::TxDone(outcome) => {
                        s.busy = false;
                        s.busy_label.clear();
                        s.status = describe_outcome(&outcome);
                        for line in describe_outcome_log(&outcome) {
                            push_log(&s, line);
                        }
                    }
                    UiMsg::WebState(ws) => {
                        s.web = ws;
                    }
                    UiMsg::WebExited { code } => {
                        push_log(&s, format!("dsh web 已退出，退出码 {code:?}"));
                        s.web = WebState::Stopped;
                        let _ = config::update(|f| f.running_port = None);
                    }
                    UiMsg::Failed { context, message } => {
                        s.busy = false;
                        s.busy_label.clear();
                        s.status = format!("{context}失败");
                        push_log(&s, format!("{context}失败: {message}"));
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                // §5.2：worker 已终止。必须明确告知，否则 UI 看起来正常
                // 但所有操作都无响应 —— 这是最难排查的故障形态。
                let mut s = state.borrow_mut();
                s.busy = false;
                s.status = "后台工作线程已停止，请重启程序".into();
                changed = true;
                break;
            }
        }
    }
    changed
}

fn describe_outcome(o: &TxOutcome) -> String {
    match o {
        TxOutcome::Committed { pm, version } => format!("已安装 {version}（{}）", pm.label()),
        TxOutcome::Rejected { reason } => describe_reject(reason),
        TxOutcome::RolledBack { restored, .. } => {
            format!("操作失败，已恢复到 {} {}", restored.pm.label(), restored.version)
        }
        TxOutcome::Degraded { .. } => "操作失败且未能完全恢复，请按日志中的命令手动处理".into(),
    }
}

fn describe_reject(r: &RejectReason) -> String {
    match r {
        RejectReason::PmUnavailable(pm) => format!("{} 不可用", pm.label()),
        RejectReason::BinNotOnPath { pm, dir } => format!(
            "{} 的全局目录不在 PATH 中：{}。请先把它加入 PATH 再试",
            pm.label(),
            dir.display()
        ),
        RejectReason::InvalidVersion(v) => format!("版本号非法：{v}"),
        RejectReason::NoOriginInstalled => "未检测到已安装的 dsh".into(),
    }
}

fn describe_outcome_log(o: &TxOutcome) -> Vec<String> {
    match o {
        TxOutcome::Degraded { failed, reason, manual } => {
            let mut v = vec![
                format!("降级：在「{}」阶段失败", failed.label()),
                format!("原因：{reason}"),
                "请手动执行以下命令：".to_string(),
            ];
            v.extend(manual.iter().cloned());
            v
        }
        TxOutcome::RolledBack { failed, detail, .. } => vec![
            format!("在「{}」阶段失败：{detail}", failed.label()),
            "已完成回滚".to_string(),
        ],
        TxOutcome::Rejected { reason } => vec![format!("已拒绝：{}", describe_reject(reason))],
        _ => vec![],
    }
}
```

- [ ] **Step 2: 写 `main` 把上面接起来（worker 与回调在 Task 18/19 补）**

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // §3.8 / FR-30：启动时读取持久化的偏好端口
    let (preferred_port, had_state) = match config::load() {
        config::Loaded::Ok(s) => (s.preferred_port.unwrap_or(3080), true),
        config::Loaded::Missing => (3080, false),
        config::Loaded::Corrupt(e) => {
            eprintln!("state.json 损坏，使用缺省端口：{e}"); // FR-32：记日志但不阻止启动
            (3080, false)
        }
        config::Loaded::NoLocation => (3080, false),
    };
    let _ = had_state;

    let win = MainWindow::new()?;
    let tray = AppTray::new()?;

    let state = Rc::new(RefCell::new(AppState::new(preferred_port)));
    push_log(&state.borrow(), "DSH Manager 启动");

    // 首帧：显示加载态，而不是误导性的"未检测到"
    // （§6.1.1：事件循环启动到首批数据到达之间有约 290ms 空窗期）
    project(&state.borrow(), &win, &tray);

    let (_job_tx, _job_rx) = mpsc::channel::<Job>(); // Task 18 接上
    let (_msg_tx, msg_rx) = mpsc::channel::<UiMsg>(); // Task 18 接上

    // 决策 3：80ms Timer 排空。实测 timer 精度 ±1.2ms，未被节流。
    // ⚠ GC-15：timer 必须存活到事件循环结束，且其捕获的 Rc 永不离开 UI 线程。
    let timer = Timer::default();
    {
        let win_w = win.as_weak();
        let tray_w = tray.as_weak();
        let state = state.clone();
        timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
            if drain(&msg_rx, &state) {
                if let (Some(w), Some(t)) = (win_w.upgrade(), tray_w.upgrade()) {
                    project(&state.borrow(), &w, &t);
                }
            }
        });
    }

    slint::run_event_loop_until_quit()?; // FR-23：不是 run_event_loop()
    drop(timer);
    Ok(())
}
```

- [ ] **Step 3: 编译并运行**

Run: `cargo build && cargo run`
Expected:
1. 窗口出现，显示"已安装 未检测到 dsh"与"端口 3080"（暂无数据源，属正常）
2. 托盘图标出现
3. 关闭窗口 → 进程不退出（事件循环仍在跑），托盘仍在
4. 控制台**不出现**新窗口

- [ ] **Step 4: 提交**

```bash
git add src/main.rs
git commit -m "feat(main): AppState、Timer 排空与状态推送

UI 线程独占 AppState，无锁 —— worker 需要的一切由 UI 打包进
Job 载荷。这把并发正确性问题转化为消息传递正确性问题。

project() 是唯一的状态投影点，同时推给窗口与托盘两个实例
（FR-25：二者不共享 global）。两处赋值放在同一函数里，分开写
迟早会漏掉一处。

首帧显示加载态而非'未检测到'：事件循环启动到首批数据到达之间
有约 290ms 空窗期（§6.1.1 实测），显示误导性结论比空白更糟。

含 §5.2 的 worker 崩溃检测：通道断开时明确告知用户，否则 UI
看起来正常但所有操作都无响应 —— 最难排查的故障形态。"
```

---

### Task 18: `src/main.rs` — worker 线程与 Job 执行

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: Task 17 的 `AppState` / `push_log`；`pm` / `dsh` / `txn` / `config`
- Produces:
  - `struct SystemBackend` + `impl txn::Backend for SystemBackend`
  - `spawn_worker(rx: Receiver<Job>, tx: Sender<UiMsg>, ui_tx: Sender<UiMsg>)`
  - `execute(job: Job, tx: &Sender<UiMsg>)`

- [ ] **Step 1: 实现 `txn::Backend` 的真实版本**

在 `src/main.rs` 中追加：

```rust
/// `txn::Backend` 的真实实现。单元测试用 `txn::FakeBackend`，
/// 生产用这个 —— 这是全项目唯一使用 trait 的地方，理由见 ARCHITECTURE §4.2.2。
struct SystemBackend;

impl txn::Backend for SystemBackend {
    fn run(&self, pm_kind: Pm, args: &[String]) -> Result<pm::CmdOut, String> {
        pm::run_cmd(pm_kind.exe(), args)
    }
    fn bin_dir(&self, pm_kind: Pm) -> Result<std::path::PathBuf, String> {
        pm::probe_pm(pm_kind)
            .map(|i| i.bin_dir)
            .ok_or_else(|| format!("{} 不可用", pm_kind.label()))
    }
    fn path_dirs(&self) -> Vec<std::path::PathBuf> {
        pm::path_dirs()
    }
    /// TR-4：直接执行指定目录的 shim，**不走 PATH**。
    fn dsh_version_at(&self, dir: &std::path::Path) -> Option<Version> {
        pm::read_dsh_version_at(dir)
    }
    fn dsh_version_on_path(&self) -> Option<(Pm, Version)> {
        pm::read_dsh_version_on_path()
    }
    fn log(&self, line: &str) {
        // 事务引擎内的日志通过 UiMsg 走，这里不直接持有 Sender，
        // 故由 execute() 在调用前后补充关键日志。
        let _ = line;
    }
}
```

> **注意**：`Backend::log` 拿不到 `Sender`。为了让事务步骤日志能进 GUI，`execute` 在调用 `txn::run` 前后自行输出关键节点。这与 ARCHITECTURE §4.2.2 的 trait 定义一致（trait 不持有 UI 依赖，纯逻辑）。

- [ ] **Step 2: 实现 worker 循环与 Job 分发**

```rust
/// 唯一的 worker 线程。**无状态** —— 它需要的一切都在 Job 载荷里。
/// 串行执行天然满足 FR-15（禁止并发事务）。
fn spawn_worker(rx: Receiver<Job>, tx: Sender<UiMsg>) {
    std::thread::spawn(move || {
        while let Ok(job) = rx.recv() {
            execute(job, &tx);
        }
    });
}

fn execute(job: Job, tx: &Sender<UiMsg>) {
    let send = |m: UiMsg| {
        let _ = tx.send(m);
    };
    match job {
        Job::Probe => match std::panic::catch_unwind(pm::probe_env) {
            Ok(env) => send(UiMsg::Probed(env)),
            Err(_) => send(UiMsg::Failed { context: "环境探测", message: "内部错误".into() }),
        },

        Job::FetchCatalog => match dsh::fetch_catalog() {
            Ok(c) => send(UiMsg::Catalog(c)),
            Err(e) => send(UiMsg::Failed { context: "版本列表拉取", message: e }),
        },

        Job::FetchNotes { version } => {
            let result = dsh::fetch_notes(&version);
            send(UiMsg::Notes { version, result });
        }

        Job::Transact { origin, target, port } => {
            send(UiMsg::Log(format!(
                "事务开始：{} {} → {} {}",
                origin.pm.label(), origin.version,
                target.pm.label(), target.version
            )));

            // TR-1 前置：dsh web 必须已停止（避开 Windows 文件锁）。
            // 这一步放在事务引擎【之外】—— 停止可能需要与用户交互，
            // 而事务引擎必须保持纯逻辑无 UI 依赖（ARCHITECTURE §4.2.5）。
            let was_running = dsh::port_in_use(port);
            if was_running {
                send(UiMsg::Log("事务前停止 dsh web".into()));
                match dsh::find_listener_pid(port) {
                    Ok(pid) if dsh::is_node(pid) => {
                        if let Err(e) = dsh::stop_by_pid(pid) {
                            send(UiMsg::Failed { context: "停止 dsh web", message: e });
                            return;
                        }
                        let _ = config::update(|f| f.running_port = None);
                        // 等端口真正释放
                        let freed = dsh::wait_port_ready(
                            port, || false, Duration::from_secs(10),
                        );
                        let _ = freed;
                    }
                    Ok(_) => {
                        send(UiMsg::Failed {
                            context: "停止 dsh web",
                            message: format!("端口 {port} 被非 node 进程占用，拒绝操作"),
                        });
                        return;
                    }
                    Err(e) => {
                        send(UiMsg::Failed { context: "定位 dsh web", message: e });
                        return;
                    }
                }
            }

            send(UiMsg::TxProgress(TxStep::S1Install));
            let outcome = txn::run(&SystemBackend, origin, target);
            send(UiMsg::TxDone(outcome));

            // TR-7：事务后重新探测（owner PM 可能已改变）
            send(UiMsg::Log("事务结束，重新探测环境".into()));
            if let Ok(env) = std::panic::catch_unwind(pm::probe_env) {
                send(UiMsg::Probed(env));
            }

            // 事务前在运行 → 重新启动
            if was_running {
                send(UiMsg::Log("事务前 dsh web 在运行，重新启动".into()));
                start_web(port, tx);
            }
        }

        Job::StartWeb { port } => start_web(port, tx),

        Job::StopWeb { pid } => {
            match dsh::stop_by_pid(pid) {
                Ok(()) => {
                    // FR-31：停止后清除运行态端口。全部停止路径都汇聚到这里。
                    let _ = config::update(|f| f.running_port = None);
                    send(UiMsg::WebState(WebState::Stopped));
                }
                Err(e) => send(UiMsg::Failed { context: "停止 dsh web", message: e }),
            }
        }

        Job::OpenUrl { url } => {
            if let Err(e) = dsh::open_url(&url) {
                send(UiMsg::Failed { context: "打开网页", message: e });
            }
        }
    }
}

/// FR-16 / FR-17。启动流程抽成函数，供 StartWeb 与事务后重启复用。
fn start_web(port: u16, tx: &Sender<UiMsg>) {
    let send = |m: UiMsg| {
        let _ = tx.send(m);
    };
    send(UiMsg::WebState(WebState::Starting { port }));

    // FR-17：启动前端口探测
    if dsh::port_in_use(port) {
        match dsh::find_listener_pid(port) {
            Ok(pid) if dsh::is_node(pid) => {
                // 是 dsh web，但不是我们启的
                send(UiMsg::Log(format!("端口 {port} 已被外部 dsh web 占用（pid {pid}）")));
                let _ = config::update(|f| f.running_port = Some(port));
                send(UiMsg::WebState(WebState::External { port }));
            }
            Ok(pid) => {
                send(UiMsg::Failed {
                    context: "启动 dsh web",
                    message: format!("端口 {port} 被非 node 进程占用（pid {pid}）"),
                });
                send(UiMsg::WebState(WebState::Failed { reason: "端口被占用".into() }));
            }
            Err(e) => {
                send(UiMsg::Failed { context: "启动 dsh web", message: e });
                send(UiMsg::WebState(WebState::Stopped));
            }
        }
        return;
    }

    match dsh::spawn_web(port, tx.clone()) {
        Ok(pid) => {
            // 就绪确认后才写运行态端口（FR-31）
            if dsh::wait_port_ready(port, || true, Duration::from_secs(20)) {
                let _ = config::update(|f| f.running_port = Some(port));
                send(UiMsg::WebState(WebState::Running { port, pid }));
                send(UiMsg::Log(format!("dsh web 已就绪：http://127.0.0.1:{port}")));
            } else {
                send(UiMsg::WebState(WebState::Failed { reason: "启动超时".into() }));
                let _ = dsh::stop_by_pid(pid);
            }
        }
        Err(e) => {
            send(UiMsg::Failed { context: "启动 dsh web", message: e });
            send(UiMsg::WebState(WebState::Failed { reason: "启动失败".into() }));
        }
    }
}
```

- [ ] **Step 3: 在 `main` 中接上 worker 与启动任务**

把 Task 17 的 `main` 中这两行替换掉：

```rust
    let (_job_tx, _job_rx) = mpsc::channel::<Job>(); // Task 18 接上
    let (_msg_tx, msg_rx) = mpsc::channel::<UiMsg>(); // Task 18 接上
```

替换为：

```rust
    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (msg_tx, msg_rx) = mpsc::channel::<UiMsg>();
    spawn_worker(job_rx, msg_tx.clone());
```

并在 `timer.start(...)` 之后、`run_event_loop_until_quit()` 之前追加：

```rust
    // 启动时的初始任务（Q-3：每次启动查询一次更新，不做后台轮询）
    let _ = job_tx.send(Job::Probe);
    let _ = job_tx.send(Job::FetchCatalog);

    // FR-31：若上次有未清除的运行态端口，探测它 —— 这正是 FR-22 的
    // 孤儿恢复机制入口。若无占用则视为过期，静默清除。
    if let config::Loaded::Ok(s) = config::load() {
        if let Some(rp) = s.running_port {
            if rp != preferred_port {
                push_log(&state.borrow(), format!("上次的运行端口为 {rp}，探测中…"));
                let tx = msg_tx.clone();
                let job_tx2 = job_tx.clone();
                std::thread::spawn(move || {
                    if dsh::port_in_use(rp) {
                        if let Ok(pid) = dsh::find_listener_pid(rp) {
                            if dsh::is_node(pid) {
                                let _ = tx.send(UiMsg::Log(
                                    format!("检测到外部 dsh web 运行在端口 {rp}（pid {pid}）"),
                                ));
                                let _ = tx.send(UiMsg::WebState(WebState::External { port: rp }));
                                return;
                            }
                        }
                    }
                    // 过期：清除并回落到偏好端口
                    let _ = config::update(|f| f.running_port = None);
                    let _ = tx.send(UiMsg::Log("上次的运行端口已空闲，状态已清除".into()));
                    let _ = job_tx2;
                });
            }
        }
    }
```

- [ ] **Step 4: 手动验证事务被正确触发（不实际执行）**

本步骤只验证**接线正确**，用真实但安全的场景：

Run: `cargo run`
Expected:
1. 约 1 秒内界面显示"已安装 0.1.6-alpha.2"、"alpha 通道"、"✓ 已是最新"
2. 包管理器下拉显示 `npm · dsh 安装于此` 与 `pnpm`
3. 版本下拉展开有 **22 项**，降序，带通道标注，当前版本带 `← 当前`
4. 日志区出现"环境探测完成"等行

> 若第 1 条显示"未检测到 dsh"：检查 `pm::read_dsh_version_on_path()`。本机实测 `where dsh` 应解析到 `C:\Users\xueyu\AppData\Roaming\npm\dsh.cmd`。

- [ ] **Step 5: 提交**

```bash
git add src/main.rs
git commit -m "feat(main): worker 线程、Job 执行与 SystemBackend

事务流程完整接线：
- TR-1 前置（停 dsh web）放在事务引擎【之外】—— 停止可能需与用户
  交互，而事务引擎必须保持纯逻辑无 UI 依赖
- TR-7 事务后重新探测 owner PM
- FR-31 运行态端口的写入/清除都发生在启停路径内（唯一写入点约束）

启动时的孤儿恢复入口：读取持久化的运行态端口并探测之 ——
这正是 FR-22 的机制，也是 FR-31 存在的唯一理由。端口空闲时
静默清除过期状态（FR-32）。"
```

---

### Task 19: `src/main.rs` — 回调接线与退出语义

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: Task 18 的 `job_tx` / `spawn_worker` / `start_web`
- Produces: 完整的 `wire_callbacks(...)` 与 `main` 收尾

- [ ] **Step 1: 实现回调接线**

```rust
/// 把 Slint 回调接到 Job 派发上。
fn wire_callbacks(
    win: &MainWindow,
    tray: &AppTray,
    job_tx: &Sender<Job>,
    state: &Rc<RefCell<AppState>>,
    win_weak: slint::Weak<MainWindow>,
    quit: Rc<dyn Fn()>,
) {
    let send = {
        let tx = job_tx.clone();
        move |j: Job| {
            let _ = tx.send(j);
        }
    };

    // ── 主窗口 ──
    {
        let send = send.clone();
        let state = state.clone();
        win.on_install_clicked(move || {
            let mut s = state.borrow_mut();
            let (Some(owner), Some(installed)) = (s.env.owner, s.env.installed.clone()) else {
                s.status = "未检测到已安装的 dsh，无法执行变更".into();
                return;
            };
            // ⚠ 目标必须来自【用户的选择】。不能取 catalog.versions.first()
            // （那是全局最新，可能是更旧的 rc），也不能取 owner PM
            // （用户可能在下拉里切到了别的 PM）。
            let Some(target_version) = s.selected_version.clone() else {
                s.status = "请先选择一个目标版本".into();
                return;
            };
            let target_pm = s.selected_pm.unwrap_or(owner);
            let port = s.preferred_port;
            s.busy = true;
            s.busy_label = "准备中…".into();
            drop(s);

            send(Job::Transact {
                origin: Origin { pm: owner, version: installed },
                target: Target { pm: target_pm, version: target_version },
                port,
            });
        });
    }

    {
        let state = state.clone();
        win.on_pm_changed(move |idx| {
            let mut s = state.borrow_mut();
            if let Some(info) = s.env.available.get(idx as usize) {
                s.selected_pm = Some(info.kind);
            }
        });
    }

    {
        let send = send.clone();
        let state = state.clone();
        win.on_refresh_clicked(move || {
            send(Job::Probe);
            send(Job::FetchCatalog);
        });
    }

    {
        let send = send.clone();
        let state = state.clone();
        win.on_version_changed(move |idx| {
            // 选中项必须写回 AppState —— install 回调读的就是它
            let picked = {
                let mut s = state.borrow_mut();
                let picked = s
                    .catalog
                    .as_ref()
                    .and_then(|c| c.versions.get(idx as usize).cloned());
                s.selected_version = picked.clone();
                picked
            };
            if let Some(v) = picked {
                send(Job::FetchNotes { version: v });
            }
        });
    }

    {
        let state = state.clone();
        win.on_port_changed(move |text| {
            if let Ok(p) = text.trim().parse::<u16>() {
                state.borrow_mut().preferred_port = p;
                // FR-30：用户修改端口时持久化偏好。
                // 【不派发任何 Job】—— 端口变更只是改偏好，不触发动作。
                let _ = config::update(|f| f.preferred_port = Some(p));
            }
        });
    }

    {
        let send = send.clone();
        let state = state.clone();
        win.on_start_web(move || {
            let port = state.borrow().preferred_port;
            send(Job::StartWeb { port });
        });
    }

    {
        let send = send.clone();
        let state = state.clone();
        win.on_stop_web(move || {
            let s = state.borrow();
            if let Some(port) = s.web.port() {
                drop(s);
                // 停止需要 pid：自己启的从状态拿，外部的现场查
                let pid = state
                    .borrow()
                    .web_pid
                    .or_else(|| dsh::find_listener_pid(port).ok());
                if let Some(pid) = pid {
                    send(Job::StopWeb { pid });
                }
            }
        });
    }

    {
        let send = send.clone();
        let state = state.clone();
        win.on_open_web(move || {
            if let Some(port) = state.borrow().web.port() {
                send(Job::OpenUrl { url: format!("http://127.0.0.1:{port}") });
            }
        });
    }

    {
        let state = state.clone();
        win.on_hide_to_tray(move || {
            if let Some(w) = win_weak.upgrade() {
                let _ = w.hide();
            }
            push_log(&state.borrow(), "主窗口已隐藏，程序仍在托盘运行");
        });
    }

    {
        let send = send.clone();
        win.on_link_clicked(move |url| {
            // FR-27b：说明正文自带的链接透传给系统浏览器
            send(Job::OpenUrl { url: url.to_string() });
        });
    }

    // ── 托盘 ──
    {
        let win_weak = win_weak.clone();
        tray.on_show_window(move || {
            if let Some(w) = win_weak.upgrade() {
                let _ = w.show();
            }
        });
    }
    {
        let send = send.clone();
        let state = state.clone();
        tray.on_start_web(move || {
            let port = state.borrow().preferred_port;
            send(Job::StartWeb { port });
        });
    }
    {
        let send = send.clone();
        let state = state.clone();
        tray.on_stop_web(move || {
            if let Some(port) = state.borrow().web.port() {
                if let Ok(pid) = dsh::find_listener_pid(port) {
                    send(Job::StopWeb { pid });
                }
            }
        });
    }
    {
        let send = send.clone();
        let state = state.clone();
        tray.on_open_web(move || {
            if let Some(port) = state.borrow().web.port() {
                send(Job::OpenUrl { url: format!("http://127.0.0.1:{port}") });
            }
        });
    }
    {
        let q = quit.clone();
        tray.on_quit_app(move || q());
    }
}
```

> **注意**：`send` 是 `impl Fn(Job)` 的闭包，捕获 `mpsc::Sender`（`Sender: Clone`，故闭包也可 `Clone`）。**每个使用它的闭包都要先 `let send = send.clone();`** —— 直接 move 进去会让后续闭包编译失败。

- [ ] **Step 2: 在 `drain` 中让 `WebState` 同步维护 `web_pid`**

Task 17 已定义 `AppState.web_pid`。把 `drain` 里的 `UiMsg::WebState` 分支改为：

```rust
UiMsg::WebState(ws) => {
    s.web_pid = match &ws {
        WebState::Running { pid, .. } => Some(*pid),
        _ => None,
    };
    s.web = ws;
}
```

- [ ] **Step 3: 实现退出语义与 `main` 收尾（FR-21）**

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ... （Task 17 / 18 的内容）

    // FR-23：关闭窗口 → 隐藏，不退出。
    // ⚠ 两个已实测确认的易错点（ARCHITECTURE §2.4.2）：
    //   1. on_close_requested 挂在 slint::Window 上（win.window()），
    //      不在生成的组件上（win.on_close_requested 不存在）
    //   2. 必须返回 HideWindow 才是"接受关闭并隐藏"；
    //      返回 KeepWindowShown 会【取消】关闭 —— 表现是点 X 毫无反应
    {
        let state = state.clone();
        win.window().on_close_requested(move || {
            push_log(&state.borrow(), "主窗口已隐藏，程序仍在托盘运行");
            slint::CloseRequestResponse::HideWindow
        });
    }

    // FR-21：退出必须先停掉本程序启动的 dsh web，不留孤儿 node 进程。
    let quit: Rc<dyn Fn()> = {
        let state = state.clone();
        let job_tx = job_tx.clone();
        Rc::new(move || {
            let pid = state.borrow().web_pid;
            if let Some(pid) = pid {
                let _ = dsh::stop_by_pid(pid);
                let _ = config::update(|f| f.running_port = None);
            }
            let _ = job_tx; // 不再接受新任务
            slint::quit_event_loop().ok();
        })
    };

    // 托盘"退出"也走同一条路径
    wire_callbacks(&win, &tray, &job_tx, &state, win.as_weak(), quit);

    slint::run_event_loop_until_quit()?;
    drop(timer);
    Ok(())
}
```

- [ ] **Step 4: 编译并完整手动验证**

Run: `cargo build && cargo run`

逐条对照 SRS §8.2 验证清单：

| 编号 | 操作 | 预期 |
|---|---|---|
| V-1 | 启动 | 窗口出现，**无任何控制台窗口**；已安装版本显示 `0.1.6-alpha.2` |
| V-2 | 看 PM 下拉 | 列出 npm、pnpm；**不出现** bun、yarn |
| V-3 | 看 PM 下拉标注 | npm 项带 `· dsh 安装于此` |
| V-4 | 展开版本下拉 | 22 项，降序，标注通道 |
| V-5 | 看"已是最新" | 显示 `✓ 已是最新`；**绝不显示要更新到 0.1.5-rc.2** |
| V-6 | 选 `0.1.6-alpha.2` | 显示中英双语说明；选 `0.0.1-rc.1` 显示"该版本无更新说明" |
| V-7 | 点"启动" | 浏览器自动打开；`netstat -ano \| findstr :3080` 有 LISTENING；**无黑窗口** |
| V-8 | 关闭主窗口 | 窗口消失，托盘图标仍在，3080 仍监听 |
| V-9 | 托盘左键/右键 | 左键触发；右键弹菜单 |
| V-10 | 托盘菜单状态 | 运行中时"启动"置灰、"停止"可用，反之亦然 |
| V-11 | 托盘"退出" | 程序退出且 **3080 不再监听**（无孤儿） |
| V-23 | 端口改为 8080 → 重启程序 | 端口字段仍为 **8080** |
| V-24 | 启动后查看 `%APPDATA%\dsh-manager\state.json` | 含 `running_port`；停止后该字段清除 |
| V-26 | 把 state.json 改成非法 JSON → 启动 | **正常启动**（用 3080），日志有记录，**无错误弹窗** |

- [ ] **Step 5: 提交**

```bash
git add src/main.rs
git commit -m "feat(main): 回调接线与退出语义（FR-21/FR-23）

close-requested 的正确用法（实测确认）：
- API 在 slint::Window 上（win.window().on_close_requested）
- 必须返回 HideWindow；返回 KeepWindowShown 会取消关闭，
  表现为点 X 毫无反应且不报错

退出语义（FR-21）：先停掉本程序启动的 dsh web 再退出，不留孤儿
node 进程（实测该进程占约 289MB）。停止用 AppState.web_pid 而非
find_listener_pid，避免误停外部进程。"
```

---

### Task 20: 端到端验证与收尾

**Files:**
- Create: `docs/VERIFICATION.md`
- Modify: 无代码改动

**Interfaces:**
- Consumes: 全部前置任务
- Produces: 验证记录

- [ ] **Step 1: 验证高风险路径 V-25（孤儿恢复）**

这是 **FR-31 存在的唯一理由**，必须实测：

```bash
# 1. 在【非默认端口】启动 dsh web
#    GUI 里把端口改成 8080，点启动，确认 8080 在监听
netstat -ano | findstr :8080

# 2. 用任务管理器强制结束 dsh-manager.exe（模拟崩溃，留下孤儿）
taskkill /F /IM dsh-manager.exe

# 3. 确认孤儿仍在
netstat -ano | findstr :8080    # 应仍有 LISTENING

# 4. 重新启动管理器
cargo run
```

Expected:
- 界面显示**"检测到外部 dsh web 运行在端口 8080"**，状态为"运行中"
- 点"停止"能真正终止该孤儿进程
- `netstat -ano | findstr :8080` 变为无 LISTENING

**若第 4 步显示"已停止"**：FR-31 未生效，孤儿将静默失联。回到 Task 18 检查 `running_port` 是否真的在端口就绪后被写入。

- [ ] **Step 2: 验证迁移事务 V-15 / V-16**

```bash
# 记录当前状态
where.exe dsh
npm ls -g --depth=0 | findstr deepseek
pnpm ls -g --depth=0 | findstr deepseek   # 应为空
```

在 GUI 中把包管理器切到 **pnpm**，点安装，观察日志。

Expected:
- 日志中出现 `pnpm.cmd add -g @deepseek-ai/dsh@...`
- 随后出现 `npm.cmd uninstall -g @deepseek-ai/dsh`（**顺序必须是先装后卸** —— TR-6）
- 完成后重新探测，owner 变为 **pnpm**
- `where.exe dsh` 指向 `C:\Users\xueyu\AppData\Local\pnpm\bin\dsh.cmd`

**验证完请迁回 npm**（切换回 npm 再点安装），以免影响后续工作。

- [ ] **Step 3: 移除临时 lint 抑制并确认零警告**

Task 2 在 `src/main.rs` 加了一行临时的 `#![allow(dead_code)]`，用于抑制"类型已定义但尚未被消费"的警告。到本任务时全部模块都已接线，那行必须删除：

1. 删除 `src/main.rs` 中的 `#![allow(dead_code)]` 及其上方 4 行说明注释
2. 运行 `cargo build`，**确认零警告**
3. 运行 `cargo test`，**确认零警告**

**若出现任何 `dead_code` 警告**：那是真实死代码，**必须删除对应项，不得恢复抑制**。逐个判断：
- 从来没有消费者的项 → 删掉
- 只是尚未接线的项 → 说明 Task 17~19 有遗漏，回到对应任务补上

> 这一步的存在意义：那行 allow 会掩盖真实死代码。没有这一步，它会永久留在代码里。

- [ ] **Step 4: 写验证记录 `docs/VERIFICATION.md`**

```markdown
# DSH Manager 验证记录

对应 SRS v1.2 §8.2 / §8.3 的验证清单。日期：<填写>

## 环境

| 项 | 值 |
|---|---|
| dsh 版本 | |
| owner PM | |
| Node / npm / pnpm | |

## 功能验证（§8.2）

| 编号 | 结果 | 备注 |
|---|---|---|
| V-1 ~ V-11 | | |
| V-12 端口冲突 | | |
| V-13 外部进程停止 | | |
| V-14 换版本事务 | | |
| V-15 迁移事务 | | |
| V-16 前置检查 | | |
| V-17 失败补偿 | | |
| V-18 事务期间 UI | | |
| V-23 偏好端口持久化 | | |
| V-24 运行态端口记录 | | |
| V-25 孤儿恢复 | | |
| V-26 持久化失败降级 | | |

## 关键约束验证（§8.3）

| 编号 | 结果 | 备注 |
|---|---|---|
| V-19 TR-4 验证不走 PATH | | |
| V-20 TR-6 先装后卸 | | |
| V-21 TR-5 补偿前探测 | | |
| V-22 FR-25 双实例状态同步 | | |

## 未通过项与处理

| 编号 | 现象 | 处理 |
|---|---|---|
| | | |
```

- [ ] **Step 5: 逐条执行 §8.2 / §8.3 并填写记录**

Run: 按 SRS §8.2 与 §8.3 的表格逐条执行，把实际结果填入 `docs/VERIFICATION.md`。

**任何一条不通过都必须记录，不得留空或跳过。**

- [ ] **Step 6: 提交**

```bash
git add docs/VERIFICATION.md
git commit -m "docs: 端到端验证记录

对照 SRS v1.2 §8.2 / §8.3 逐条执行并记录结果。
其中 V-25（孤儿恢复）是 FR-31 存在的唯一理由，为必验项。"
```

---

## 自检

### 1. Spec 覆盖检查

| SRS 需求 | 对应任务 |
|---|---|
| FR-1 ~ FR-2 PM 扫描与 bin 目录 | Task 7 |
| FR-3 owner 判定 | Task 6 |
| FR-4 版本读取 | Task 7 |
| FR-5 探测缓存与刷新 | Task 17 / 19（刷新按钮） |
| FR-6 版本列表拉取 | Task 11 |
| FR-7 通道判定 | Task 5 |
| FR-8 "最新版本"定义 | Task 5 / 11（GC-14 回归测试） |
| FR-9 版本展示 | Task 15 / 17 |
| FR-10 版本列表展开 | Task 17（`version_labels`） |
| FR-11 目标选择 | Task 15 / 17 |
| FR-12 操作触发 | Task 19 |
| FR-13 迁移等价性 | Task 9（同 PM 跳过 S3） |
| FR-14 PM 专属命令 | Task 2（`impl Pm`） |
| FR-15 操作期间状态 | Task 17（`busy`） |
| FR-16 启动 dsh web | Task 14 / 18 |
| FR-17 端口检测 | Task 13 / 18 |
| FR-18 运行状态展示 | Task 15 / 17 |
| FR-19 停止 dsh web | Task 13 / 14 |
| FR-20 打开网页 | Task 14 |
| FR-21 退出语义 | Task 19 |
| FR-22 孤儿进程 | Task 18 / Task 20 V-25 |
| FR-23 窗口隐藏与托盘 | Task 16 / 19 |
| FR-24 托盘菜单 | Task 16 |
| FR-25 双实例状态同步 | Task 17（`project`） |
| FR-26 更新说明拉取 | Task 12 |
| FR-27 渲染与降级 | Task 12 / 15 |
| FR-27b 说明中的链接 | Task 15（`link-clicked`）/ 19 |
| FR-28 日志面板 | Task 15 / 17（`LOG_CAP`） |
| FR-29 关于对话框 | Task 15（`AboutSlint`） |
| FR-30 偏好端口持久化 | Task 3 / 19 |
| FR-31 运行态端口持久化 | Task 4 / 18 |
| FR-32 持久化失败降级 | Task 3 / 4（`Loaded` 枚举） |
| TR-1 ~ TR-3 前置检查 | Task 8 |
| TR-4 验证不走 PATH | Task 9（V-19 测试） |
| TR-5 补偿前探测 | Task 10（V-21 测试） |
| TR-6 先装后卸 | Task 9（V-20 测试） |
| TR-7 事务后重新探测 | Task 18 |
| TR-11 补偿先探测再动作 | Task 10（测试） |
| NFR-5 参数数组不拼串 | GC-9 / Task 7 |
| NFR-6 版本字符集校验 | Task 8（`is_safe_version`） |
| NFR-7 终止前校验进程名 | Task 13（`is_node`） |
| NFR-11 日志上限 | Task 17（`LOG_CAP`） |
| CON-8 AboutSlint 署名 | Task 15 |
| CON-9 不嵌入字体 | GC-11 |

**无遗漏。**

### 2. 占位符扫描

计划中**不含** "TBD" / "TODO" / "实现细节略" / "参照 Task N" 等占位。

**一处需实现者注意的显式约束**（非占位符，是刻意的提醒）：
- Task 9 Step 3 说明了简版 `compensate` 是临时脚手架，**必须与 Task 10 合并提交** —— 不得留下脚手架版本。

### 3. 类型一致性检查

| 名称 | 定义处 | 使用处 | 一致 |
|---|---|---|---|
| `Pm::exe()` / `label()` / `install_args()` / `uninstall_args()` / `bin_dir_args()` / `ALL` | Task 2 | Task 5 ~ 19 | ✓ |
| `SHIM_NAMES` | Task 2 | Task 6（`shim_in`、`find_dsh_on_path`） | ✓ |
| `WebState`（**无 `owned` 字段**） | Task 2 | Task 17 的 `is_running()` / `port()` | ✓ |
| `WebState::Running { port, pid }` | Task 2 | Task 18 写入、Task 17 `web_pid` 提取 | ✓ |
| `AppState.web_pid` | Task 17（定义）/ Task 19 Step 2（维护） | Task 19 退出与停止 | ✓ |
| `AppState.selected_pm` / `selected_version` | Task 17（定义 + `selected_pm_index` / `selected_version_index`） | Task 17 `project`、Task 19 `on_pm_changed` / `on_version_changed` / `on_install_clicked` | ✓ |
| `txn::Backend` 六个方法 | Task 8 | Task 19 `SystemBackend` 实现全部六个 | ✓ |
| `txn::install` / `uninstall` / `run` / `precheck` / `manual_commands` | Task 8 ~ 10 | Task 18 `txn::run` | ✓ |
| `config::{Loaded, StateFile, load, update, save_to, load_from, update_at}` | Task 3 / 4 | Task 17 / 18 / 19 | ✓ |
| `pm::{probe_env, run_cmd, path_dirs, probe_pm, read_dsh_version_at, read_dsh_version_on_path, shim_in, same_dir, channel_of, latest_in, sorted_desc, CmdOut, CREATE_NO_WINDOW}` | Task 5 ~ 7 | Task 8 ~ 19 | ✓ |
| `dsh::{parse_catalog, fetch_catalog, preprocess_notes, parse_release_body, fetch_notes, port_in_use, parse_netstat_pid, find_listener_pid, is_node, spawn_web, wait_port_ready, stop_by_pid, open_url, agent}` | Task 11 ~ 14 | Task 17 ~ 19 | ✓ |
| `UiMsg` / `Job` 全部变体 | Task 2 | Task 17 / 18 | ✓ |
| Slint 属性名（`web-running` ↔ `set_web_running`） | Task 15 / 16 | Task 17 `project` | ✓ |
| Slint 回调名（`install-clicked` ↔ `on_install_clicked`） | Task 15 / 16 | Task 19 `wire_callbacks` | ✓ |

**一处已知的类型细节**：`WebState::Running` 在 Task 2 中定义为 `{ port: u16, pid: u32 }`（**没有** `owned` 字段）。ARCHITECTURE §4.1 的草案里曾有 `owned: bool`，但 Task 14 的设计决策 2 把"自己启动的"与"外部启动的"合并成了同一条停止路径，因此该字段已无用途 —— **以本计划为准**。

### 4. 已知需实现者判断的点

| 位置 | 情况 | 处理 |
|---|---|---|
| Task 15 Step 2 | `in property <styled-text> notes-text;` 无默认值可能被编译器拒绝 | 改用 `@markdown("")` |
| Task 2 Step 2 | `cfg_attr(not(test), windows_subsystem)` 若吞掉测试输出 | 迁到 `src/lib.rs` 分离测试目标，**并上报** |
| Task 16 Step 1 | 托盘图标需非空 PNG | 用给出的 Python 片段生成占位图 |
| Task 18 Step 4 | 若显示"未检测到 dsh" | 先手工执行 `where dsh` 确认 PATH 解析 |

---

## 执行顺序与依赖

```
Task 1（骨架）
  └─► Task 2（model）
        ├─► Task 3 ─► Task 4        （config）
        ├─► Task 5 ─► Task 6 ─► Task 7   （pm）
        └─► Task 8 ─► Task 9+10     （txn，9 与 10 必须合并执行）
                                        │
Task 11 ─► Task 12 ─► Task 13 ─► Task 14   （dsh，只依赖 Task 2/7）
                                        │
                          Task 15 ─► Task 16
                                        │
                    Task 17 ─► Task 18 ─► Task 19 ─► Task 20
```

**并行提示**：阶段 3（Task 11–14）与阶段 2 的 config/pm/txn 互不依赖，可并行开发。阶段 4 必须等全部前置完成。


