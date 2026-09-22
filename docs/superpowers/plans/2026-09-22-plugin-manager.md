# 第三方插件卡（插件管理）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在主窗口右栏「更新说明」之上新增插件卡：列出 web profile 已装的第三方插件（名称 / 规格 / 已装版本 / 最新版本 / 可更新），支持安装、更新（单个与全部）、卸载；所有变更经 `dsh plugin --profile web …` 转发。

**Architecture:** 新增 `src/plugin.rs`（profile 定位 / 盘点 / 规格校验 / registry 查询 / 命令执行），纯函数尽量无 I/O 以便单测。UI 状态仍走既有 Job/UiMsg + 80ms 排空 + `project()` 单点投影。**不碰事务引擎**（`src/txn.rs` 一行不动）。

**Tech Stack:** Rust 2024 + Slint 1.18；`serde_json` / `ureq` / `semver`（均已在依赖里，**零新增依赖**）。

**Spec:** `docs/superpowers/specs/2026-09-22-plugin-manager-design.md`（本计划的每一条都从它推出；执行者先读它的 §2「已验证的事实」）

## Global Constraints

- **零新增依赖**：`Cargo.toml` 不动。
- **零警告**：`cargo build` 与 `cargo test` 都必须零警告；删掉任何东西后要复查死代码（本项目门槛）。
- **不写 profile 任何文件**：只读 `package.json` 与 `node_modules/<包>/package.json`。
- **不停 `dsh web`**：插件操作与运行中的服务共用同一把 profile 写锁（spec F9），不新增任何停/启逻辑。
- **命令必须经 `dsh plugin --profile web …`**，且用 Windows shim 全名（`dsh.cmd` → 回落 `dsh.exe`，`src/model.rs:17` 的 `SHIM_NAMES`）。
- **子进程必须带 `CREATE_NO_WINDOW`**：用既有 `pm::run_cmd`（`src/pm.rs:99`），不要自己 `Command::new`。
- **UI 文案的字符串拼接一律在 Rust 侧完成**（project() 推格式化好的字符串），Slint 只渲染。
- **不可见元素照样占布局位置**：三态用 `if`，不用 `visible:`（本文件多处已记这条坑）。
- **`theme.rs::parse_palette` 的坑**：`ui/app.slint` 里凡含 `brush` 类型字样（尖括号形式）的**注释行**都会被当成令牌行并报错 —— 新增注释时不要写出那种尖括号写法。

---

## 文件结构

| 文件 | 职责 | 动作 |
|---|---|---|
| `src/plugin.rs` | profile 定位 / 盘点已装 / 规格白名单 / packument 解析 / 并行查最新版 / 执行 `dsh plugin` | **新建** |
| `src/model.rs` | `PluginRow`（纯数据 + `updatable()`）、`PluginOp`（+`args`/`describe`）、`Job`/`UiMsg` 新变体 | 修改 |
| `src/main.rs` | `mod plugin;`、`AppState.plugins`、worker 两个 Job 臂、`drain` 两个臂、`project()` 投影、6 个回调接线 | 修改 |
| `ui/app.slint` | `PluginRow` 结构体、6 个属性、6 个 callback、插件卡与 `PluginRowView`、卸载确认框 | 修改 |
| `docs/SRS.md` / `docs/ARCHITECTURE.md` / `docs/VERIFICATION.md` | 需求与设计的对应修订 + 验证清单 | 修改（Task 6） |

**任务边界说明**：Task 1（类型，编译期验收）与 Task 2（纯逻辑 + 单测，`cargo test` 验收）可以独立验收；Task 3（接线）后功能才端到端可用；Task 4（UI）后才能看见；Task 5 是探针/真机验证；Task 6 是文档。每个 Task 结束时都提交。

---

## Task 1: `src/model.rs` —— 类型与消息（**必须排在最前**）

⚠ 原计划把 `plugin.rs` 排第一，但它的测试要构造 `PluginRow` / `PluginOp` —— 那两个类型在
本任务才创建，**顺序反了 Task 1 编不过**（自审时发现）。类型先行，`plugin.rs` 才能引用。

**Files:**
- Modify: `src/model.rs`

