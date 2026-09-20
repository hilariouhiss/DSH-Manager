# DSH Manager 架构设计与关键模块详细设计

| 项目 | 内容 |
|---|---|
| 文档版本 | 1.2 |
| 日期 | 2026-09-20 |
| 关联文档 | [docs/SRS.md](SRS.md) v1.2（需求依据） |
| 技术栈 | Rust 2024 edition + Slint 1.18 |
| 状态 | 待实现 |

---

## 0. 本文档的定位

### 0.1 取舍说明

本文档**不是**一份面面俱到的详细设计，而是一次有取舍的设计交付：

| 部分 | 深度 | 理由 |
|---|---|---|
| 架构总览、并发模型、状态所有权、模块划分 | **写全** | 这些决策错了会全盘返工 |
| Slint ↔ Rust 接口契约 | **写全** | 接口定了，两侧可并行开发 |
| `txn.rs` 事务引擎 | **详细到算法级** | 唯一有破坏性后果的模块（SRS §4） |
| `pm.rs` 探测 | **详细到纯函数签名** | 逻辑分支多且需单元测试（SRS §8.4） |
| `dsh.rs` 网络与进程监督 | **详细到函数签名** | 涉及子进程生命周期与平台陷阱 |
| 日志面板、关于对话框、控件禁用态 | **仅架构交代** | 无设计风险，实现时随手完成 |

**刻意不写**：逐控件的像素布局、每个错误提示的完整文案、每个函数的完整实现。目前一行代码都没有，把这些写死属于凭空推测。

### 0.2 本文档对 SRS 的修订

设计与需求核对时发现 4 处需要修订 SRS，已在 §8 列出。**在 SRS 更新到 v1.1 之前，两处冲突以本文档为准。**

---

## 1. 架构总览

### 1.1 进程与线程拓扑

```
┌─ dsh-manager.exe  （单进程，windows_subsystem = "windows"） ──────────────┐
│                                                                          │
│  ┌─ UI 线程（主线程）────────────────────────────────────────────────┐  │
│  │                                                                   │  │
│  │   slint::run_event_loop_until_quit()          ← FR-23             │  │
│  │                                                                   │  │
│  │   AppState  ◄── 独占所有权，【无锁】                               │  │
│  │     pm_env / catalog / notes / web / tx / log(Rc<VecModel>)        │  │
│  │                                                                   │  │
│  │   slint::Timer (80ms)  ──► drain(msg_rx) ──► project(state)        │  │
│  │                                                  │                │  │
│  │                              ┌───────────────────┴──────────┐     │  │
│  │                              ▼                              ▼     │  │
│  │                     MainWindow 实例                  AppTray 实例  │  │
│  │                     （窗口）                          （托盘）      │  │
│  │                     ⚠ 两者不共享 global，必须推两份（FR-25）        │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│         ▲  UiMsg                      Job  ▼                             │
│         │  mpsc                       mpsc  │                            │
│  ┌──────┴────────────────────────────────────┴───────────────────────┐  │
│  │  worker 线程（1 个，无状态）                                        │  │
│  │    loop { let job = job_rx.recv()?; execute(job, &msg_tx) }        │  │
│  │    串行执行，天然满足 FR-15（禁止并发事务）                          │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌─ dsh web 子进程的辅助线程（每次启动 3 个）────────────────────────┐  │
│  │    stdout reader ──► UiMsg::Log                                  │  │
│  │    stderr reader ──► UiMsg::Log                                  │  │
│  │    waiter: join(readers) → child.wait() → UiMsg::WebExited        │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌─ dsh web 子进程（分离，CREATE_NO_WINDOW）─────────────────────────┐  │
│  │    dsh.cmd web --port <p>   →  node.exe 监听 127.0.0.1:<p>        │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────┘
```

### 1.2 三条核心架构决策

#### 决策 1：UI 线程独占 `AppState`，**不使用 `Arc<Mutex<..>>`**

**做法**：`AppState` 是 UI 线程的普通局部变量（外层 `Rc<RefCell<..>>` 仅为供 Timer 闭包捕获）。worker 线程**不持有任何共享状态**。

**替代方案（被否决）**：把状态放进 `Arc<Mutex<AppState>>`，UI 与 worker 共享。

**否决理由**：
- 共享可变状态会引入锁顺序、死锁、UI 卡顿等一整类问题
- worker 其实不需要"看"全局状态 —— 它需要的一切都可由 UI 线程打包进 `Job` 载荷。例如事务需要 `origin=(PM_old, V0)`，UI 线程在派发时就把这两个值填好
- 状态只有一个写者（UI 线程）时，**状态机推理是局部的**，这直接降低了 §4 事务逻辑的实现风险

**这条决策是整个架构里最有价值的一条**：它把"并发正确性"问题转化成了"消息传递正确性"问题。

#### 决策 2：`dsh web` 子进程采用 actor 模型，**Child 句柄不共享**

**做法**：
- 停止子进程**统一走 `taskkill /PID <pid> /T /F`**，只需要 PID，不需要 `Child` 句柄
- `std::process::Child` 被 move 进 waiter 线程，仅用于 `wait()` 回收，避免句柄泄漏
- 因此**没有任何跨线程共享的进程句柄**，也就不需要锁

**额外收益**：因为停止只依赖 PID，SRS FR-19 的"自己启动的"与"外部启动的"两条路径**合并成同一个实现** —— 区别仅在于 PID 从 `child.id()` 来还是从 `netstat` 解析来。这比分别实现两套停止逻辑更短。

**为什么用 `taskkill /T` 而不只是 `Child::kill()`**：`/T` 会终止整棵进程树。`Child::kill()` 底层是 `TerminateProcess`，只杀单个进程。若 `dsh web` 内部派生了 worker 子进程，残留进程会继续占着端口。

#### 决策 3：UI 更新走 **80ms Timer 排空**，不用 `invoke_from_event_loop`

| 方案 | 评价 |
|---|---|
| `invoke_from_event_loop` 逐条投递 | 可行，但每条消息一个闭包，状态散落在 N 个闭包里更新 |
| **`slint::Timer` + `mpsc` 排空**（采纳） | 全部更新收敛到 `drain()` + `project()` 两个函数；天然满足 FR-25（一次排空同时推两份） |

**代价**：日志最多延迟 80ms 可见。对日志与进度显示完全无感知。

**约束**：`slint::Timer` 被文档标注为 `!Send + !Sync`，且官方明确 *"Timers can only be used in the thread that runs the Slint event loop. They don't fire if used in another thread."* 因此 Timer 必须创建并持有在 UI 线程，且**必须保持存活**（文档原文：*"You must keep the Timer object around for as long as you want the timer to keep firing"*）。

### 1.3 模块划分与依赖方向

```
                    ┌──────────────┐
                    │  model.rs    │  纯数据：Job / UiMsg / AppState
                    │  （无逻辑）   │  被所有模块依赖，不依赖任何模块
                    └──────┬───────┘
                           │
        ┌──────────────────┼──────────────────┬──────────────┐
        ▼                  ▼                  ▼              ▼
  ┌──────────┐      ┌──────────┐      ┌──────────┐   ┌──────────┐
  │  pm.rs   │      │  dsh.rs  │      │  txn.rs  │   │ config.rs│
  │ PM 探测  │◄─────│ 版本/网络 │◄─────│ 事务引擎 │   │ state.json│
  │ 命令表   │      │ 进程监督  │      │ 补偿逻辑 │   │ 读写     │
  └──────────┘      └──────────┘      └──────────┘   └──────────┘
        ▲                  ▲                  ▲              ▲
        └──────────────────┼──────────────────┴──────────────┘
                           │
                    ┌──────┴───────┐
                    │  main.rs     │  入口 / UI 循环 / worker 循环 /
                    │              │  wire_callbacks / project
                    └──────────────┘
                           │
                    ┌──────┴───────┐
                    │ ui/app.slint │  MainWindow + AppTray
                    └──────────────┘
```

