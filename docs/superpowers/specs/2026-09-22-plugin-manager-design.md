# 第三方插件管理（插件卡）—— 设计

日期：**2026-09-22**　状态：待评审　上游：`docs/SRS.md` v1.3 的 §1.3 / CON-5（**当前明文禁止**本项目干预 profile 插件管理，本设计要修订它）

目标一句话：**在主窗口右栏「更新说明」之上新增一张插件卡，列出 web profile 已装的第三方插件（规格 / 已装版本 / 最新版本 / 是否可更新），支持安装、更新（单个与全部）、卸载；所有变更经 `dsh plugin --profile web …` 转发，本项目不写 profile 的任何文件。**

---

## 1. 非目标（明确不做）

- **不做禁用 / 启用。** 本轮只做安装 / 更新 / 卸载（用户决定）。DSH 自带插件页的条目级 `disabled` 与 `dsh.profile.bundles` 摘除能力，本项目不复制。
- **不做 profile 选择器。** 固定 `web`（本程序管的就是 `dsh web`）。
- **不做插件市场 / 搜索 / 详情页。** 安装输入是一个包规格文本框，不是目录。
- **不做自动轮询检查更新。** 只在启动、手动刷新、每次操作完成后查。
- **不写 profile 的任何文件。** `package.json` / `cordis.patch.yml` / `pnpm-workspace.yaml` 一个都不写；只**读** `package.json` 与 `node_modules/<包>/package.json`。
- **不动现有事务路径。** 不停 `dsh web`、不重启、不抽公共函数（理由见 §5）。
- **不代改 `pnpm-workspace.yaml`。** pnpm 因 `allowBuilds` / `minimumReleaseAge` 拦下时，原文进日志，由用户自己处理 —— 代改等于把"不写 profile 文件"这条自己破掉。
- **不新增依赖。** `serde_json` / `ureq` / `semver` 都已在 `Cargo.toml` 里。
- **不引入 pnpm 输出的启发式解析。** 退出码 + 原文进日志，不做"识别出 EPERM 就建议停服务"这类猜测（pnpm 的文案跨版本会变）。

---

## 2. 已验证的事实（对着 dsh 安装产物与真实 profile 核实，非推断）

这一节是设计的支点。每条都写明出处，以便复核。

| # | 事实 | 出处 / 证据 |
|---|---|---|
| **F1** | `dsh plugin --profile <name> <args…>` 把剩余参数**原样转发给 pnpm**，在 profile 目录里执行 | `@deepseek-ai/dsh/lib/bin.js:116-124`（`plugin` 子命令只做 `action` 转发）、`lib/plugin-CnNK4cws.js` |
| **F2** | 转发过程持 **profile 写锁**（锁文件就是 profile 的 `package.json`），退出码 0 后跑 bundle reconcile | `dsh-plugin-manager/lib/types/operations.js:145-156`（`withFileLock(join(dir,'package.json'), …)`）、`:74-133`（`:131-132`：`if (exitCode === 0 && options.activateNewBundles !== false) await reconcile(…)`） |
| **F3** | reconcile **只把新装的依赖追加进 `dsh.profile.bundles`**，保留既有 enablement，并清掉已不在 `dependencies` 里的项 | 同上 `:39-70`。⇒ 卸载会连带清干净、不残留悬挂项。**这正是必须走 `dsh plugin`、不能直接调 `pnpm.cmd` 的原因** |
| **F4** | profile 目录 = `%DSH_HOME%\profiles\<name>`；本机 `DSH_HOME=C:\Users\xueyu\.dsh` | 实测环境变量 + 目录实测存在 |
| **F5** | 「已装第三方插件」= profile `package.json` 的 `dependencies` 键集合（本机 6 个，与用户贴的 `dsh plugin --profile web list` 输出逐条一致） | 实测 `package.json` |
| **F6** | `node_modules` 里存在**不在** `dependencies` 的传递依赖（实测 `@hilariouhiss/dsh-skill-kit`）⇒ **列表来源不得是 node_modules** | 实测 |
| **F7** | 已装版本 = `<profile>/node_modules/<包>/package.json` 的 `version`（本机 6 个全部读到） | 实测 6/6 |
| **F8** | web profile 内**没有任何** `.node` 原生模块 | 实测 `find node_modules -name "*.node"` 为空。⚠ `pnpm-workspace.yaml` 的 `allowBuilds` 里那三个名字（node-pty / cpu-features / ssh2）是 DSH 写的**白名单模板**，不是"已安装" |
| **F9** | `dsh plugin` 与**运行中的服务共用同一把 profile 写锁**；CLI 改完后由 HMR 监视器应用（`patchReload: live`） | `dsh-plugin-manager/README.md`「Use this package」；`operations.d.ts` 的 `runPluginCommand` 注释原文 *"with the same write lock as the service"*；DSH 自带的 Web 插件页本身就跑在运行中的服务器里 |
| **F10** | 本程序已有带系统代理的 `ureq` Agent，与带 `CREATE_NO_WINDOW` 的命令执行器 | `src/dsh.rs:46`（`agent()`，FR-33）、`src/pm.rs:99`（`run_cmd`） |
| **F11** | dsh 在 Windows 上的 shim 有 `.exe` 与 `.cmd` 两种，必须按 `SHIM_NAMES` 顺序尝试 | `src/model.rs:17`；`src/dsh.rs:634-649` 同款做法 |
| **F12** | 右栏现状是**单块**更新说明卡：`vertical-stretch: 1`、`min-height: 260px` | `ui/app.slint:1626-1736` |
| **F13** | `GlassField` 的注释声称"本程序唯一的调用点是 4 位端口号" | `ui/app.slint:747-748`。本设计给它加了第二个调用点（包规格输入框）⇒ **该注释必须改** |