**Interfaces:**
- Produces:
  - `pub struct PluginRow { pub name: String, pub spec: String, pub installed: Option<Version>, pub latest: Option<Version> }`（`#[derive(Clone, Debug, PartialEq)]`）
  - `impl PluginRow { pub fn updatable(&self) -> bool }`
  - `pub enum PluginOp { Install(String), Update { name: String, version: Version }, UpdateAll(Vec<(String, Version)>), Remove(String) }`（同上 derive）
  - `impl PluginOp { pub fn args(&self, profile: &str) -> Vec<String>; pub fn describe(&self) -> String }`
  - `Job::FetchPlugins`、`Job::PluginOp { op: PluginOp }`
  - `UiMsg::Plugins(Result<Vec<PluginRow>, String>)`、`UiMsg::PluginOpDone { op: PluginOp, ok: bool }`

- [ ] **Step 1: 写失败的测试**（`src/model.rs` 底部 `#[cfg(test)] mod tests`）

```rust
#[test]
fn plugin_op_args_are_table_driven() {
    let v: Version = "1.3.0".parse().unwrap();
    assert_eq!(PluginOp::Install("@s/p@1.0.0".into()).args("web"),
        ["plugin", "--profile", "web", "add", "@s/p@1.0.0"]);
    assert_eq!(PluginOp::Update { name: "@s/p".into(), version: v.clone() }.args("web"),
        ["plugin", "--profile", "web", "add", "@s/p@1.3.0"]);
    // ★ 全部更新 = 一次子进程、多规格（spec §5.2 的取舍：一荣俱荣，换一次加锁与一次 install）
    assert_eq!(PluginOp::UpdateAll(vec![("@s/p".into(), v.clone()), ("@s/q".into(), v)]).args("web"),
        ["plugin", "--profile", "web", "add", "@s/p@1.3.0", "@s/q@1.3.0"]);
    assert_eq!(PluginOp::Remove("@s/p".into()).args("web"),
        ["plugin", "--profile", "web", "remove", "@s/p"]);
}

#[test]
fn updatable_requires_both_versions_and_a_strictly_newer_latest() {
    let r = |i: Option<&str>, l: Option<&str>| PluginRow {
        name: "@s/p".into(), spec: "1.0.0".into(),
        installed: i.map(|s| s.parse().unwrap()), latest: l.map(|s| s.parse().unwrap()),
    };
    assert!(r(Some("1.0.0"), Some("1.1.0")).updatable());
    assert!(!r(Some("1.1.0"), Some("1.1.0")).updatable());
    assert!(!r(Some("1.1.0"), Some("1.0.0")).updatable(), "不得把降级当更新");
    assert!(!r(Some("1.0.0"), None).updatable(), "最新版未知 → 不算可更新");
    assert!(!r(None, Some("1.0.0")).updatable(), "未安装 → 不算可更新");
}

#[test]
fn plugin_op_describe_is_user_facing() {
    let v: Version = "1.3.0".parse().unwrap();
    assert_eq!(PluginOp::Install("@s/p@1.0.0".into()).describe(), "安装 @s/p@1.0.0");
    assert_eq!(PluginOp::Update { name: "@s/p".into(), version: v.clone() }.describe(), "更新 @s/p → 1.3.0");
    assert_eq!(PluginOp::Remove("@s/p".into()).describe(), "卸载 @s/p");
    assert_eq!(PluginOp::UpdateAll(vec![("@s/p".into(), v)]).describe(), "全部更新（1 个）");
}
```

- [ ] **Step 2: 跑测试确认失败** — Run: `cargo test model:: 2>&1 | tail -20` → Expected: 编译失败（符号不存在）。
- [ ] **Step 3: 加类型**（`PluginRow`/`PluginOp` 放在既有 `WebState` 附近；`Job`/`UiMsg` 各加两个变体，并在变体上写明它们与 `FetchNotes` 的合并逻辑无关）。
- [ ] **Step 4: 跑测试确认通过** — Run: `cargo test 2>&1 | tail -5` → Expected: 全绿（既有 116 条 + 新增 3 条）。
- [ ] **Step 5: 提交** — `feat(plugin): PluginRow / PluginOp / Job / UiMsg 类型`

---

## Task 2: `src/plugin.rs` —— 纯逻辑与盘点

**Files:**
- Create: `src/plugin.rs`
- Modify: `src/main.rs`（只加一行 `mod plugin;`，放在 `mod pm;` 之后的字母序位置）