**依赖规则的唯一硬约束**：`model.rs` 不得依赖任何其他模块。它是纯数据类型层，这样 `txn.rs` 的单元测试可以只依赖 `model.rs` + 一个假 backend。

**`config.rs` 的定位**：只做 `state.json` 的 I/O —— 路径解析、读取与缺省回退、损坏处理、写入。**不含 UI 逻辑，也不含"何时该写"的决策**。写入时机由调用方（`dsh.rs` 的启停路径、`main.rs` 的端口变更处理）决定。这条边界让 FR-30 ~ FR-32 的行为可以被单元测试穷尽，而不需要启动 GUI。

### 1.4 数据流

```
启动
  │
  ├─ Job::Probe        ──► PmEnv { available, owner, installed, dsh_path }
  └─ Job::FetchCatalog ──► Catalog { versions[22], tags{latest,next,alpha} }
                                    │
                                    ▼
                        UI 派生显示值（纯函数）
                        · channel_of(installed)      → "alpha"
                        · latest_in(versions, alpha) → "0.1.6-alpha.2"
                        · latest == installed        → "✓ 已是最新"
                                    │
用户选中某版本 ──► Job::FetchNotes ──► preprocess_notes ──► StyledText
                                    │
用户点"安装"  ──► Job::Transact { origin, target, port }
                                    │
                                    ├─ TxProgress 步骤  ──► 状态栏 + 日志
                                    └─ TxDone(TxOutcome) ──► 结果提示
                                                             │
                                                    TR-7 重新 Probe
                                                             │
                                                    事务前若在运行 → 重启 web
```

---

## 2. 并发与通信模型

### 2.1 线程清单

| 线程 | 数量 | 生命周期 | 职责 |
|---|---|---|---|
| UI 线程 | 1 | 进程全程 | Slint 事件循环、`AppState` 读写、80ms 排空、状态推送 |
| worker | 1 | 进程全程 | 串行执行 `Job` |
| stdout reader | 每个 web 子进程 1 个 | 子进程存续期 | 读子进程 stdout → `UiMsg::Log` |
| stderr reader | 每个 web 子进程 1 个 | 子进程存续期 | 读子进程 stderr → `UiMsg::Log` |
| waiter | 每个 web 子进程 1 个 | 子进程存续期 | `join` 两个 reader → `child.wait()` → `UiMsg::WebExited` |

**为什么 stdout / stderr 需要两个线程**：管道缓冲区满时写端会阻塞。若单线程串行读，先读的那个管道阻塞时另一个管道的缓冲区会被写满，导致子进程卡死 —— 经典死锁。

**为什么 waiter 必须 `join` 两个 reader 之后才 `wait()`**：保证子进程退出时其输出已被完整读取，否则日志会截尾。

### 2.2 Job / UiMsg 协议

```rust
// src/model.rs

/// UI 线程 → worker。worker 无状态，因此 Job 必须自包含全部上下文。
pub enum Job {
    Probe,
    FetchCatalog,
    FetchNotes { version: Version },
    Transact { origin: Origin, target: Target, port: u16 },
    StartWeb { port: u16 },
    StopWeb { pid: u32 },
    OpenUrl { url: String },
}

/// worker → UI 线程
pub enum UiMsg {
    Probed(PmEnv),
    Catalog(Catalog),
    Notes { version: Version, result: Result<String, NotesError> },
    Log(LogLine),
    TxProgress(TxStep),
    TxDone(TxOutcome),
    WebState(WebState),
    WebExited { code: Option<i32> },
    Failed { context: &'static str, message: String },
}
```

**设计要点**：`Job` 完全自包含。注意 `Transact` 携带 `origin`，这是决策 1 的直接体现 —— worker 不需要去查"当前装的是什么"，UI 在派发时已经填好。

### 2.3 通道

| 方向 | 类型 | 持有者 |
|---|---|---|
| UI → worker | `mpsc::Sender<Job>` | UI 线程持有 sender；worker 持有 receiver |
| worker → UI | `mpsc::Sender<UiMsg>` | worker 持有 sender；UI 线程持有 receiver |

两个 `mpsc`，无锁。worker 崩溃（`recv()` 返回 `Err`）时优雅退出，UI 侧表现为消息停止，需在 `drain()` 中检测并提示（见 §5.2）。

### 2.4 实现前验证（R-1 / R-2 已完成）

本节原为动工前的风险清单。**R-1 与 R-2 已于 2026-09-20 用最小 spike 实测解决**，其中 R-2 还证伪了本文档的一处代码。

| 编号 | 状态 | 结论 |
|---|---|---|
| **R-1** | ✅ **已解决** | `Timer::start` 的回调**无 `Send` 约束**。闭包可捕获 `Rc<RefCell<AppState>>`、`Rc<VecModel<..>>`、`mpsc::Receiver<..>`。**架构决策 3 成立，无需退回 `invoke_from_event_loop`** |
| **R-2** | ✅ **已解决**（并修正了本文档 §4.5 的一处错误） | 详见 §2.4.2 |
| **R-3** | ⬜ 待处理 | `SystemTrayIcon` 在 `icon` 为空时不创建图标，需准备一张非空 `.png` 资源 |

#### 2.4.1 R-1 实测证据

写了一个最小验证程序，让 `Timer` 的回调闭包同时捕获三类 `!Send` 值：

- **编译**：**通过**。证明 `Timer::start` 的回调签名确实没有 `Send` 约束。
  > 这与官方文档 *"Timers can only be used in the thread that runs the Slint event loop"* 一致 —— Slint 是用**约定**而不是类型系统来保证线程安全的。因此这条约束必须靠我们自己的代码纪律守住：**Timer 及其捕获的 `Rc` 永远不得离开 UI 线程**。
- **运行**：**通过**。闭包成功改写了 `Rc<RefCell<AppState>>` 与 `Rc<VecModel<SharedString>>`，并排空了 `mpsc` 通道，进程正常退出（exit 0）。
  > 首版 spike 的运行数据曾出现一个疑点：timer 周期设为 50ms、运行约 1 秒却只触发 6 次。6 × 50ms = 300ms，缺失的 700ms 无法用"次数"区分是*启动延迟*还是*被节流*。补测后确认是**事件循环启动延迟**，timer 本身精确（见 §2.4.3）。

#### 2.4.2 R-2 实测证据 —— 含对本文档的修正