**F8 + F9 的合并结论**：先前"profile 里有原生模块、运行中的进程会锁住它们、所以变更前必须先停 `dsh web`"的说法**不成立** —— 该前提经实测被证伪（F8），而 DSH 的设计恰恰是让 CLI 与运行中的服务共用写锁（F9）。因此本设计**不停服务、为此不弹窗**，`Job::Transact` 的 TR-1 路径**逐字不动**。连带后果见 §5 与 §11 的变更记录。

---

## 3. 数据流与职责

```
启动 / 点「刷新」 / 每次插件操作完成后
   │
   └─ Job::FetchPlugins（worker 线程）
        ├─ ① 读 <profile>/package.json 的 dependencies          ← F5，纯文件 I/O，毫秒级
        ├─ ② 逐个读 node_modules/<包>/package.json 的 version    ← F7
        └─ ③ 并行查 registry 的 dist-tags.latest（thread::scope） ← 见 §4.3
                       │
                       ▼  UiMsg::Plugins(Result<Vec<PluginRow>, String>)
             AppState.plugins ──► project() ──► MainWindow.plugin-rows / plugin-summary

用户点〔安装〕/〔更新〕/〔更新全部〕/〔卸载〕
   │
   ├─ 校验（只对"安装"的输入框，见 §7）→ 不通过：状态栏提示，不派发
   ├─ 卸载：先弹确认框（唯一对话框，见 §8.4）
   └─ Job::PluginOp { op }（worker 线程）
        ├─ 记日志：`$ dsh.cmd plugin --profile web add …`（FR-28 的命令原文）
        ├─ pm::run_cmd("dsh.cmd" → 回落 "dsh.exe", args)          ← F11，CREATE_NO_WINDOW
        ├─ 退出码非 0 → 原文进日志 + UiMsg::Failed（状态栏短提示）
        └─ 无论成败 → 紧接着做一次 FetchPlugins（见 §5.3），再发 PluginOpDone
```

**职责边界：**

- **`src/plugin.rs` 只管三件事**：定位 profile、盘点已装插件、构造 `dsh plugin` 参数与查 registry。**不含 UI 逻辑，也不含"什么时候该拉取"的决策**（与 `config.rs` 的边界同款）。
- **Rust 是唯一真相源。** 可更新与否、汇总文案（"6 个插件 · 1 个可更新"）都在 Rust 侧算完，Slint 只渲染字符串 —— 沿用 `version_labels()` 的既有做法（`ui/app.slint` 的下拉里"标注通道"就是拼好字符串推过去的）。
- **UI 只上报意图。** 新增 6 个 callback（§8.3），与既有 `callback refresh-clicked()` / `callback install-clicked()` 完全同形。
- **worker 无状态。** `Job::PluginOp` 自包含全部上下文（与 `Job::Transact` 携带 `origin` 同理）。

---

## 4. 数据来源

### 4.1 profile 定位（纯函数，可单测）