**Interfaces:**
- Consumes: Task 1 的 `crate::model::{PluginRow, PluginOp, SHIM_NAMES}`
- Produces:
  - `pub const PROFILE: &str = "web"`
  - `pub fn profile_dir_from(dsh_home: Option<OsString>, userprofile: Option<OsString>) -> Option<PathBuf>`
  - `pub fn profile_dir() -> Option<PathBuf>`
  - `pub fn valid_spec(spec: &str) -> bool`
  - `pub fn parse_latest(body: &str) -> Option<Version>`
  - `pub fn fetch_latest(agent: &ureq::Agent, name: &str) -> Option<Version>`
  - `pub fn fill_latest(rows: &mut [PluginRow])`
  - `pub fn read_installed(dir: &Path) -> Result<Vec<PluginRow>, String>`
  - `pub fn update_all_op(rows: &[PluginRow]) -> Option<PluginOp>`
  - `pub fn run_plugin(args: &[String]) -> Result<pm::CmdOut, String>`
  - `pub fn fetch_all() -> Result<Vec<PluginRow>, String>`

- [ ] **Step 1: 写失败的测试**（`src/plugin.rs` 底部 `#[cfg(test)] mod tests`，与 `pm.rs`/`dsh.rs` 同款）

必须覆盖（**这就是验收标准**）：

```rust
#[test]
fn valid_spec_accepts_registry_specs() {
    for s in ["@hilariouhiss/dsh-gitbash", "@hilariouhiss/dsh-gitbash@1.0.1",
              "left-pad", "left-pad@1.2.3-rc.1", "@scope/pkg@latest", "a@1.0.0+build.7"] {
        assert!(valid_spec(s), "{s} 应通过");
    }
}

#[test]
fn valid_spec_rejects_non_registry_sources() {
    // ★ 信任边界：file:/link:/git+/http/路径/空白/引号 一律拒绝（spec §7）
    for s in ["file:../x", "link:./x", "git+https://h/r.git", "github:u/r",
              "https://registry.npmjs.org/x", "../x", r"C:\x", "a/b", "@scope/",
              "@scope", "", " ", "a b", "a\"b", "@scope/p@1@2", "@scope/p@^1.0.0"] {
        assert!(!valid_spec(s), "{s} 应被拒绝");
    }
}

#[test]
fn parse_latest_reads_dist_tag_latest() {
    let body = r#"{"dist-tags":{"latest":"1.3.0","beta":"2.0.0-beta.1"},"versions":{}}"#;
    assert_eq!(parse_latest(body), Some("1.3.0".parse().unwrap()));
    assert_eq!(parse_latest(r#"{"versions":{}}"#), None);      // 无 dist-tags → None，不是 Err
    assert_eq!(parse_latest("not json"), None);
    assert_eq!(parse_latest(r#"{"dist-tags":{"latest":"nope"}}"#), None); // 非法版本号
}

#[test]
fn profile_dir_prefers_dsh_home_then_userprofile() {
    let d = |s: &str| Some(OsString::from(s));
    assert_eq!(profile_dir_from(d(r"C:\h"), d(r"C:\u")), Some(PathBuf::from(r"C:\h\profiles\web")));
    assert_eq!(profile_dir_from(None, d(r"C:\u")), Some(PathBuf::from(r"C:\u\.dsh\profiles\web")));
    assert_eq!(profile_dir_from(None, None), None);
}

#[test]
fn read_installed_lists_only_dependencies_and_tolerates_missing_version() {
    // ★ spec F6 的回归钉子：node_modules 里**不在 dependencies** 的包不得出现在结果里
    let dir = tempdir();
    write(dir.join("package.json"), r#"{"dependencies":{"@s/b":"^1.0.0","@s/a":"1.2.1"}}"#);
    write(dir.join("node_modules/@s/a/package.json"), r#"{"version":"1.2.1"}"#);
    write(dir.join("node_modules/@s/transitive/package.json"), r#"{"version":"9.9.9"}"#);
    let rows = read_installed(&dir).unwrap();
    assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), vec!["@s/a", "@s/b"]); // 字典序 = pnpm 的顺序
    assert_eq!(rows[0].spec, "1.2.1");            // spec 原样保留（钉死与范围混排是现状）
    assert_eq!(rows[0].installed, Some("1.2.1".parse().unwrap()));
    assert_eq!(rows[1].installed, None);          // node_modules 里没有 → None（UI 显示"未安装"）
}

#[test]
fn read_installed_reports_unreadable_profile() {
    assert!(read_installed(Path::new(r"C:\definitely\not\here")).is_err());
}

#[test]
fn update_all_op_collects_only_updatable_rows() {
    let rows = vec![
        row("@s/old", "1.0.0", Some("1.0.0"), Some("1.1.0")),   // ← 只收这一条
        row("@s/new", "1.1.0", Some("1.1.0"), Some("1.1.0")),
        row("@s/unknown", "1.0.0", Some("1.0.0"), None),
        row("@s/missing", "1.0.0", None, Some("2.0.0")),
    ];
    assert_eq!(update_all_op(&rows),
        Some(PluginOp::UpdateAll(vec![("@s/old".into(), "1.1.0".parse().unwrap())])));
    assert_eq!(update_all_op(&rows[1..]), None);                 // 没有可更新项 → None（按钮禁用）
}
```