**查证结果**：`close-requested` 作为 **Slint 语言**回调**至今未实现** —— [slint-ui/slint#6401](https://github.com/slint-ui/slint/issues/6401) 仍为 open（标签 `priority:low`、`api`）。拦截能力只存在于**语言绑定**侧，且挂在 **`slint::Window`** 上，**不在生成的组件上**。

**正确的 Rust API**（签名已核实，**同样无 `Send` 约束**）：

```rust
pub fn on_close_requested(&self, callback: impl FnMut() -> CloseRequestResponse + 'static)
```

**两个变体的语义经运行期实测确定**（程序化触发 `close()` 后观测 `is_visible()`）：

| 返回值 | `is_visible()` | 语义 |
|---|---|---|
| `CloseRequestResponse::KeepWindowShown` | `true` | **取消**关闭，窗口保持显示 |
| `CloseRequestResponse::HideWindow` | `false` | **接受**关闭，窗口隐藏 |

**对 FR-23 的直接结论**：

1. Slint 的默认关闭行为**本身就是隐藏窗口**。配合 `run_event_loop_until_quit()`，主窗口隐藏后托盘照常存活。
2. 因此 FR-23 **不需要任何自定义代码就已满足**。注册 `on_close_requested` 仅用于附带副作用（写日志、首次关闭时提示"程序仍在托盘运行"）。
3. 若注册，必须返回 **`HideWindow`**。⚠️ 返回 `KeepWindowShown` 会取消关闭 —— 表现是**点 X 按钮毫无反应**，这是个容易搞反且不易察觉的坑。

**附带发现**：`close()` 是 Slint **语言**里 Window 元素的内建函数；**Rust 侧没有 `Window::close()`**。若需程序化触发关闭（例如做自动化测试），必须在 `.slint` 里声明一个回调并调用 `self.close()`（注意是 `self.close()`，不是 `close()`）。

#### 2.4.3 其他实测数据

**编译与依赖**

| 项 | 实测值 |
|---|---|
| Slint 全量依赖冷编译 | **约 2 分钟**（依赖树含 winit / femtovg / accesskit / swash / resvg / fontique 等） |
| 增量编译（仅改 `.slint`） | **4.3 – 8.2 秒** —— 界面迭代速度可接受 |
| `ListView` / `ComboBox` 等 std-widgets | **不是语言内建元素**，必须 `import { ListView } from "std-widgets.slint";`，否则报 `Unknown element 'ListView'` |

**启动延迟与 Timer 精度**（对应 SRS NFR-2 的 1 秒预算）

| 时点 | 实测 |
|---|---|
| `MainWindow::new()` 返回 | **10.5 ms** |
| `show()` 返回 | **12.5 ms** |
| 首次 timer 回调（即事件循环真正开始派发） | **290.8 ms** |

**结论**：

- **NFR-2 的 1 秒预算非常宽裕**。窗口在 **12.5ms** 内就已就绪，远优于要求。
- **事件循环启动有约 278ms 的固定延迟**。这是 Slint/winit 初始化后端与窗口系统的开销，不是我们的代码问题。
- **Timer 精度实测：平均 50.5ms，抖动 ±1.2ms（最小 49.8 / 最大 51.2）**。因此 §1.2 决策 3 选用的 **80ms 排空间隔完全可靠**，日志与状态的显示延迟不会超过约 80ms 量级。
- 该疑点值得记录的原因：若当时误判为"timer 被节流"，可能会去调大排空间隔甚至改用 `invoke_from_event_loop`，都是**基于错误结论的返工**。

---

## 3. Slint ↔ Rust 接口

### 3.1 组件清单

`ui/app.slint` 导出两个顶层组件：

```slint
export component MainWindow inherits Window { ... }
export component AppTray inherits SystemTrayIcon { ... }   // 恰好一个 Menu 子元素
```

**两者不共享 global**（SRS FR-25），因此 Rust 侧必须分别设置属性。`MainWindow` 与 `AppTray` 之间不得有任何 Slint 层的直接引用。

### 3.2 MainWindow 属性契约

```slint
export component MainWindow inherits Window {
    // ---- 版本信息（FR-9）----
    in property <string>  installed-version: "检测中…";
    in property <string>  installed-channel: "";
    in property <string>  latest-version:    "";
    in property <bool>    up-to-date:        false;
    in property <bool>    version-known:     false;

    // ---- 选择器（FR-10、FR-11）----
    in property <[string]> pm-options:       [];   // "npm · dsh 安装于此"
    in property <int>      pm-index:         0;
    in property <[string]> version-options:  [];   // "0.1.6-alpha.2  (alpha)  ← 当前"
    in property <int>      version-index:    0;

    // ---- dsh web（FR-18）----
    in property <bool>     web-running:      false;
    in property <string>   web-url:          "";
    in property <string>   port-text:        "3080";

    // ---- 更新说明（FR-27）----
    in property <styled-text> notes-text:    @markdown("");
    in property <NotesStatus> notes-status:  .loading;

    // ---- 日志与忙碌态（FR-15、FR-28）----
    in property <[string]> log-lines:        [];
    in property <bool>     busy:             false;
    in property <string>   busy-label:       "";
    in property <string>   status-text:      "";

    // ---- 回调 ----
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
    callback link-clicked(string);       // 来自 StyledText，交给系统浏览器
}

export enum NotesStatus { loading, ok, missing, failed }
```

**设计要点**：

1. **下拉列表用格式化字符串而不是自定义 delegate**。`ComboBox` 接受 `[string]`，把通道与"当前"标记直接拼进文案（`"0.1.6-alpha.2  (alpha)  ← 当前"`），省掉一整套自定义控件。SRS FR-10 只要求"标注通道"与"标识当前版本"，格式化字符串已满足。
2. **日志用 `[string]` + `ListView`**，不用"整段文本"属性。原因：日志会持续增长，整段拼接是每次更新 O(n)；`VecModel` 追加是 O(1)，且 `ListView` 自带虚拟化。
3. **`notes-text` 用 `styled-text` 类型**。Slint 的类型映射表中 `styled-text` 对应 `slint::StyledText`，由 `StyledText::from_markdown` 在 Rust 侧构造。

### 3.3 AppTray 属性契约

```slint
export component AppTray inherits SystemTrayIcon {
    in property <bool> web-running: false;
    in property <bool> busy:        false;

    callback show-window();
    callback open-web();
    callback start-web();
    callback stop-web();
    callback quit-app();

    Menu {
        MenuItem { title: "显示主窗口"; activated => { show-window(); } }
        MenuItem { title: "打开 DSH 网页"; enabled: root.web-running && !root.busy;
                   activated => { open-web(); } }
        MenuSeparator { }
        MenuItem { title: "启动 dsh web"; enabled: !root.web-running && !root.busy;
                   activated => { start-web(); } }
        MenuItem { title: "停止 dsh web"; enabled: root.web-running && !root.busy;
                   activated => { stop-web(); } }
        MenuSeparator { }
        MenuItem { title: "退出"; activated => { quit-app(); } }
    }
}
```

**Slint 会在 `enabled` 绑定变化时自动重建平台菜单**，因此不需要手动刷新托盘菜单。

**Windows 平台行为**（Slint 既定行为，不可配置）：左键单击触发 `clicked()`，右键弹出菜单。

### 3.4 状态推送（FR-25 的落地）

```rust
/// 唯一的状态投影点。所有 UI 更新必须经过此函数。
fn project(state: &AppState, win: &MainWindow, tray: &AppTray) {
    // ---- 推给窗口 ----
    win.set_installed_version(state.pm_env.installed.map(|v| v.to_string()).into());
    win.set_installed_channel(state.channel_label().into());
    win.set_latest_version(state.latest_label().into());
    win.set_up_to_date(state.is_up_to_date());
    win.set_pm_options(model_from(&state.pm_labels()));
    win.set_version_options(model_from(&state.version_labels()));
    win.set_web_running(state.web.is_running());
    win.set_web_url(state.web.url().into());
    win.set_notes_text(state.notes.styled());
    win.set_notes_status(state.notes.status());
    win.set_log_lines(state.log.clone().into());
    win.set_busy(state.tx.is_busy());
    // ... 其余省略

    // ---- 推给托盘（独立实例，必须再推一次）----
    tray.set_web_running(state.web.is_running());
    tray.set_busy(state.tx.is_busy());
}
```

**强制约定**：任何修改 `AppState` 的代码路径，都必须在同一次排空结束时调用一次 `project()`。禁止在别处直接设置 Slint 属性 —— 否则窗口与托盘必然会不一致。

---

## 4. 关键模块详细设计

### 4.1 `src/pm.rs` — 包管理器探测

#### 4.1.1 类型

```rust
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Pm { Npm, Pnpm, Bun, Yarn }

impl Pm {
    /// CON-6：Windows 上必须是 shim 全名
    pub fn exe(self) -> &'static str {
        match self {
            Pm::Npm  => "npm.cmd",
            Pm::Pnpm => "pnpm.cmd",
            Pm::Bun  => "bun.exe",
            Pm::Yarn => "yarn.cmd",
        }
    }
    pub fn label(self) -> &'static str { /* "npm" | "pnpm" | "bun" | "yarn" */ }

    pub fn bin_dir_args(self) -> &'static [&'static str] {
        match self {
            Pm::Npm  => &["prefix", "-g"],
            Pm::Pnpm => &["bin", "-g"],
            Pm::Bun  => &["pm", "bin", "-g"],
            Pm::Yarn => &["global", "bin"],
        }
    }

    /// FR-14 命令表
    pub fn install_args(self, version: &str) -> Vec<String> { /* ... */ }
    pub fn uninstall_args(self) -> Vec<String> { /* ... */ }
}

pub struct PmInfo { pub kind: Pm, pub version: String, pub bin_dir: PathBuf }

pub struct PmEnv {
    pub available: Vec<PmInfo>,      // FR-1
    pub owner: Option<Pm>,           // FR-3
    pub installed: Option<Version>,  // FR-4
    pub dsh_path: Option<PathBuf>,
}
```

#### 4.1.2 owner 判定：拆成两个纯函数

```rust
/// dsh 在 Windows 上的可执行 shim 候选名。
/// .exe 用于 bun（bun 在 Windows 生成 .exe shim），.cmd 用于 npm / pnpm / yarn。
const SHIM_NAMES: [&str; 2] = ["dsh.exe", "dsh.cmd"];

/// 在 PATH 目录序列中找出第一个含 dsh shim 的完整路径。
/// 顺序即语义：先命中的就是用户敲 `dsh` 时真正执行的那个。
pub fn find_dsh_on_path(
    path_dirs: &[PathBuf],
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf>;

/// 根据 dsh shim 的所在目录判断 owner PM。
/// 路径比较在 Windows 上必须大小写不敏感。
pub fn owner_of(shim: &Path, bins: &[PmInfo]) -> Option<Pm>;
```

**为什么自己扫 PATH 而不用 `where.exe`**：

| | `where.exe dsh` | 自扫 PATH |
|---|---|---|
| 依赖 | 外部进程 | 无（`std::env::split_paths`） |
| 单元测试 | 需真实环境 | 传入目录数组 + 假 `exists` 闭包即可 |
| PATH 顺序 | 由 where 决定 | 明确可控 |

SRS §8.4 要求 owner 判定有单元测试，自扫方案可直接满足。

**`SHIM_NAMES` 同时含 `.exe` 与 `.cmd` 是必需的**：bun 在 Windows 生成的是 `.exe` shim，只查 `.cmd` 会漏掉 bun。

**路径比较必须大小写不敏感**，且需处理尾部分隔符与 `..`。统一先做 `std::path::absolute` 归一，再按 `eq_ignore_ascii_case` 比较。

#### 4.1.3 通道判定与"通道内最新"（FR-7、FR-8）

```rust
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Channel { Stable, Rc, Alpha, Other }

/// 由 semver 的 pre 字段推导通道
pub fn channel_of(v: &Version) -> Channel {
    if v.pre.is_empty() { return Channel::Stable; }
    let s = v.pre.as_str();
    if s.starts_with("alpha") { Channel::Alpha }
    else if s.starts_with("rc") { Channel::Rc }
    else { Channel::Other }
}

/// FR-8 的核心：只在当前通道内找最新，绝不使用 npm 的 latest tag
pub fn latest_in<'a>(versions: &'a [Version], ch: Channel) -> Option<&'a Version> {
    versions.iter().filter(|v| channel_of(v) == ch).max()
}
```

`semver` 的排序天然正确：`0.1.6-alpha.2 > 0.1.5-rc.2`（先比 patch 再比 pre），正是 SRS §2.2.5 需要的语义。

### 4.2 `src/txn.rs` — 事务引擎

> 这是全项目**唯一有破坏性后果**的模块（SRS §4）。设计目标：算法正确性可在不触碰真实机器的前提下被单元测试穷尽。

#### 4.2.1 类型

```rust
pub struct Origin { pub pm: Pm, pub version: Version }
pub struct Target { pub pm: Pm, pub version: Version }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TxStep {
    Precheck, S1Install, S2Verify, S3Uninstall, S4VerifyFinal,
    C1Probe, C2Restore, C2Cleanup, C3Confirm,
}

pub enum TxOutcome {
    Committed { pm: Pm, version: Version },
    Rejected { reason: RejectReason },
    RolledBack { failed: TxStep, restored: Origin, detail: String },
    Degraded   { failed: TxStep, reason: String, manual: Vec<String> },
}

pub enum RejectReason {
    PmUnavailable(Pm),                        // TR-2
    BinNotOnPath { pm: Pm, dir: PathBuf },    // TR-3
    InvalidVersion(String),                   // NFR-6
    NoOriginInstalled,                        // 未检测到已安装的 dsh
}
```

#### 4.2.2 注入点（唯一使用 trait 的地方）

```rust
/// 事务引擎对操作系统的全部依赖。
/// ⚠ 这里的 trait 不是为了"未来可能有别的实现"，而是 SRS §8.4 强制要求的
///    可测试性：V-17 / V-20 / V-21 三条验证必须能在假 backend 上构造失败路径。
pub trait Backend {
    fn run(&self, pm: Pm, args: &[String]) -> Result<CmdOut, String>;
    fn bin_dir(&self, pm: Pm) -> Result<PathBuf, String>;
    fn path_dirs(&self) -> Vec<PathBuf>;

    /// 直接执行指定目录下的 dsh shim —— 【不走 PATH】(TR-4)
    fn dsh_version_at(&self, dir: &Path) -> Option<Version>;

    /// 经 PATH 解析后执行 —— 仅用于 S4 最终验证
    fn dsh_version_on_path(&self) -> Option<(Pm, Version)>;

    fn log(&self, line: &str);
}
```

**这是全项目唯一的 trait**。其余模块一律用具体函数，不引入抽象。

#### 4.2.3 算法

```rust
pub fn run<B: Backend>(b: &B, origin: Origin, target: Target) -> TxOutcome {
    // ═══ 前置检查：任何一条不通过即拒绝，【零副作用】═══
    if let Some(reason) = precheck(b, &target) {
        return TxOutcome::Rejected { reason };
    }

    // ═══ S1 安装新版本 ═══
    // 顺序关键：先装后卸（TR-6）。最坏结果是"多一份"，不是"一份都没有"。
    if let Err(e) = install(b, target.pm, &target.version) {
        return compensate(b, origin, target, TxStep::S1Install, e);
    }

    // ═══ S2 验证新安装（TR-4：绝不走 PATH）═══
    let target_dir = match b.bin_dir(target.pm) { Ok(d) => d, Err(e) => {
        return compensate(b, origin, target, TxStep::S2Verify, e);
    }};
    if b.dsh_version_at(&target_dir).as_ref() != Some(&target.version) {
        return compensate(b, origin, target, TxStep::S2Verify,
                          "新安装的版本与目标不一致".into());
    }

    // ═══ S3 卸载旧 PM（同 PM 时跳过 —— FR-13）═══
    if target.pm != origin.pm {
        if let Err(e) = uninstall(b, origin.pm) {
            return compensate(b, origin, target, TxStep::S3Uninstall, e);
        }
    }

    // ═══ S4 最终验证（经 PATH）═══
    match b.dsh_version_on_path() {
        Some((pm, v)) if pm == target.pm && v == target.version =>
            TxOutcome::Committed { pm: target.pm, version: target.version },
        other => compensate(b, origin, target, TxStep::S4VerifyFinal,
                            format!("最终验证不符：{other:?}")),
    }
}
```

#### 4.2.4 补偿算法

```rust
fn compensate<B: Backend>(
    b: &B, origin: Origin, target: Target, failed: TxStep, detail: String,
) -> TxOutcome {
    let origin_dir = b.bin_dir(origin.pm).ok();

    // ═══ C1 先探测 origin 是否完好（TR-5）═══
    // 关键：在动任何东西之前先看现状。若 origin 已被 S3 破坏，
    // 盲目卸载 target 会把两边都毁掉。
    let origin_ok = origin_dir.as_ref()
        .and_then(|d| b.dsh_version_at(d))
        .is_some_and(|v| v == origin.version);

    // ═══ C2b origin 损坏 → 先修复 origin ═══
    if !origin_ok {
        if install(b, origin.pm, &origin.version).is_err() {
            return degraded(b, &origin, &target, failed, detail, TxStep::C2Restore);
        }
    }

    // ═══ C2a 清理 target 残留 —— 【先探测再动作】═══
    // 通用规则（本文档 §8 建议增补为 TR-11）：补偿中的每个动作都先确认
    // 目标状态是否存在，避免对不存在的包执行卸载而误判为失败。
    if target.pm != origin.pm {
        let present = b.bin_dir(target.pm).ok()
            .and_then(|d| b.dsh_version_at(&d)).is_some();
        if present && uninstall(b, target.pm).is_err() {
            return degraded(b, &origin, &target, failed, detail, TxStep::C2Cleanup);
        }
    }

    // ═══ C3 确认确实回到了 origin ═══
    let restored = origin_dir.as_ref()
        .and_then(|d| b.dsh_version_at(d))
        .is_some_and(|v| v == origin.version);

    if restored {
        TxOutcome::RolledBack { failed, restored: origin, detail }
    } else {
        degraded(b, &origin, &target, failed, detail, TxStep::C3Confirm)
    }
}
```

**降级报告必须给出可直接复制的命令**：

```rust
fn degraded(...) -> TxOutcome {
    let manual = vec![
        // 重装 origin 版本
        format!("{} {}", origin.pm.exe(), origin.pm.install_args(&origin.version.to_string()).join(" ")),
        // 清理 target 残留
        format!("{} {}", target.pm.exe(), target.pm.uninstall_args().join(" ")),
    ];
    TxOutcome::Degraded { failed, reason, manual }
}
```

#### 4.2.5 前置检查

```rust
fn precheck<B: Backend>(b: &B, target: &Target) -> Option<RejectReason> {
    // TR-2：目标 PM 可用
    if b.run(target.pm, &["--version".into()]).is_err() {
        return Some(RejectReason::PmUnavailable(target.pm));
    }
    // TR-3：目标 PM 的 bin 目录必须在 PATH 中，否则事务完成后 dsh 会从 PATH 消失
    let dir = b.bin_dir(target.pm).ok()?;
    if !b.path_dirs().iter().any(|p| same_dir(p, &dir)) {
        return Some(RejectReason::BinNotOnPath { pm: target.pm, dir });
    }
    // NFR-6：版本号字符集
    if !is_safe_version(&target.version.to_string()) {
        return Some(RejectReason::InvalidVersion(target.version.to_string()));
    }
    None
    // 注：TR-1（dsh web 已停止）由调用方在派发 Job 前处理，
    //     因为"停止确认"需要与 UI 交互，不属于纯逻辑。
}
```

**TR-1 为何不在此处**：停止 `dsh web` 可能需要用户确认，涉及 UI 交互。事务引擎保持纯逻辑、无 UI 依赖。调用方（UI 线程）在派发 `Job::Transact` 前完成停止。

### 4.3 `src/dsh.rs` — 版本、网络与进程监督

#### 4.3.1 目录探测（FR-2 的配合实现）

```rust
/// 在目录中查找 dsh 的可执行 shim
pub fn shim_in(dir: &Path) -> Option<PathBuf> {
    SHIM_NAMES.iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// dsh 的安装根目录（Windows 下 shim 与包目录同名共存）
pub fn is_installed_in(dir: &Path) -> bool { shim_in(dir).is_some() }
```

#### 4.3.2 网络接口

```rust
pub struct Catalog {
    pub versions: Vec<Version>,            // 降序
    pub tags: BTreeMap<String, Version>,   // latest / next / alpha
}

/// FR-6：拉取版本目录
pub fn fetch_catalog() -> Result<Catalog, NetError> {
    // GET https://registry.npmjs.org/@deepseek-ai/dsh
    // Accept: application/vnd.npm.install-v1+json   ← 精简 packument，体积小得多
    // 响应：{ "dist-tags": {...}, "versions": { "0.1.6-alpha.2": {...}, ... } }
    // 只需 versions 的键集合与 dist-tags，因此用 serde_json::Value 解析即可，
    // 不需要 serde derive。
}

/// FR-26：拉取更新说明
pub fn fetch_notes(v: &Version) -> Result<String, NotesError> {
    // GET https://api.github.com/repos/deepseek-ai/deepseek-harness/releases/tags/dsh-v{v}
    // 404 → NotesError::Missing（SRS §2.2.6 的 6 个版本会走到这里）
}

pub enum NotesError { Missing, Net(String) }
```

**超时**：`ureq` 配置全局超时 15s（NFR-3）。**User-Agent 必须设置** —— GitHub API 对无 UA 的请求会返回 403。

#### 4.3.3 更新说明预处理（FR-27 的必要补充）

**问题**：Slint 的 `StyledText` 明确列出的 **Currently Unsupported** 包含 **Headings** 与 **Other HTML tags**。而 DSH 的 release notes：

- 通篇使用 `### 新增功能` / `### 体验优化` 这类 ATX 标题
- 正文混有裸 HTML，如 `<h3 id="cn-v0.1.6-alpha.2">新增功能</h3>`

**不处理的话**，这些会原样显示成 `### 新增功能` 和 `<h3 id="...">` 这样的垃圾文本。

```rust
/// 把 GitHub release body 转成 StyledText 能正确渲染的形式。
/// 只做三件事，不做完整 markdown 解析。
pub fn preprocess_notes(md: &str) -> String {
    // 1. 删除裸 HTML 标签（保留标签内的文本）：
    //    <h3 id="cn-...">新增功能</h3>  →  新增功能
    // 2. ATX 标题降级为粗体：  ### 新增功能  →  **新增功能**
    // 3. 语言导航行 [中文](#cn-..) | [English](#en-..) 保留原样
    //    （锚点在 StyledText 中无法跳转，但保留不影响可读性）
}
```

**明确不做**：不做完整 markdown 解析、不做中英文分离、不做目录生成。`StyledText` 已支持粗体 / 斜体 / 行内代码 / 链接 / 列表，这些直接受益，无需干预。

#### 4.3.4 进程监督

```rust
/// 端口占用探测。stdlib 实现，无依赖。
pub fn port_in_use(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
}

/// 启动 dsh web。返回 pid。子进程的 Child 句柄被移入内部 waiter 线程。
pub fn spawn_web(port: u16, tx: Sender<UiMsg>) -> Result<u32, String> {
    let mut child = Command::new("dsh.cmd")                  // CON-6
        .args(["web", "--port", &port.to_string()])           // 不传 --no-open（FR-16）
        .creation_flags(CREATE_NO_WINDOW)                     // CON-7，0x08000000
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let pid = child.id();
    let out = child.stdout.take();
    let err = child.stderr.take();

    let h1 = spawn_reader(out, tx.clone());
    let h2 = spawn_reader(err, tx.clone());
    thread::spawn(move || {                      // Child 被 move 进来，独占
        let _ = h1.join();
        let _ = h2.join();
        let code = child.wait().ok().and_then(|s| s.code());
        let _ = tx.send(UiMsg::WebExited { code });
    });
    Ok(pid)
}

/// 等待端口就绪。替代方案是解析 stdout，但那依赖 dsh 的输出格式（AS-3）。
pub fn wait_port_ready(port: u16, alive: impl Fn() -> bool,
                       timeout: Duration) -> bool {
    // 每 200ms 探一次 TcpStream::connect_timeout；进程已退出则立即返回 false
}

/// 统一停止入口。自己启动的与外部启动的走同一条路径（见 §1.2 决策 2）。
pub fn stop_by_pid(pid: u32) -> Result<(), String> {
    // taskkill /PID <pid> /T /F
    // /T 终止进程树：Child::kill() 只杀单进程，残留子进程会继续占端口
}

/// FR-19：定位监听指定端口的外部进程
pub fn find_listener_pid(port: u16) -> Result<u32, String> {
    // 解析 `netstat -ano`，匹配 `TCP  127.0.0.1:<port>  ...  LISTENING  <pid>`
}

/// NFR-7：终止前必须校验进程名，避免误杀占用同端口的其他程序
pub fn is_node(pid: u32) -> bool {
    // tasklist /FI "PID eq <pid>" /FO CSV  → 进程名 == "node.exe"
}

/// FR-20：打开默认浏览器。URL 由端口号拼成，不含外部输入。
pub fn open_url(url: &str) -> Result<(), String> {
    // Command::new("cmd").args(["/c", "start", "", url])
    //                                    ↑ 空标题参数，start 处理带引号 URL 时必需
    //   .creation_flags(CREATE_NO_WINDOW)
}
```

**关于 `stdout` 就绪探测 vs 端口探测**：不解析 `dsh web` 的输出文本，因为那依赖 `dsh` 的输出格式（SRS AS-3 已将其列为假设）。TCP 探测只依赖"端口最终会监听"这一稳定事实。

### 4.4 `src/config.rs` — 配置持久化

> 覆盖 SRS FR-30 / FR-31 / FR-32 / §3.8。

#### 4.4.1 内存表示

```rust
/// state.json 的内存表示。
/// 两个字段都是 Option —— "缺失"与"值为默认"必须可区分：
/// 对 FR-31 而言，running_port 为 None 本身就是有意义的信息（没有本程序启动的实例）。
#[derive(Default, Clone, Debug, PartialEq)]
pub struct StateFile {
    pub preferred_port: Option<u16>,
    pub running_port:   Option<u16>,
}

/// 读取结果。刻意不用 Result<StateFile, Error>，理由见 4.4.3。
#[derive(Debug)]
pub enum Loaded {
    Ok(StateFile),
    Missing,            // 文件不存在 —— 首次运行，【静默】
    Corrupt(String),    // 解析失败 —— 记录日志，回落缺省值
    NoLocation,         // APPDATA 不可用 —— 记录日志，功能降级为不持久化
}
```

#### 4.4.2 接口

```rust
/// 解析 state.json 的完整路径。不创建目录，不触碰文件系统。
/// 不引入 dirs crate（§7.2 拒绝清单）。
pub fn state_path() -> Option<PathBuf> {
    // std::env::var_os("APPDATA")?.join("dsh-manager").join("state.json")
}

/// 读取。**任何失败都不返回 Err** —— 失败语义在 Loaded 里表达（FR-32）。
pub fn load() -> Loaded;

/// 原子写入。失败返回 Err 供调用方记日志，但调用方【不得】因此中断主操作（FR-32）。
pub fn save(s: &StateFile) -> Result<(), String>;

/// 读—改—写。单字段更新的便捷入口。
pub fn update(f: impl FnOnce(&mut StateFile)) -> Result<(), String>;
```

#### 4.4.3 为什么用 `Loaded` 枚举而不是 `Result`

"文件不存在"与"文件损坏"对用户的意义**完全不同**：

| 情况 | 含义 | SRS FR-32 要求的行为 |
|---|---|---|
| 文件不存在 | 首次运行，**正常状态** | **静默** —— 绝不能报错 |
| 文件损坏 | 异常 | 记日志 + 回落缺省值 |

用 `Result<StateFile, Error>` 会把两者都压成 `Err`，调用方还得再做一次区分才知道该不该吭声。`Loaded` 枚举让**类型本身编码了 FR-32 的行为表格** —— 调用方的 `match` 分支与需求表格一一对应，不会漏掉静默分支。

#### 4.4.4 原子写入（关键）

`save()` **必须**走"写临时文件 → rename"，**不得**直接截断目标文件：

```
1. 写 <path>.tmp          （与目标同目录，保证同卷）
2. std::fs::rename(<path>.tmp, <path>)
3. 任一步失败 → 清理 .tmp，返回 Err
```

**依据**（这是本模块唯一有讲究的地方）：

直接截断写入时，若进程在写入中途被终止，`state.json` 会变成**半截 JSON**。而 FR-31 要应对的核心场景 —— **管理器异常终止** —— 恰恰就是进程在任意时刻被杀掉的场景。

也就是说：**最需要持久化生效的那个场景，正是朴素写入最容易毁掉数据的场景。** 一旦文件被写坏，下次启动读到损坏文件、回落缺省端口，孤儿就失联了 —— FR-31 在最关键的时刻失效。

原子替换用一个 `rename` 消除了这个窗口。语义已核实：Rust 官方文档明确 `fs::rename` *"replacing the original file if `to` already exists"*，Windows 上通过 `MoveFileExW` 实现，可覆盖已存在文件。

#### 4.4.5 写入点收敛（FR-31 的强制约束在代码上的落点）

`running_port` 的写与清**只允许出现在两个函数内部**：

```rust
// dsh.rs
fn start_web(...) -> ... {
    // ... 端口就绪确认之后：
    let _ = config::update(|s| s.running_port = Some(port));   // ① 唯一写入点
}

fn stop_web(...) -> ... {
    // ... 进程确认终止之后：
    let _ = config::update(|s| s.running_port = None);         // ② 唯一清除点
}
```

**全部停止路径都汇聚到 `stop_web`**：用户手动停止、事务中的临时停止（§4 的 TR-1）、FR-21 的退出停止。因此收敛到这一个函数就覆盖了所有路径。

**注意 `let _ =`**：按 FR-32，持久化失败**不得**使停止/启动操作失败。错误只记日志。

#### 4.4.6 已知限制

**假设单实例运行。** 程序不阻止用户启动第二个实例；两个实例会争用同一个 `state.json`（后写覆盖先写）并出现两个托盘图标。

这不是持久化引入的问题 —— 两个实例本来就无法正确协同管理同一个 `dsh web`。但持久化让"互相覆盖"成为新的表现形态，故记录在此。

**不做单实例守护**（锁文件 / 命名互斥体）：SRS 未要求，且属独立功能。

### 4.5 `src/main.rs` — 入口与 UI 循环

```rust
#![windows_subsystem = "windows"]        // CON-7：不加这行 GUI 背后会出现黑窗口

slint::include_modules!();               // 引入 build.rs 生成的 MainWindow / AppTray

fn main() -> Result<(), Box<dyn Error>> {
    let win  = MainWindow::new()?;
    let tray = AppTray::new()?;          // 托盘图标出现即实例创建 + 事件循环运行

    let (job_tx, job_rx) = mpsc::channel();
    let (msg_tx, msg_rx) = mpsc::channel();
    spawn_worker(job_rx, msg_tx);        // 决策 1：worker 无状态

    let state = Rc::new(RefCell::new(AppState::new()));
    wire_window_callbacks(&win, &job_tx, &state);
    wire_tray_callbacks(&tray, &job_tx, &state, &win);

    // FR-23：关闭主窗口 → 隐藏，不退出。语义见 §2.4.2。
    // ⚠ 两个已实测确认的易错点：
    //   1. on_close_requested 挂在 slint::Window 上（win.window()），
    //      不是挂在生成的组件上（win.on_close_requested 不存在）
    //   2. 必须返回 HideWindow 才是"接受关闭并隐藏"；
    //      返回 KeepWindowShown 会取消关闭 —— 表现是点 X 毫无反应
    {
        let state = state.clone();
        win.window().on_close_requested(move || {
            note_hidden_to_tray(&state);
            CloseRequestResponse::HideWindow
        });
    }

    let timer = Timer::default();        // 必须存活到事件循环结束（决策 3）
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

    let _ = job_tx.send(Job::Probe);
    let _ = job_tx.send(Job::FetchCatalog);
    let _ = job_tx.send(Job::FetchNotes { version: /* 当前版本 */ });

    slint::run_event_loop_until_quit()?;   // FR-23：不是 run_event_loop()
    Ok(())
}
```

### 4.6 `ui/app.slint` 与 `build.rs`

```rust
// build.rs
fn main() {
    slint_build::compile("ui/app.slint").unwrap();
}
```

`ui/app.slint` 内含主窗口与托盘两个顶层组件，以及共享的 `NotesStatus` 枚举与少量样式常量。**不拆分多个 `.slint` 文件** —— 两个组件之间不允许有 Slint 层依赖，拆文件只会增加 `import` 噪音。

⚠️ **`std-widgets` 必须显式导入**（实测确认，见 §2.4.3）。`ListView`、`ComboBox`、`Button`、`ScrollView` 等**都不是语言内建元素**：

```slint
import { Button, ComboBox, ListView, ScrollView, AboutSlint } from "std-widgets.slint";
```

漏掉导入会报 `Unknown element 'ListView'`，且该错误发生在 `build.rs` 阶段，信息指向 `.slint` 行号而非 Rust 代码。

⚠️ **`AboutSlint`** 也在 `std-widgets` 中。它是 SRS CON-8 强制要求署名的组件，必须出现在"关于"对话框里。

---

## 5. 错误处理设计

### 5.1 错误分类

| 类别 | 产生位置 | 呈现方式 |
|---|---|---|
| **可恢复的操作失败** | 网络超时、单个 PM 探测失败 | 日志行 + 对应区域显示短提示，不影响其他功能 |
| **前置检查拒绝** | `RejectReason` | 主界面明确文案 + 引导动作（如"把 X 加入 PATH"） |
| **事务回滚** | `TxOutcome::RolledBack` | 醒目提示"操作失败，已恢复到 X"，并展开日志 |
| **事务降级** | `TxOutcome::Degraded` | **最强提示** + 两条可复制的手动命令 |
| **内部错误** | 通道断开、Slint 平台错误 | 日志 + 状态栏，不 panic |

### 5.2 worker 崩溃检测

`drain()` 在 `msg_rx.try_recv()` 返回 `Disconnected` 时，说明 worker 线程已终止。此时：

- 状态栏显示"后台工作线程已停止，请重启程序"
- 所有触发 Job 的控件禁用（继续发送只会静默失败）

**这是必要的**：否则 UI 会看起来正常但所有操作都无响应，属于最难排查的故障形态。

### 5.3 不引入错误处理库

错误全部以"承载用户可读消息的枚举或 `String`"表示，手写少量 `message()` 方法。不引入 `anyhow` / `thiserror`：

- 错误不需要 `source()` 链 —— 它们直接面向用户展示
- `RejectReason` 需要携带结构化数据（PM、目录路径）用于生成引导文案，`anyhow` 反而不便

---

## 6. 可测试性设计

### 6.1 纯函数切分

SRS §8.4 要求对核心逻辑做单元测试。以下逻辑被刻意设计为**无 I/O 的纯函数**：

| 函数 | 覆盖需求 | 测试方式 |
|---|---|---|
| `channel_of(&Version)` | FR-7 | 表驱动：`0.1.6-alpha.2`→Alpha、`0.1.5-rc.2`→Rc、`0.1.0`→Stable |
| `latest_in(&[Version], Channel)` | FR-8 | **关键用例**：`[0.1.5-rc.2, 0.1.6-alpha.2]` + Alpha → `0.1.6-alpha.2`（防误降级） |
| `find_dsh_on_path(dirs, exists)` | FR-3 | 传入假目录数组 + 假 `exists`，验证取第一个命中 |
| `owner_of(shim, bins)` | FR-3 | 验证大小写不敏感、`dsh.exe` 与 `dsh.cmd` 都能命中 |
| `preprocess_notes(&str)` | FR-27 | 输入真实 release notes 片段，验证 HTML 被剥离、标题变粗体 |
| `is_safe_version(&str)` | NFR-6 | 验证字符集边界 |

### 6.2 事务引擎的失败路径测试

`txn.rs` 的 `Backend` trait 使 SRS §8.3 的三条关键验证可在纯单元测试中完成：

| 测试 | 构造方式 | 断言 |
|---|---|---|
| **V-17** 补偿生效 | 假 backend：(a) `install` 成功 (b) `dsh_version_at(target)` 返回正确版本 (c) `uninstall(origin)` 失败 (d) `dsh_version_at(origin)` 仍返回 V0 | `TxOutcome::RolledBack { failed: S3Uninstall, restored: origin }` |
| **V-20** S1 失败零副作用 | 假 backend：`install` 直接返回 `Err` | `RolledBack`，且 `uninstall` **从未被调用**（用 `Cell<bool>` 记录） |
| **V-21** 不盲目回滚 | 假 backend：`uninstall(origin)` 失败 **且** `dsh_version_at(origin)` 返回损坏状态（`None`）；`install(origin)` 也失败 | `Degraded`，且 `uninstall(target)` **从未被调用** |
| **TR-4** 验证不走 PATH | 假 backend：`dsh_version_at(target_dir)` 返回 `Err`/旧版本，`dsh_version_on_path()` 返回新版本 | 返回 `RolledBack`（证明没用 PATH 的结果） |
| **FR-13** 同 PM 跳过 S3 | `origin.pm == target.pm`，断言 `uninstall` 未被调用 | `Committed` |

### 6.3 测试边界

**不做**：GUI 测试、真实进程启停的自动化测试、网络 mock 框架。这些由 SRS §8.2 的人工清单覆盖。

---

## 7. 依赖选型

### 7.1 采纳的依赖

```toml
[package]
name = "dsh-manager"
version = "0.1.0"
edition = "2024"

[dependencies]
slint        = "1.18"
ureq         = "3.4"
serde_json   = "1"
semver       = "1"

[build-dependencies]
slint-build  = "1.18"

[profile.release]
opt-level = "s"
lto       = true
strip     = true
```

| 依赖 | 用途 | 为什么是它 |
|---|---|---|
| `slint` | GUI | SRS 决策 1；用默认 feature 集（CON-2） |
| `slint-build` | 编译期生成 Rust 绑定 | Slint 标准方式 |
| `ureq` | 阻塞式 HTTPS | **不用 `reqwest`** —— reqwest 会拉入 tokio 运行时，违反 CON-3，且本项目是两个 GET 请求 |
| `serde_json` | 解析 registry 与 GitHub 响应 | 用 `Value` 取值即可，**不引入 `serde` derive** |
| `semver` | 版本解析与比较 | 预发布版本排序必须正确（`0.1.6-alpha.2 > 0.1.5-rc.2`），手写易错 |

**待验证（实现时确认）**：`ureq` 3.4 的默认 TLS 背后端。若默认使用 `rustls` + 内置根证书，则在无企业代理的环境下工作正常；若需系统证书库，需追加 feature。当前环境未配置代理（SRS §2.2.5），预期无问题。

### 7.2 明确拒绝的依赖

| 依赖 | 拒绝理由 |
|---|---|
| `tokio` / `async-std` | CON-3 明令禁止；本项目全部 I/O 可阻塞执行于 worker 线程 |
| `reqwest` | 拉入 tokio，过重 |
| markdown 解析库（`pulldown-cmark` 等） | Slint 内建 `StyledText::from_markdown`（SRS FR-27 明令不得引入） |
| `open` / `webbrowser` | `cmd /c start` 三行即可 |
| `tray-icon` / `winit` | Slint 内建 `SystemTrayIcon` |
| `anyhow` / `thiserror` | 见 §5.3 |
| `dirs` / `directories` | 只需要一个 `%APPDATA%` 路径，`std::env::var_os("APPDATA")` 即可（SRS FR-30 明令不得引入） |
| `serde`（derive） | 读写的都是少数字段，`serde_json::Value` 足够 —— 包括 `state.json` 的两个字段 |
| `windows` crate（Win32 绑定） | 仅需 `CREATE_NO_WINDOW` 一个常量，用 `std::os::windows::process::CommandExt::creation_flags(0x0800_0000)` 即可 |

---

## 8. 对 SRS 的修订记录

本节记录"设计过程中发现需求文档需要修订"的全部条目。**以下 4 项已全部落入 SRS v1.1**，保留在此是为了留痕 —— 每条都说明了*为什么*原表述有问题。

| # | SRS 位置 | 原状 | 已应用的修订 | 应用版本 |
|---|---|---|---|---|
| **1** | §9.1 文件结构 | 列出 7 个文件，`Job`/`UiMsg`/`AppState` 无归属 | 新增 **`src/model.rs`**（共享类型层，被所有模块依赖且不依赖任何模块）；`worker` 循环并入 `src/main.rs` | v1.1 |
| **2** | §4.3 | TR-5 仅要求"补偿前探测 PM_old 完好性" | 增补 **TR-11：补偿中的所有动作必须先探测目标状态再执行**。TR-5 是其特例。依据：若对不存在的包执行卸载，卸载命令的非零退出会被误判为补偿失败，从而错误地报告 `Degraded` | v1.1 |
| **3** | FR-27 | 只说"用 `StyledText::from_markdown` 渲染" | 补充 **`StyledText` 不支持 Headings 与 HTML 标签**（官方 Currently Unsupported 列表），必须先做轻量预处理，否则 `### 新增功能` 与 `<h3 id="...">` 会原样显示 | v1.1 |
| **4** | §5.2 / FR-27 | "禁止任何指向 GitHub 网页的界面链接" | 明确适用范围：**我们自己不生成** GitHub 网页链接。release notes **正文中自带的**链接通过 `link-clicked` 交给系统浏览器处理，不算违反 | v1.1 |

### 8.1 已增补的非功能需求

| # | 内容 | 理由 | 应用版本 |
|---|---|---|---|
| **NFR-11** | 日志缓冲上限 2000 行，超出时丢弃最旧行 | §8.2 的多条验证（V-12/V-14）会长时间运行；`dsh web` 的输出理论上无界，`VecModel` 无上限会持续吃内存 | v1.1 |

### 8.2 由本文档设计阶段反向产生的需求

以下需求不是"修订"，而是**架构设计过程中识别出的缺失能力**，已作为新需求写入 SRS：

| # | SRS 位置 | 内容 | 产生原因 | 应用版本 |
|---|---|---|---|---|
| **1** | §3.8 / FR-30 ~ FR-32 | 配置持久化：偏好端口 + 运行态端口 + 失败降级 | 设计 FR-22（孤儿恢复）时发现：**该恢复机制隐含依赖"知道孤儿在哪个端口"**，而需求原文未提供这个信息的来源。不补则 FR-22 在非默认端口上静默失效（SRS FR-22 已记录该后果） | v1.2 |
| **2** | §9.1 | 新增 `src/config.rs` | 上述需求的 I/O 落点（8 → 9 项） | v1.2 |

---

## 9. 已知风险与遗留问题

### 9.1 实现风险

> **R-1 与 R-2 已于 2026-09-20 通过 spike 关闭**，结论见 §2.4。下表保留完整状态。

| 编号 | 状态 | 风险 | 影响 | 应对 |
|---|---|---|---|---|
| **R-1** | ✅ **已关闭** | ~~`Timer::start` 回调可能要求 `Send`~~ | — | 实测**无 `Send` 约束**，架构决策 3 成立（§2.4.1） |
| **R-2** | ✅ **已关闭** | ~~`close-requested` 的取消语义未验证~~ | — | API 在 `slint::Window` 上；返回 `HideWindow` 即满足 FR-23（§2.4.2） |
| **R-3** | ⬜ 待处理 | `SystemTrayIcon` 的 `icon` 为空时不创建托盘图标 | 中 —— 托盘功能不可用 | 需准备一个非空 `.png` 并 `@image-url` 嵌入 |
| **R-4** | ⬜ 待处理 | `.cmd` 调用触发 Rust BatBadBut 参数转义检查 | 低 —— 版本号是安全字符 | 保守做法：参数用数组传递，不拼接字符串 |
| **R-5** | ⬜ 待处理 | `ureq` 3.4 默认 TLS 后端与根证书来源 | 低 | 实现时确认；必要时追加 feature |
| **R-6** | ⬜ 待处理 | bun / yarn 命令表条目未经实测（本机未安装） | 低 —— 本机不会走到 | SRS §8.4 已列为遗留验证限制 |

**新识别的一条纪律**（源自 R-1 的实测结论）：Slint 用**约定**而非类型系统保证 Timer 的线程安全。因此代码评审时必须人工确认：**`Timer` 及其回调捕获的所有 `Rc` 永不离开 UI 线程**。类型系统不会帮你拦住这个错误。

### 9.2 已决问题

| # | 问题 | 决定 |
|---|---|---|
| **Q-1** | 用户修改的端口是否需要跨重启持久化 | ✅ **做，A + B 都要**（用户决定）：**FR-30** 持久化偏好端口 + **FR-31** 持久化运行态端口。设计见 §4.4 |
| **Q-2** | release notes 中英双语混排是否提供语言切换 | ✅ **不做切换，显示全文**（用户决定） |
| **Q-3** | 是否需要"检查更新"的自动轮询 | ✅ **每次启动查询一次** + 手动刷新（用户决定），不做后台轮询 |

### 9.3 变更记录

| 版本 | 日期 | 说明 |
|---|---|---|
| 1.0 | 2026-09-20 | 首版。基于 SRS v1.0 与全部实测环境数据 |
| 1.1 | 2026-09-20 | 关闭 R-1 / R-2：补充 §2.4 实测证据；**修正 §4.5（原 §4.4）中被证伪的 `on_close_requested` 用法**；补充 std-widgets 导入要求与编译耗时实测；Q-2 / Q-3 结案 |
| 1.2 | 2026-09-20 | **新增配置持久化设计**（Q-1 由"不做"改为"A + B 都做"）：**①** §1.3 模块图新增 `config.rs` 并说明其边界；**②** 新增 **§4.4 `src/config.rs` 详细设计**（`Loaded` 枚举的设计理由、**原子写入及其依据**、写入点收敛约束、单实例限制），原 §4.4 / §4.5 顺延为 §4.5 / §4.6；**③** §7.2 拒绝清单新增 `dirs`；**④** §9.2 Q-1 结案 |