```rust
/// %DSH_HOME%\profiles\web；DSH_HOME 缺失时回落 %USERPROFILE%\.dsh\profiles\web。
/// 两个环境变量都拿不到 → None（UI 显示"未找到 profile"，全部控件禁用）。
pub fn profile_dir_from(dsh_home: Option<OsString>, userprofile: Option<OsString>) -> Option<PathBuf>
```

不新增依赖（沿用 `config::state_path` 用 `std::env::var_os("APPDATA")` 的同款做法）。profile 名是常量 `"web"`。

### 4.2 已装插件列表（纯函数，可单测）

```rust
pub struct PluginRow {
    pub name: String,            // "@hilariouhiss/dsh-gitbash"
    pub spec: String,            // "^1.0.1"（package.json 里的原样规格）
    pub installed: Option<Version>,  // node_modules 里读到的版本；读不到 → None
    pub latest: Option<Version>,     // registry 的 dist-tags.latest；查询失败 → None
}

/// 只依赖传入的两个目录，不做网络。测试在 tempdir 里造一个假 profile 即可穷尽。
pub fn read_installed(profile: &Path) -> Result<Vec<PluginRow>, String>
```

- 列表来源**只**是 `package.json` 的 `dependencies`（F5/F6）。⚠ **不要**去遍历 `node_modules`：那里有传递依赖（F6），会把 `dsh-skill-kit` 这种列进来，与用户看到的 `pnpm list` 输出不一致。
- **不过滤包名。** profile 的 `dependencies` 本来就只有第三方插件 —— DSH 自带的 bundle（`@deepseek-ai/dsh-base` / `dsh-web-app` / `dsh-experimental-agent-team-profile`）只在 `dsh.profile.bundles` 里，不在 `dependencies` 里。**不要顺手加 `@deepseek-ai` 过滤**：那是把一条实测事实换成一条猜测的启发式。
- `spec` 原样保留（`^1.0.1` / `1.2.1` 混排是现状），推给 UI 作次要信息 —— 用户能一眼看出哪些是钉死的。

### 4.3 最新版本

| 项 | 取值 |
|---|---|
| 请求 | `GET https://registry.npmjs.org/<包>`，scoped 包把 `/` 转义成 `%2F`（`@scope%2Fname`）。两种写法 registry 都接受，转义是为了不被任何中间层做路径处理 |
| 头 | `Accept: application/vnd.npm.install-v1+json`（精简 packument，与 `fetch_catalog` 同款，`src/dsh.rs:290`） |
| 取用 | `dist-tags.latest` |
| 代理 | `dsh::agent()` —— 已有系统代理支持（FR-33），**不需要新代码** |
| 并发 | `std::thread::scope`，每包一线程，一个 `Agent` clone 进去共用；6 个包 ≈ 一次请求的墙钟时间（本机单次实测 4.1s，串行会是 ~25s） |
| 失败 | 该行 `latest = None` → UI 显示 `—`、〔更新〕禁用；**不影响其他行**（与 FR-6/FR-21 的"单个失败不牵连"同款） |
| 比较 | `latest > installed` 才算可更新（`semver` 已在依赖里） |

⚠ **这里用 `latest` tag 是对的，不要照抄 FR-8 的禁令。** FR-8 禁止把 `latest` 当"最新"是针对 **dsh 自身**：它有 alpha/rc/stable 三条通道，`latest` 比已装的 alpha 版更旧（SRS §2.2.5 实测）。**第三方插件没有通道概念**，`dist-tags.latest` 就是它语义正确的最新版。这条会写进代码注释，免得下一个人"顺手改对"。

---

## 5. 命令与操作

### 5.1 命令表（`Op` → pnpm 参数，表驱动）

| 操作 | 发给 `dsh` 的参数 | 说明 |
|---|---|---|
| 安装 | `plugin --profile web add <用户输入的规格>` | 规格来自输入框，先过 §7 白名单 |
| 更新单个 | `plugin --profile web add <包>@<latest>` | 显式版本 |
| 更新全部 | `plugin --profile web add <包1>@<v1> <包2>@<v2> …` | **一次子进程**，见 §5.2 |
| 卸载 | `plugin --profile web remove <包>` | |