- [ ] **Step 2: 跑测试确认失败** — Run: `cargo test plugin:: 2>&1 | tail -20` → Expected: 编译失败（`plugin` 模块不存在）。

- [ ] **Step 3: 实现**（关键实现点，逐条都是 spec 里的约束）

```rust
// src/plugin.rs
//! 第三方插件管理：profile 盘点 + registry 查最新版 + `dsh plugin` 命令转发。
//!
//! **本项目不写 profile 的任何文件**（spec §1）：只读 `package.json` 与
//! `node_modules/<包>/package.json`；一切变更经 `dsh plugin --profile <name> …`
//! 转发，由 dsh 自己持写锁并在成功后 reconcile bundle 列表。

pub const PROFILE: &str = "web";

/// `%DSH_HOME%\profiles\web`；`DSH_HOME` 缺失时回落 `%USERPROFILE%\.dsh\profiles\web`。
/// 与 Node 的 `os.homedir()` 一致（Windows 上就是 USERPROFILE），故这两个变量覆盖了
/// dsh 自己的解析结果。两者都拿不到 → None（UI 显示"未找到 profile"，控件禁用）。
pub fn profile_dir_from(dsh_home: Option<OsString>, userprofile: Option<OsString>) -> Option<PathBuf> { … }

/// 规格白名单（**信任边界**，spec §7）。允许 `名称` 或 `名称@版本`：
///   · 名称 = `pkg` 或 `@scope/pkg`，字符集 `[A-Za-z0-9._~-]`
///   · 版本段 = 空或 `[A-Za-z0-9.+-]`（`^1.0.0` / `~1.0.0` 这类范围**刻意不允许** ——
///     界面显示的"最新版"是具体版本，装范围会让显示与实际情况分叉）
/// 这一步挡住的是 `file:` / `link:` / `git+` / `github:` / 绝对与相对路径 / 空白 / 引号：
/// 它们要么让 pnpm 去本机路径或远端 git 取代码并执行其构建脚本，要么破掉 SRS CON-4
/// （只允许 registry.npmjs.org 与 api.github.com 两个**主机**）。
/// ⚠ 切分必须用 `rfind('@')` **且下标 > 0**：scoped 包名以 `@` 开头，
/// 用 `rsplit_once('@')` 会把 `@scope/name` 切成 ("", "scope/name")。
pub fn valid_spec(spec: &str) -> bool { … }

/// `dist-tags.latest`。没有该字段 / JSON 非法 / 版本号非法一律 `None` —— 不返回 Err：
/// 单个包查不到只该让那一行显示"—"，不该牵连其他行（spec §4.3 失败路径）。
pub fn parse_latest(body: &str) -> Option<Version> { … }

/// scoped 包的 `/` 转义成 `%2F`（两种写法 registry 都接受，转义避免中间层做路径处理）。
/// ⚠ 这里用 `latest` tag **是对的**：FR-8 禁止把 latest 当"最新"是针对 dsh 自身
/// （它有 alpha/rc/stable 三通道，latest 可能更旧，SRS §2.2.5）；第三方插件没有通道概念。
pub fn fetch_latest(agent: &ureq::Agent, name: &str) -> Option<Version> {
    // GET https://registry.npmjs.org/<转义名>
    //   Accept: application/vnd.npm.install-v1+json、User-Agent: dsh::USER_AGENT
    // 照 src/dsh.rs:290 fetch_catalog 的写法：.body_mut().read_to_string()
}

/// 并行补齐 `latest`：每包一线程（`std::thread::scope`），一个 Agent clone 共用。
/// 依据：本机单次 registry 请求实测 4.1s，6 个串行 ≈ 25s（spec §4.3 / NFR-12）。
/// 线程 panic 只让那一行变 None（`join().unwrap_or(None)`），不影响其他行。
pub fn fill_latest(rows: &mut [PluginRow]) { … }

/// 已装插件 = `package.json` 的 `dependencies`（★ 不是 node_modules：
/// 那里有传递依赖，实测 `@hilariouhiss/dsh-skill-kit` 不在 dependencies 里，spec F6）。
/// **不加任何包名过滤** —— profile 的 dependencies 本来就只有第三方插件，
/// `@deepseek-ai/*` 的三个 bundle 只在 `dsh.profile.bundles` 里。
/// 每行 installed 读 `node_modules/<包>/package.json` 的 version，读不到 → None。
/// 顺序由 `serde_json::Map` 决定（默认 BTreeMap = 字典序，与 pnpm 写入的顺序一致）。
pub fn read_installed(dir: &Path) -> Result<Vec<PluginRow>, String> { … }

/// 汇总成"一次 add 多规格"的更新操作；没有可更新项 → None。
pub fn update_all_op(rows: &[PluginRow]) -> Option<PluginOp> { … }

/// 执行 `dsh plugin …`：按 `SHIM_NAMES` 顺序尝试（bun 生成 `dsh.exe`，GC-7）。
/// 全失败才 Err；退出码非零是**正常结果**（由调用方判定），与 `pm::run_cmd` 的语义一致。
/// ⚠ 必须走 `pm::run_cmd`（它带 `CREATE_NO_WINDOW`，GC-8），不要自己 `Command::new`。
pub fn run_plugin(args: &[String]) -> Result<pm::CmdOut, String> { … }

/// worker 的唯一入口：定位 profile → 盘点 → 并行补最新版。
pub fn fetch_all() -> Result<Vec<PluginRow>, String> { … }
```

- [ ] **Step 4: 跑测试确认通过** — Run: `cargo test plugin:: 2>&1 | tail -20` → Expected: 7 个测试全 PASS。
- [ ] **Step 5: 提交** — `feat(plugin): profile 盘点 / 规格白名单 / registry 查最新版 / dsh plugin 转发`

---

## Task 3: `src/main.rs` —— worker 与状态接线

**Files:**
- Modify: `src/main.rs`

**Interfaces:**
- Consumes: Task 1 的 `plugin::*`、Task 2 的类型
- Produces: `AppState.plugins: PluginsState`、`PluginsState::rows/status`、`project()` 推的三个属性

- [ ] **Step 1:** `AppState` 加字段：

```rust
/// 插件卡的盘点结果。`status` 与 `NotesState` 同款：Loading / Ok / Failed(String)
/// —— "还没查到"与"查完但没有插件"必须可区分，否则加载期会渲染成"没有插件"。
struct PluginsState { rows: Vec<PluginRow>, status: PluginsStatus }
enum PluginsStatus { Loading, Ok, Failed(String) }
```

- [ ] **Step 2:** worker 两个 Job 臂：

```rust
Job::FetchPlugins => match plugin::fetch_all() {
    Ok(rows) => send(UiMsg::Plugins(Ok(rows))),
    Err(e) => send(UiMsg::Plugins(Err(e))),
},

Job::PluginOp { op } => {
    // FR-28：命令原文进日志（与事务引擎的 `$ <exe> <args…>` 同款形状）。
    let args = op.args(plugin::PROFILE);
    send(UiMsg::Log(format!("$ dsh.cmd {}", args.join(" "))));
    let ok = match plugin::run_plugin(&args) {
        Ok(out) => {
            for line in out.stdout.lines().chain(out.stderr.lines()) {
                if !line.trim().is_empty() { send(UiMsg::Log(line.to_string())); }
            }
            if out.code == 0 { true } else {
                send(UiMsg::Failed { context: "插件操作", message: format!("{} 失败（退出码 {}），详见日志", op.describe(), out.code) });
                false
            }
        }
        Err(e) => { send(UiMsg::Failed { context: "插件操作", message: e }); false }
    };
    // ⚠ 无论成败都重新盘点：失败时用户也要看到"实际没变"的列表。
    match plugin::fetch_all() {
        Ok(rows) => send(UiMsg::Plugins(Ok(rows))),
        Err(e) => send(UiMsg::Plugins(Err(e))),
    }
    send(UiMsg::PluginOpDone { op, ok });
}
```

- [ ] **Step 3:** `drain` 两个臂：`Plugins(Ok(rows))` → 写 `rows` + `status = Ok`；`Plugins(Err(e))` → `status = Failed(e)` + 一条日志；`PluginOpDone { ok }` → `busy = false`、`status` 文案（成功/失败），并把 `op` 记进日志。
- [ ] **Step 4:** `project()` 推送（**只在这里设 Slint 属性**，除对话框可见性外）：

```rust
win.set_plugin_rows(ModelRc::from(Rc::new(VecModel::from(state.plugin_rows()))));
win.set_plugin_summary(state.plugin_summary().into());
win.set_plugins_loading(matches!(state.plugins.status, PluginsStatus::Loading));
```

`AppState::plugin_rows()` 把 `Vec<PluginRow>` 转成 Slint 结构体数组（格式化的 `installed`/`latest` 字符串，`"—"`/`"未安装"` 哨兵）；`plugin_summary()` 出 `"6 个 · 1 个可更新"` / `"检查中…"` / `"读取失败"`。两者都是纯函数 → 加表驱动单测。

- [ ] **Step 5:** 启动时派发 `Job::FetchPlugins`；`on_refresh_clicked` 一并派发（全局刷新应当刷新插件）。
- [ ] **Step 6:** Run: `cargo build 2>&1 | tail -5` → Expected: 零警告（若出现"never read"警告，删掉那个东西，不要 `#[allow]`）。
- [ ] **Step 7:** 提交：`feat(plugin): worker 臂 / 排空 / 投影 / 启动与刷新时派发`

---

## Task 4: 回调与 UI

**Files:**
- Modify: `src/main.rs`（6 个回调）
- Modify: `ui/app.slint`（结构体 / 属性 / 回调 / 卡片 / 卸载确认框）

**Interfaces:**
- Consumes: Task 3 的属性
- Produces（Slint 侧契约）:

```slint
export struct PluginRow { name: string, spec: string, installed: string, latest: string, updatable: bool, missing: bool }

in property <[PluginRow]> plugin-rows: [];
in property <string> plugin-summary: "";          // SectionHeader.trailing：汇总（"6 个 · 1 个可更新"）
in property <string> plugin-status-line: "";      // 卡底左侧：状态（"检查中…" / 失败原因 / 上一条操作结论）
in property <bool>   plugins-loading: false;
in property <length> plugin-card-height: 200px;   // Rust 按行数算（上限 6 行），理由见 Step 3 注释
in property <int>    plugin-updatable-count: 0;   // 〔全部更新〕的启用条件与文案用它
in-out property <string> plugin-input: "";                    // 安装输入框
in-out property <bool>   plugin-remove-prompt-visible: false; // 卸载确认框
in property <string>     plugin-remove-name: "";              // 框里显示的包名

callback plugin-install();          // 读 plugin-input，Rust 侧校验
callback plugin-update(int);        // 行下标
callback plugin-update-all();
callback plugin-remove(int);        // 行下标 → 弹确认框（不派发）
callback plugin-remove-confirmed(); // 确认框里的〔卸载〕→ 派发
callback plugin-refresh();
```

⚠ **两处文案各管一件事，不要合并**（spec §8.2）：`SectionHeader.trailing` 是**汇总**，
卡底左侧那行是**状态**。两者都由 `project()` 从 `AppState` 推。

- [ ] **Step 1:** Rust 侧接线。**行下标映射回 `PluginRow`，包名不从 UI 传回来**（少一条能被 UI 伪造的输入路径）：

```rust
win.on_plugin_install(move || {
    let spec = win_weak.upgrade().map(|w| w.get_plugin_input().to_string()).unwrap_or_default();
    let spec = spec.trim().to_string();
    if !plugin::valid_spec(&spec) {
        // 拒绝路径必须置 dirty（否则稳态下没有消息可排空 → 提示永远不投影）
        let mut s = state.borrow_mut(); s.status = "包规格不合法：只支持 registry 上的包，形如 @scope/name@1.2.3".into(); s.dirty = true;
        return;
    }
    dispatch_plugin_op(&state, &send, PluginOp::Install(spec));
});
// update(i) / remove-confirmed() 同款；remove(i) 只置 pending + 弹框（见 Step 2）
```

`dispatch_plugin_op()` = 一个共用小函数：读 `state.plugins.rows.get(i)` → 组 `PluginOp` → `busy = true`、`busy_label = op.describe()` → `send(Job::PluginOp { op })`。**`busy` 闸门复用既有那一套**（与 `Job::Transact` 同源），派发时置真、`PluginOpDone` 清掉。

- [ ] **Step 2:** 卸载确认框：点〔卸载〕→ 记 `plugin_remove_index: Option<usize>` 到 `AppState` + `set_plugin_remove_name(...)` + `set_plugin_remove_prompt_visible(true)`（同关闭询问框：对话框可见性允许直接设，其余属性仍只走 `project()`）；〔卸载〕确认 → `request` 派发 + 关框；取消/Esc/遮罩 → 清 index + 关框。

- [ ] **Step 3:** `ui/app.slint`：右栏最上方插入插件卡（**复用既有组件**，结构见 spec §8.2）：

```slint
Bezel {
    // ⚠ 高度由 Rust 推（`plugin-card-height`）：按行数取、上限 6 行 —— 内容自适应
    // 在 Slint 里做不到（ListView 没有内容高度可读），故在 Rust 侧算好。
    height: root.plugin-card-height;
    SectionHeader { label: "插件"; trailing: root.plugin-summary; }
    HorizontalLayout {   // 安装行
        GlassField { text <=> root.plugin-input; text-align: left;
                     placeholder-text: "@scope/name 或 @scope/name@1.2.3"; enabled: !root.busy; }
        PillButton { label: "安装"; show-glyph: false; interactive: !root.busy; clicked => { root.plugin-install(); } }
    }
    Divider { }
    ListView {           // ⚠ 不要用 ScrollView 包它（虚拟化会失效，日志区同款坑）
        vertical-stretch: 1;
        for row in root.plugin-rows : PluginRowView { row: row; index: row-index; }
    }
    HorizontalLayout {   // 汇总行
        Text { text: root.plugin-status-line; }          // Loading / 失败文案
        Rectangle { horizontal-stretch: 1; }
        PillButton { label: "全部更新"; show-glyph: false;
                     interactive: !root.busy && root.plugin-updatable-count > 0;
                     clicked => { root.plugin-update-all(); } }
        PillButton { icon-only: true; width: 38px; label: "刷新"; glyph: @image-url("icons/refresh.svg");
                     interactive: !root.plugins-loading; clicked => { root.plugin-refresh(); } }
    }
}
```

每行 `PluginRowView` 两行式（右栏可用宽度约 420px，单行塞不下）：

```
@hilariouhiss/dsh-colgrep   ^1.2.1        ← 名称（等宽，超出省略）+ 规格（ink-4，右靠）
1.2.1 → 1.3.0      〔更新〕〔卸载〕        ← 版本 + 状态 + 动作
```

三态：可更新 → 〔更新〕+〔卸载〕；已是最新 → `1.3.0  已是最新` +〔卸载〕；最新未知 → `1.2.1 → —  未知`（〔更新〕禁用）+〔卸载〕；未安装 → `未安装` + 按钮文案〔安装〕（走同一条 `Update` 路径）。

- [ ] **Step 4:** 卸载确认框（照「首次关闭询问」与「重装确认」的写法：遮罩吃点击 + 空壳 `FocusScope` 抢焦点管 Esc + `Bezel` 400px；**强调给推荐动作且放最右** —— 破坏性动作这里强调给〔取消〕）。
- [ ] **Step 5:** Run: `cargo build 2>&1 | tail -5` → Expected: 零警告。
- [ ] **Step 6:** 提交：`feat(plugin): 插件卡 UI + 6 个回调 + 卸载确认框`

---

## Task 5: 验证（自动化 + 探针）

- [ ] **Step 1:** `cargo test 2>&1 | tail -3` → 全绿，并把新增条数记进 VERIFICATION。
- [ ] **Step 2:** 假 `dsh.cmd` 冒烟（照 `src/pm.rs:498` 的手法）：在 tempdir 放一个真可执行的 `dsh.cmd`（内容回显参数与 `%ERRORLEVEL%`），断言 **① 参数逐字正确 ② 退出码 0 与非 0 分别走成功/失败 ③ 命令原文进了日志**。这条不需要 pnpm，沙箱里也能跑。
- [ ] **Step 3:** 离屏探针（`target/ui-probe/`，配方见 VERIFICATION §2.4）扩展三帧：`plugin-rows` 空 / 6 行（含 1 行可更新）/ 卸载确认框可见。记录：卡片上下沿、每行两行式的 x 聚类、右栏两张卡的高度分配（插件卡 vs 更新说明卡）、确认框 400px 居中且未被裁。
- [ ] **Step 4:** 真机 UIA 读回：启动后读无障碍树，确认 `插件`、汇总文案、6 个包名、安装输入框、〔全部更新〕〔刷新〕都在树里，且 `dsh web` 未被中断。
- [ ] **Step 5:** 把 V-P1~V-P7（spec §10.3，需用户在有 pnpm 的真实环境执行）写进 `docs/VERIFICATION.md`，并注明**本轮沙箱跑不了 pnpm**（`pnpm --version` → `The path cannot be traversed because it contains an untrusted mount point`），故端到端由用户执行。
- [ ] **Step 6:** 提交：`test(plugin): 单测 + 假 shim 冒烟 + 探针与 UIA 证据 + V-P1~V-P7 清单`

---

## Task 6: 文档

**Files:** `docs/SRS.md`（→ v1.4）、`docs/ARCHITECTURE.md`（→ v1.7）、`docs/VERIFICATION.md`

- [ ] **Step 1:** SRS：① §1.3「范围外」删去插件管理；② **CON-5 改写**为"变更一律经 `dsh plugin --profile <name>` 转发；只读 profile 的 `package.json` 与 `node_modules/<包>/package.json`"；③ §1.4 术语加"插件卡"；④ 新增 §3.9：**FR-34 列表 / FR-35 安装 / FR-36 更新（含全部更新）/ FR-37 卸载**；⑤ 同时落**前三轮的 UI 修订**：**FR-9**（版本主卡删除、"是否已是最新"移入操作卡、通道最新不再单独显示）、**FR-11**（单选择器 + PM 只读指示）、**FR-12**（目标 PM 恒为 owner）、**FR-12b**（同版本重装确认）、**FR-13**（迁移从界面撤下、引擎保留的如实记录）；⑥ §5.1.1 主窗口草图重画（两张左栏卡 + 插件卡）；⑦ §5.2 接口表加 `dsh plugin --profile web add|remove` 与 `registry.npmjs.org/<包>`；**CON-4 不改**但点明它约束的是主机；⑧ 新增 **NFR-12**（插件最新版查询必须并行且后台）；⑨ §9.3 变更记录。
- [ ] **Step 2:** ARCHITECTURE：① §1.3 模块图加 `plugin.rs`（依赖方向 `model.rs` ← `plugin.rs` → `dsh.rs`/`pm.rs`）；② §1.4 数据流加插件一条；③ §3.2/§3.3 属性契约按本轮实际形状更新（PM 三件套删除、`pm-label`、`reinstall-*`、`plugin-*`）；④ 新增 **§4.8 `src/plugin.rs`**（职责边界、纯函数清单、为何走 `dsh plugin` 而非直接 pnpm、为何 `latest` tag 在这里是对的、为何不停 `dsh web`）；⑤ §6.1 纯函数表补 `valid_spec` / `read_installed` / `parse_latest` / `profile_dir_from` / `PluginOp::args` / `plugin_card_height`；⑥ §7.1 依赖表**不变**；⑦ §9.3 变更记录（含"撤回必须先停 dsh web"这条被证伪的前提）。
- [ ] **Step 3:** Run: `cargo test 2>&1 | tail -3`（文档不改变代码，但提交前跑一次以确认树是绿的）。
- [ ] **Step 4:** 提交：`docs: SRS v1.4 + ARCHITECTURE v1.7（插件卡 + 操作卡改版 + 版本主卡删除）`

---

## Self-Review

**Spec coverage：** §1 非目标 → 无任务（刻意不做，已记在 spec）；§2 事实 → 各任务的注释里逐条引用；§3 数据流 → Task 3；§4.1/4.2 → Task 2；§4.3 → Task 2（`parse_latest`/`fetch_latest`/`fill_latest`）；§5.1 命令表 → Task 1（`PluginOp::args` 测试）、§5.2 一次多规格 → Task 1 测试、§5.3 操作后重拉 → Task 3 Step 2；§6 状态与消息 → Task 1（类型）+ Task 3（接线）；§7 校验 → Task 2 `valid_spec` + Task 4 拒绝路径；§8.1/8.2/8.3 UI → Task 4；§8.4 卸载确认 → Task 4 Step 4；§9 错误处理 → Task 3 Step 2/3；§10 测试与验证 → Task 5；§11 文档 → Task 6；§12 已知限制 → 无任务（不做）。

**Placeholder scan：** 无 TBD/TODO；每个"实现"步骤都给了签名、约束与出处；UI 结构给了可编译的骨架而非"类似上文"。

**Type consistency：** `PluginRow`（model）↔ `plugin_rows()` 的 Slint 结构体转换 ↔ `PluginRowView.row`；`PluginOp::args(&self, profile: &str)` 在 Task 1 测试、Task 2 的 `update_all_op` 与 Task 3 worker 里同名同参；`profile_dir_from` 的 `Option<OsString>` 参数在测试与实现里一致；`fetch_all` 是 worker 唯一入口，Task 2 单测只测它内部的纯函数。

**两处刻意的偏离（已与用户确认过方向）：** ① 全局〔刷新〕一并刷新插件（spec §3 只写了插件卡自己的刷新按钮，多这一条是因为"刷新"对用户是全局语义）；② 插件卡的像素高度由 Rust 计算（spec §8.1 说"像素在实现时实测调"，故 Task 5 Step 3 必须用探针量出来再定值）。