**为什么更新用 `add <包>@<版本>` 而不是 `pnpm update`**：前者目标确定，不依赖 pnpm 的 range / save-prefix 语义。现状里 6 个有 3 个是精确钉死的（`1.2.1` / `1.1.0` / `1.0.0`），`pnpm update` 对它们不会动，用户会看到"点了没反应"。

**可执行文件解析**：按 `SHIM_NAMES` 顺序尝试（`dsh.exe` → `dsh.cmd`），与 `spawn_web` 同一套（F11）。`pm::run_cmd` 已带 `CREATE_NO_WINDOW`（GC-8），不重复实现。

### 5.2 「全部更新」= 一次 `add` 多规格（刻意的取舍）

- **做法**：把所有 `latest > installed` 的行拼成一条命令，一次子进程。
- **收益**：一次 pnpm 解析/安装、一次加锁、一次 reconcile（F2），而不是 N 次串行（每次几秒到几十秒）。
- **代价**：颗粒度变粗 —— 一条命令里任一规格失败，整批不生效。日志里有 pnpm 的原文，用户可退回逐行更新。
- **不做的**：不做"逐个跑、逐个汇报成功/失败"的编排（那是 N 倍耗时与 N 份代码）。这条取舍写在这里，将来真需要颗粒度再改。

### 5.3 操作完成后自动重拉

`Job::PluginOp` 在命令跑完后**紧接着**做一次 §3 的盘点（同一个 Job、同一个线程），然后依次发 `PluginOpDone` 与 `Plugins`。这样"装完列表就更新了"，不需要 UI 再派发一次、也不会有"操作完成但列表还是旧的"的中间态。

---

## 6. 状态与消息

### 6.1 `src/model.rs`

```rust
pub enum PluginOp {
    Install(String),                       // 用户输入的规格
    Update { name: String, version: Version },
    UpdateAll(Vec<(String, Version)>),      // 已按可更新项筛好
    Remove(String),
}

impl PluginOp {
    /// `dsh` 的参数表（表驱动，不散落在分支里）。
    pub fn args(&self, profile: &str) -> Vec<String>;
    /// 日志与状态栏用的一句话（如 `更新 dsh-colgrep → 1.3.0`）。
    pub fn describe(&self) -> String;
}

pub enum Job {
    // …既有…
    FetchPlugins,
    PluginOp { op: PluginOp },
}

pub enum UiMsg {
    // …既有…
    Plugins(Result<Vec<PluginRow>, String>),
    PluginOpDone { op: PluginOp, ok: bool },
}
```

### 6.2 `AppState`（`src/main.rs`）

```rust
struct PluginsState {
    rows: Vec<PluginRow>,
    status: PluginStatus,     // Loading / Ok / Failed(String)  —— 与 NotesState 同款
}
```

- 复用既有 **`busy`** 闸门：派发 `PluginOp` 时置真、收到 `PluginOpDone` 时清掉 —— 与 `Job::Transact` 的置位/清除路径一致（评审时确认这两条路径没有相互覆盖的窗口）。
- `FetchPlugins`（只读）**不**置 `busy`，只把 `plugins.status = Loading`。
- **不复用 `dirty`**：`Plugins` / `PluginOpDone` 都是排空到的消息，`drain` 已经会把该帧标成 changed。

### 6.3 `project()` 推送

```rust
win.set_plugin_rows(model_from(&state.plugin_rows()));   // 已算好的 [PluginRow]（含格式化字符串）
win.set_plugin_summary(state.plugin_summary().into());   // "6 个插件 · 1 个可更新" / "检查中…" / …
win.set_plugins_loading(...);
```

⚠ 任何改 `AppState` 的路径都必须在同一次排空末尾走一次 `project()`（ARCHITECTURE §3.4 的强制约定）；托盘没有插件相关属性，**不需要**多推一份。

---

## 7. 输入校验（信任边界，不做懒）

安装输入框只接受 **npm registry 包规格**：

```
规格    := 名称 ( "@" 版本 )?
名称    := ( "@" scope "/" )? pkg
字符集  := [A-Za-z0-9._~-]（scope 与 pkg 同）；版本段额外允许 [A-Za-z0-9.+-]
```

**明确拒绝**：`file:` / `link:` / `git+` / `github:` / `http(s):` / 任何含路径分隔符 `/`（除 scope 那一个）或 `\` 的输入 / 空白 / 引号 / 空串。

两条理由，都不是洁癖：

1. **安全边界** —— `file:` 与 `git+` 规格会让 pnpm 从本机路径或远端 git 拉代码并在 profile 里执行其构建脚本。本程序只承诺"装 registry 上的包"。
2. **SRS CON-4**（只允许 `registry.npmjs.org` 与 `api.github.com`）—— 用户手打的 `git+https://…` 会让子进程去连别的**主机**，那就破了自己写下的约束。

校验失败：状态栏给一句可读提示（如"包规格不合法：只支持 registry 上的包，形如 `@scope/name@1.2.3`"），**不派发 Job、不写日志**（沿用 `on_install_clicked` 两条拒绝路径的既有做法）。

---

## 8. UI

### 8.1 位置与高度分配

```
右栏（horizontal-stretch: 1）
├── 【新】插件卡 Bezel                      ← §8.2
└── 更新说明卡（现有，vertical-stretch: 1，min-height: 260px）
```

- 插件卡**按内容取高**，上限为"可见 6 行"；超出由卡内 `ListView` 自己滚。
- 更新说明保留 `vertical-stretch: 1` 吃剩余高度；窗口被拉矮时**优先保更新说明的 `min-height: 260px`**（长文阅读的下限），插件卡压到自己的 `min-height`（约 3 行可见）仍可滚。
- **具体像素在实现时对着运行中的窗口实测调**（本仓库的惯例：`ui/app.slint` 里每一处尺寸注释都是量出来的）。本设计只钉"谁让位、各自下限是多少"。
- ⚠ 顺带改掉一句已经失效的注释：`ui/app.slint:1627-1628` 的"右栏整列让给更新说明（长文阅读需要整列宽度与高度）" —— 加了本卡之后它不再成立。
- ⚠ 同样要改 `ui/app.slint:747-748` 的"本程序唯一的调用点是 4 位端口号"（F13）。

### 8.2 卡片结构（全部复用既有组件）

```
Bezel {
  SectionHeader { label: "插件"; trailing: <汇总> }   // 汇总 = "6 个 · 1 个可更新"，Rust 侧拼好
  HorizontalLayout {                         // 安装行
    GlassField { placeholder: "@scope/name 或 @scope/name@1.2.3"; text-align: left }
    PillButton { label: "安装"; show-glyph: false }
  }
  Divider { }
  ListView { for row in plugin-rows : PluginRowView { … } }   // vertical-stretch: 1 + min-height
  HorizontalLayout {                         // 汇总行
    Text { <状态文案，见下> }
    Rectangle { horizontal-stretch: 1 }
    PillButton { label: "全部更新"; … }
    PillButton { label: "刷新"; … }
  }
}
```

- **两处文案各管一件事，不要合并**：`SectionHeader.trailing` 是**汇总**（"6 个 · 1 个可更新"）；底行左侧 `Text` 是**状态**（`检查中…` / `读取 profile 失败：<原因>` / 上一条操作的结论）。两者来源是 §6.3 的 `plugin-summary` 与 `plugins-loading`/错误文案，不重复显示同一句话。
- 〔全部更新〕在"可更新项为 0"或 `busy` 时**禁用**；〔刷新〕只在 `plugins-loading` 时禁用。
- 复用：`Bezel` / `SectionHeader` / `GlassField`（749）/ `PillButton`（181）/ `Divider`（497）/ `Tag`（376）/ `ListView`（需 `import … from "std-widgets.slint"`，已在文件里）。
- 输入框用 `text-align: left`（`GlassField` 的缺省是 `center`，是给 4 位端口号用的）。
- ⚠ **不要**用 `ScrollView` 包 `ListView`（会按内容全高布局、虚拟化失效）—— 与日志区同一条坑，`ui/app.slint:1582-1584` 已记过。
- ⚠ 三态用 `if` 而不是 `visible:`（不可见元素照样占布局位置，`ui/app.slint:1654-1660` 已记过）。

**每行两行式**（右栏可用宽度实测约 420px，塞不下"长包名 + 规格 + 两个版本 + 两个按钮"的一个单行）：

```
@hilariouhiss/dsh-colgrep                    ← 等宽，超出省略
1.2.1 → 1.3.0  可更新        〔更新〕〔卸载〕  ← 版本 + Tag + 动作
```

- 已是最新：`1.3.0  已是最新`，不显示〔更新〕（禁用不如不显示）。
- 最新版未知：`1.2.1 → —  未知`，〔更新〕禁用。
- 已装版本读不到：显示`未安装`，按钮文案改〔安装〕（走同一条 `Update` 路径，按 latest 重新装）。
- 规格 `^1.0.1` 作为次要文案并入第一行行尾（`ink-4`），不单独占一行。

### 8.3 回调与属性契约（`main.rs` ↔ `app.slint`）

```slint
export struct PluginRow {
    name: string, spec: string,
    installed: string, latest: string,      // 已格式化；未知/未装分别是 "—" / "未安装"
    updatable: bool, missing: bool,
}

in property <[PluginRow]> plugin-rows: [];
in property <string> plugin-summary: "";
in property <bool>   plugins-loading: false;
in-out property <string> plugin-input: "";        // 安装输入框
in-out property <bool> plugin-remove-prompt-visible: false;   // §8.4

callback plugin-install();                 // 读 plugin-input
callback plugin-update(int);               // 行下标
callback plugin-update-all();
callback plugin-remove(int);               // 行下标 → 弹确认框
callback plugin-remove-confirmed();        // 确认框里的"卸载"
callback plugin-refresh();
```

Rust 侧把行下标映射回 `PluginRow`（`state.plugins.rows.get(i)`），**不把包名当字符串从 UI 传回来**（少一条能被 UI 伪造的输入路径）。

### 8.4 唯一的对话框：卸载确认

破坏性动作，沿用"首次关闭询问框"那套（`ui/app.slint:2057-2135`：遮罩吃点击 + 空壳 `FocusScope` 抢焦点管 Esc + `Bezel` 400px + 两个 `PillButton`）：

```
确认卸载 @hilariouhiss/dsh-gitbash ？
将从 profile 移除该依赖（已装版本 1.0.1）。卸载后如需恢复，可重新安装。
                                        〔卸载〕  〔取消〕(emphasis)
```

- Esc / 点遮罩 / 右上角关闭 = 取消（不替用户做决定）。
- 无"记住选择"（这不是可以记的偏好）。
- 文案不写"卸载后 dsh web 需重启"，因为不需要（F9）。

---

## 9. 错误处理

| 情况 | 行为 |
|---|---|
| `dsh plugin` 非零退出 | 日志：命令原文 + stdout/stderr 原文（FR-28）；状态栏：`插件操作失败，详见日志` |
| dsh / pnpm 不在 PATH | `run_cmd` 返回 `Err` → 同上一条；日志里带 `SHIM_NAMES` 两个名字与各自的错误（照 `spawn_web:650` 的文案形状） |
| profile 目录不存在 | 卡片显示"未找到 profile：`<路径>`"，全部控件禁用；**不自动创建**（创建 profile 是 `dsh` 的事，F2 的 `runPluginCommand` 自己会 init） |
| `package.json` 损坏 / 无 `dependencies` | `Plugins(Err(...))` → 卡片显示"读取 profile 失败"+ 日志；其余功能不受影响（与 FR-32 的"持久化失败不得牵连主操作"同款精神） |
| registry 查询失败 | 该行 `latest` 未知（`—`），其他行照常；日志一行 |
| 全部更新的一部分失败 | 整条命令失败 → 同上；列表重拉后仍是"可更新"，用户可逐行再试 |
| worker 崩溃 | 既有 §5.2 兜底覆盖，本功能不新增机制 |

不引入错误处理库（ARCHITECTURE §5.3）；错误一律是"承载用户可读消息的 `String`"。

---

## 10. 测试与验证

### 10.1 单测（纯函数，`cargo test`，全部可自动化）

| 目标 | 用例 |
|---|---|
| `valid_spec` | 表驱动：`@hilariouhiss/dsh-gitbash`、`@scope/name@1.2.3`、`name@1.0.0-rc.1` 通过；`file:../x`、`link:./x`、`git+https://…`、`https://…`、`../x`、`C:\x`、`@scope/`、``、`a b`、`a"b` 拒绝 |
| `read_installed` | 在 tempdir 造假 profile：`package.json` 含 2 个依赖 + `node_modules/<包>/package.json`（一个能读到版本、一个缺失）⇒ 断言顺序、`spec` 原样、`installed` 的 `Some/None`；再断言**不在 dependencies 里的目录不出现在结果里**（F6 的回归钉子） |
| `PluginOp::args` | 表驱动：四种 `PluginOp` 的参数形状（含"更新全部"的**多规格单命令**形状：`["plugin","--profile","web","add","a@1.0.0","b@2.0.0"]`） |
| `parse_latest` | 真实 packument 片段（含 `dist-tags.latest`）→ `Version`；缺 `dist-tags` / JSON 非法 → `None`（不 Err） |
| `profile_dir_from` | `DSH_HOME` 有 / 无（回落 `USERPROFILE`）/ 两个都没有 → `None` |
| 可更新判定 | `1.2.1 → 1.3.0` 可更新；`1.3.0 → 1.3.0` 不可；预发布版本序按 semver（`1.0.0-rc.1 < 1.0.0`） |

### 10.2 冒烟（假 shim，可自动化）

照 `src/pm.rs:498-545` 的既有手法：在 tempdir 里放一个真的可执行 `dsh.cmd`（内容 `@echo off` + 回显参数），让插件操作指向它，断言 **① 实际执行的参数逐字正确、② 退出码 0 与非 0 两条路径分别走到"成功"与"失败"分支、③ 命令原文确实进了日志**。这条不需要 pnpm，因此在我的沙箱里也能跑 —— 它是"参数构造"这一层唯一有牙齿的测试。

### 10.3 端到端（**需要用户执行** ⚠）

> ⚠ **本会话的沙箱跑不了 pnpm**：`pnpm --version` 直接返回 `The path cannot be traversed because it contains an untrusted mount point`，`dsh plugin …` 因此在**我这儿**必然失败（你能跑，我不能 —— 你贴的那份 `list` 输出就是证据）。所以下列验证**必须由你在真实环境点一次**，我负责把 10.1 / 10.2 全部自动化，并按 `docs/VERIFICATION.md` 的体例给出逐条清单：

| # | 步骤 | 通过判据 |
|---|---|---|
| V-P1 | 启动程序，看插件卡 | 6 行、包名与 `dsh plugin --profile web list` 一致；已装版本 = `node_modules` 里的真实版本；最新版列在几秒内填好 |
| V-P2 | 点某行〔更新〕（选一个确有新版可更的） | 日志出现 `$ dsh.cmd plugin --profile web add <包>@<版>` 与 pnpm 输出；`package.json` 里该包的规格与版本真的变了；列表自动刷新为"已是最新" |
| V-P3 | 输入一个已知包（如 `@hilariouhiss/dsh-gitbash@1.0.1`）点〔安装〕 | 同上；`dependencies` 新增一条，且**新包自动进了 `dsh.profile.bundles`**（F3 的实证） |
| V-P4 | 点某行〔卸载〕→ 确认框 → 取消 | 什么都不发生（`package.json` 未变、日志无命令） |
| V-P5 | 再点〔卸载〕→ 确认；**并验证 dsh web 正在运行时也能成功**（F9 的核心断言） | 依赖从 `dependencies` 移除、`dsh.profile.bundles` 里也不再残留（F3）；**`dsh web` 进程未被打断** |
| V-P6 | 输入 `file:../../evil` 点〔安装〕 | 状态栏报"规格不合法"，**日志里没有命令**、`package.json` 未变 |
| V-P7 | 〔全部更新〕 | 一次命令、多条规格；全部更新完列表变"已是最新" |

### 10.4 不做

GUI 自动化测试、pnpm 行为 mock、真实 registry 的集成测试（ARCHITECTURE §6.3 的既有边界）。

---

## 11. 文档修订清单

| 文档 | 修订 |
|---|---|
| `docs/SRS.md` → **v1.4** | ① §1.3 "范围外"删去"`dsh profile` 内插件的管理"，改写为"由 `dsh plugin` 转发管理，本项目不直接干预 pnpm 与 profile 文件"；② **CON-5 改写**（原文是硬禁令，理由"dsh 硬编码 pnpm、本项目不得干预"），新形态为"变更一律经 `dsh plugin --profile <name>` 转发；**只读** profile 的 `package.json` 与 `node_modules/<包>/package.json` 以渲染列表"；③ §2.2.9 补一句"转发的实现事实已在 `docs/superpowers/specs/2026-09-22-plugin-manager-design.md` §2 核实"；④ 新增 **§3.9 插件管理**：FR-34 列表 / FR-35 安装 / FR-36 更新（含全部更新）/ FR-37 卸载 + 触发时机；⑤ §5.1.1 主窗口草图加插件卡；⑥ §5.2 接口表加一行 `dsh plugin --profile web add\|remove`；⑦ **CON-4 不改**（它约束的是**主机**，本功能只用到 `registry.npmjs.org` 的另一个路径 —— 在 §5.2 下点明这一点，免得下一个人以为要放宽主机白名单）；⑧ 新增 **NFR-12**：插件最新版查询必须并行、必须后台，进入插件卡前 UI 不得阻塞（依据：单次 registry 4.1s 实测 × 6 串行 = 25s） |
| `docs/ARCHITECTURE.md` → **v1.7** | ① §1.3 模块图加 `plugin.rs`（依赖方向：`model.rs` ← `plugin.rs` → `dsh.rs`/`pm.rs`）；② §1.4 数据流加插件一条；③ §3.2 属性契约补 §8.3 的五个属性与六个 callback；④ 新增 **§4.8 `src/plugin.rs`**（职责边界、纯函数清单、为何走 `dsh plugin` 而非直接 pnpm、为何用 `latest` tag 是对的）；⑤ §6.1 纯函数表加 `valid_spec` / `read_installed` / `plugin_args` / `parse_latest` / `profile_dir_from`；⑥ §7.1 依赖表**不变**（零新增依赖）；⑦ §9.3 变更记录 |
| `docs/VERIFICATION.md` | 加 V-P1 ~ V-P7（§10.3）与 10.1/10.2 的自动化结果 |

---

## 12. 已知限制

1. **不显示插件的描述 / 作者 / 主页。** 要拿这些得读 packument 的更多字段或读已装包的 `package.json`；本轮只解决"装了什么、能不能更新"。
2. **更新全部是一个事务性很弱的批。** 一条 pnpm 命令、一荣俱荣（§5.2）。
3. **`minimumReleaseAge` 拦下的版本会表现为"点了没更新"**，此时日志里有 pnpm 的原文（它会直接告诉用户该往 `pnpm-workspace.yaml` 的 `minimumReleaseAgeExclude` 加什么）。本项目**不代改**那个文件。
4. **构建脚本被 pnpm 拦下时**同样只有日志里的原文。DSH 自带插件页有"允许这些脚本并重试"的能力（它写 `pnpm-workspace.yaml`），本项目不复制 —— 代价是这类插件要用户自己处理一次。
5. **列表以外的手工改动不会被实时发现**：在外部终端 `dsh plugin add` 之后，本程序要等你点〔刷新〕（或下一次插件操作）才会看到。不做文件监视（`notify` 是新增依赖，且 profile 的写入者不止一个）。
6. **不校验"这个包是不是一个 bundle"**：装一个没有 `dsh.bundle` 的普通依赖时，DSH 会在 reconcile 里打一行 warning（F3 的代码路径），它进了我们的日志 —— 这就够了，不做预检。

---

## 13. 变更记录（含被证伪的条目）

| 日期 | 变更 | 理由 |
|---|---|---|
| 2026-09-22 | 首版：落点定为**右栏、更新说明之上**；只做安装/更新/卸载（不做禁用） | 用户决定 |
| 2026-09-22 | 加**〔全部更新〕**，实现为一次 `add` 多规格 | 用户要求；取舍见 §5.2 |
| 2026-09-22 | **撤回**"变更前必须先停 `dsh web`"及其弹窗 | ⚠ 原设计的理由是"profile 内有原生模块（node-pty / cpu-features / ssh2），运行中的进程会锁住 `.node` 文件"。**该前提实测为假**：`allowBuilds` 里那三个名字是 DSH 写的白名单模板，profile 的 `node_modules` 里**一个 `.node` 都没有**（F8）。反证还有两条：`dsh plugin` 与运行中的服务**共用同一把 profile 写锁**（F9），CLI 改完由 HMR 应用；DSH 自带的 Web 插件页本身就跑在运行中的服务器里。而"停 → 重启"的真实代价是**杀掉用户正在跑的会话**。故改为：不停服务、不弹窗、**`Job::Transact` 的 TR-1 路径逐字不动**（这也是本设计为什么完全不碰现有事务路径） |
| 2026-09-22 | 列表来源钉死为 `package.json` 的 `dependencies`，并**不加** `@deepseek-ai` 过滤 | F5/F6：`node_modules` 含传递依赖（`dsh-skill-kit`）；而 profile 的 `dependencies` 本来就只有第三方插件 |
