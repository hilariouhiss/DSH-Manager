# DSH Manager 架构设计与关键模块详细设计

| 项目 | 内容 |
|---|---|
| 文档版本 | 1.10 |
| 日期 | 2026-09-23 |
| 关联文档 | [docs/SRS.md](SRS.md) v1.7（需求依据） |
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
        │                  │                  │              │
        │            ┌─────┴──────┐           │              │
        │            │ plugin.rs  │（v1.7 新增）│              │
        │            │ 插件盘点   │           │              │
        └────────────│ registry 查询│          │              │
                     │ dsh plugin │           │              │
                     │  命令转发  │           │              │
                     └─────┬──────┘           │              │
                           │                  │              │
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

**`plugin.rs` 的依赖方向**（v1.7）：它依赖 `model.rs`（类型）、`pm.rs`（`run_cmd`，带
`CREATE_NO_WINDOW`）、`dsh.rs`（`agent()` —— 因此自动继承 FR-33 的系统代理）。
**没有任何模块依赖它**，只有 `main.rs` 的 worker 调它 —— 与 `config.rs` 同为叶子。

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
用户选中某版本 ──► Job::FetchNotes ──► parse_notes_blocks ──► [NoteBlock] ──► 逐块 StyledText
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
| `ListView` / `ScrollView` 等 std-widgets | **不是语言内建元素**，必须 `import { ListView } from "std-widgets.slint";`，否则报 `Unknown element 'ListView'`。输入框与下拉框自 v1.3 起不再用 std-widgets（见 §2.4.3） |

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

> ⚠ v1.7 按**实现的实际形状**重写。原块是动工前的契约，早已与代码不符：`pm-options` /
> `pm-index` / `pm-is-owner` / `version-is-current` / `latest-version` / `installed-channel` /
> `hide-to-tray` / `about-clicked` 都已在 v1.3~v1.7 中被删除，而主题、代理、关闭行为、
> 重装确认、插件卡这 5 组从未写进来过。

```slint
export component MainWindow inherits Window {
    // ---- 版本信息（FR-9 修订）----
    in property <string>  installed-version: "检测中…";   // 哨兵："检测中…" / "未检测到 dsh"
    in property <bool>    version-known:     false;       // 探测完成 && 已装 && 通道内最新已知
    in property <bool>    up-to-date:        false;       // 操作卡右端的徽标只看这一件事
    // ⚠ `latest-version` 属性已随左栏版本主卡删除：目标版本下拉的默认选中项就是通道最新版。

    // ---- 选择器（FR-10 / FR-11 修订）----
    in property <[string]> version-options:  [];          // 裸版本号，降序
    in-out property <int>  version-index:    0;
    in property <string>   pm-label:         "检测中…";   // owner PM 的**只读指示**（原下拉框）

    // ---- dsh web（FR-18）----
    in property <bool>     web-running:      false;
    in property <bool>     web-starting:     false;       // I-2：Starting 期间 running 仍是 false
    in property <string>   web-url:          "";
    in-out property <string> port-text:      "3080";

    // ---- 更新说明（FR-27）----
    in property <[NoteBlock]> notes-blocks:  [];
    in property <NotesStatus> notes-status:  .loading;

    // ---- 插件卡（FR-34）----
    in property <[PluginRow]> plugin-rows:   [];
    in property <string>   plugin-summary:      "";       // 卡头 trailing：汇总
    in property <string>   plugin-status-line:  "";       // 卡底左侧：状态（两处各管一件事）
    in property <bool>     plugins-loading:     false;
    in property <int>      plugin-updatable-count: 0;
    in property <length>   plugin-card-height:  164px;    // Rust 按行数算，上限 6 行（§4.8.6）
    in-out property <string> plugin-input:      "";       // 安装输入框
    in-out property <bool>   plugin-remove-prompt-visible: false;
    in property <string>     plugin-remove-name:    "";
    in property <string>     plugin-remove-version: "";

    // ---- 本程序自身的更新（FR-38 / FR-39）----
    in property <bool>   update-available:      false;  // 驱动顶栏徽标与弹框
    in property <string> update-version:        "";     // 形如 "0.3.0"（Rust 填好，Slint 不解析）
    in-out property <bool> update-prompt-visible: false; // ⚠ 唯一写入方是 Rust，见 §4.9.5

    // ---- 日志与忙碌态（FR-15、FR-28）----
    in property <[string]> log-lines:        [];
    in property <bool>     busy:             false;       // 版本事务与插件操作**共用**这一道闸门
    in property <string>   busy-label:       "";
    in property <string>   status-text:      "";

    // ---- 关于（FR-29）与设置 ----
    in property <string>   app-version: "";  in property <string> dsh-path: "";
    in property <string>   owner-pm: "";     in property <string> current-port: "";
    in-out property <int>  close-behavior-index: 2;   // 与 config::CloseBehavior 下标一一对应
    in-out property <int>  theme-mode: 2;             // 与 theme::ThemeMode 下标一一对应
    in property <bool>     use-system-proxy: true;    // 只描述界面选中态（决策在 dsh::agent）

    // ---- 纯窗口本地 UI 状态（不进 AppState）----
    in-out property <bool> about-visible: false;
    in-out property <bool> settings-visible: false;
    in-out property <bool> close-prompt-visible: false;
    in-out property <bool> close-prompt-remember: true;
    in-out property <bool> reinstall-prompt-visible: false;   // FR-12b
    in property <string>   reinstall-version: "";
    in-out property <int>  open-menu: 0;                      // 0=无 / 1=文件 / 2=帮助

    // ---- 回调 ----
    callback install-clicked();          // 目标版本 == 已装版本时由 Rust 改成弹重装框
    callback version-changed(int);
    callback refresh-clicked();          // Probe + FetchCatalog + FetchPlugins
    callback start-web();   callback stop-web();   callback open-web();
    callback port-changed(string);
    callback settings-changed(int);      // 关闭行为
    callback theme-mode-changed(int);
    callback proxy-toggled(bool);
    callback close-choice(int, bool);    // (行为下标, 是否记住)
    callback reinstall-confirmed();      // FR-12b 的〔重装〕
    callback plugin-install();           // FR-35：读 plugin-input，Rust 侧校验
    callback plugin-update(int);         // FR-36：行下标
    callback plugin-update-all();
    callback plugin-remove(int);         // FR-37：行下标 → 只弹确认框
    callback plugin-remove-confirmed();  // FR-37：确认框里的〔卸载〕→ 才派发
    callback plugin-refresh();
    callback quit-app();
    // 本程序自身的更新（FR-38 / FR-39）
    callback update-badge-clicked();     // 顶栏徽标 → 把弹框叫回来
    callback check-update-clicked();     // 帮助 →〔检查更新〕（手动档）
    callback update-later-clicked();     // 〔以后再说〕/遮罩/Esc/X：只关框
    callback update-skip-clicked();      // 〔忽略此版本〕：写进 state.json
    callback update-download-clicked();  // 〔下载并安装〕
    callback link-clicked(string);       // 来自 StyledText；只有绝对 http(s) 的交出去
                                         // （main::dispatch_notes_link，见 FR-27b：
                                         //  站内锚点 / 相对路径不派发、不报错）
}

// ⚠ `PluginRow` 的**字段全是格式化好的字符串**（可更新与否、显示"未安装"还是"—"都由
// Rust 侧算完）—— 与 `version-options` 同款约定：Slint 不做判断，也就不可能与状态分叉。
export struct PluginRow {
    name: string, spec: string, installed: string, latest: string,
    updatable: bool, missing: bool,
}

export enum NotesStatus { loading, ok, missing, failed }
```

**设计要点**：

1. **下拉列表用格式化字符串而不是自定义 delegate**。下拉（v1.3 起为 `GlassSelect`，此前是 `ComboBox`）接受 `[string]`，把通道与"当前"标记直接拼进文案（`"0.1.6-alpha.2  (alpha)  ← 当前"`），省掉一整套自定义 delegate。SRS FR-10 只要求"标注通道"与"标识当前版本"，格式化字符串已满足。⚠️ v1.3 后通道后缀与"← 当前"已按用户要求移到行尾/标题，本条只保留"不写自定义 delegate"这一半。
2. **日志用 `[string]` + `ListView`**，不用"整段文本"属性。原因：日志会持续增长，整段拼接是每次更新 O(n)；`VecModel` 追加是 O(1)，且 `ListView` 自带虚拟化。
   **每行是 `read-only: true` 的 `TextInput`（v1.10 改，SRS v1.7 的 FR-28 修订）**：
   Slint 的 `Text` **完全没有选区**，`TextInput` 才有；`read-only` 的官方语义就是
   "能选不能改"。三处必须一起记住：
   - 左键按下即 `GrabMouse`（`i-slint-core items/text.rs` 的 `input_event`）⇒ 拖选能拿到鼠标，
     但**日志区按住拖动不再滚动**（滚轮 / 滚动条 / `PageUp` 照旧）；
   - `Ctrl+A` / `Ctrl+C` 不受 `read-only` 影响（被它挡掉的只有 Paste/Cut/Undo/Redo），
     剪贴板由 Slint 自己写 ⇒ **本功能零 Rust、零依赖**；
   - `accessible-role: text` 不能省：`Text` 的默认角色是 `text`，`TextInput` 的默认角色是
     `text-input` —— 不写它，读屏会把每行日志当成一个可编辑输入框。
3. **`notes-blocks` 是 `[NoteBlock]`，每块自带 kind 与一段 `styled-text`**（v1.4 改）。
   原因：Slint 的 `StyledText` 官方 Currently Unsupported 列表里有 **Headings**，且它
   **没有字重属性、只有一个字号** —— 单段文字表达不了"标题比正文大"；列表也只是渲染成
   行内的 `• ` 前缀，换行后第二行退回左边缘（没有悬挂缩进）。所以**块结构在 Rust 侧
   （`dsh::parse_notes_blocks`）解析**，Slint 侧按 kind 分别排版（`NoteBlockView`）。
   块内的**行内** markdown 仍交给 `StyledText::from_markdown` 逐块解析（粗体 / 链接 /
   行内代码它原生支持）。⚠ `NoteBlock.kind` 的下标与 `dsh::NoteKind` 一一对应。

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
    win.set_notes_blocks(model_from(&state.note_blocks()));
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
/// 把 GitHub release body 切成"标题 / 列表项 / 段落"三种块。
/// 只做结构识别，不做排版（字号 / 缩进 / 块间距由 Slint 侧的设计令牌决定）。
pub fn parse_notes_blocks(md: &str) -> Vec<NoteBlock> {
    // 1. 删除裸 HTML 标签（保留标签内的文本）：
    //    <h3 id="cn-...">新增功能</h3>  →  NoteBlock { Heading, "**新增功能**" }
    // 2. ATX 标题同样识别：  ### 新增功能  →  NoteBlock { Heading, ... }
    // 3. 无序列表去掉标记本身：  - 某条目  →  NoteBlock { Bullet, "某条目" }
    //    （圆点交给 Slint 画，才做得出悬挂缩进）
    // 4. 其余行按空行分段：空行才是段落边界，连续行留在同一个段落（markdown 软换行）
    // 5. 语言导航行 [中文](#cn-..) | [English](#en-..) 保留原样，作为普通段落
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

/// FR-20：打开默认浏览器。
///
/// ⚠ 这里早先画的是 `cmd /c start`，已被 Ruling 65 换掉：URL 来自**网络**
/// （release notes 正文里的链接），属信任边界，而 Rust 的参数编码**不**转义
/// cmd 的元字符（`& | ^ < > % !`）。现在是 `explorer.exe` + 单参数（无 shell）
/// 加协议白名单 `dsh::is_web_url` —— 非 http(s) 在 `spawn` 之前就返回 `Err`。
pub fn open_url(url: &str) -> Result<(), String> {
    if !is_web_url(url) {
        return Err(format!("拒绝打开非 http(s) 链接: {url}"));
    }
    // Command::new("explorer.exe").arg(url)     ← GC-7：全名；单个参数没有 shell
    //   .creation_flags(CREATE_NO_WINDOW)       ← GC-8
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

⚠️ **`std-widgets` 必须显式导入**（实测确认，见 §2.4.3）。`ListView`、`ScrollView`、`AboutSlint`、`Palette` 等**都不是语言内建元素**：

```slint
import { ListView, ScrollView, AboutSlint, Palette } from "std-widgets.slint";
```

⚠️ **输入框与下拉框自 v1.3 起不再来自 `std-widgets`**。原用的 Fluent `LineEdit` / `ComboBox`
把 `min-width: max(160px, …)`、`min-height: max(32px, …)`、3–4px 圆角、自带箭头与浅色底
**写死在组件体里**（`i-slint-compiler-1.18.0/widgets/fluent/{lineedit,combobox}.slint`），
而 fluent 的 `Palette` 属性全是 `out`（只读）——**从外部改不动**，此前只能靠外框遮挡，
代价是"外圆角 11px 套内圆角 4px"的双弧线与两处 `min-width: 0px; height: 30px;` 压制 hack。
现改为自绘的 `GlassField` / `GlassSelect`（`TextInput` 起壳 / `PopupWindow` 弹层），
沿用同一套设计令牌。`ListView` / `ScrollView` **保留**：虚拟化不能丢。

漏掉导入会报 `Unknown element 'ListView'`，且该错误发生在 `build.rs` 阶段，信息指向 `.slint` 行号而非 Rust 代码。

⚠️ **`AboutSlint`** 也在 `std-widgets` 中。它是 SRS CON-8 强制要求署名的组件，必须出现在"关于"对话框里。

---

### 4.7 主题子系统（`src/theme.rs` + `ui/app.slint` 的 `Tokens`）

规格：`docs/superpowers/specs/2026-09-22-theme-system-design.md`。平台事实与设计后果见
`docs/RULINGS.md` 的「主题系统的平台事实」一节；本轮实测见 `docs/VERIFICATION.md`。

#### 4.7.1 职责边界

`src/theme.rs` **只做三件事**，且都在 Rust 侧：

| 项 | 职责 |
|---|---|
| `ThemeMode` + `index`/`from_index`/`as_str`/`parse` | 三档（浅色/深色/跟随系统）与它们在 UI、`state.json` 里的表示之间的双向映射 |
| `resolve(mode, system_dark) -> bool` | **主题的唯一判据**：把"用户选的档"与"系统当前是不是暗色"合成"这一帧该不该用暗色" |
| `system_dark()` / `spawn_watcher(tx)` | 系统态探测与变更监视（见 §4.7.4） |

它**不**知道任何颜色：调色板是 `ui/app.slint` 里 `global Tokens` 的 40 条 brush 令牌，每条写成
`dark ? #暗 : #浅` 的形状（`out property <brush> 名: dark ? #AAAAAA : #BBBBBB;`）。
`Tokens` 是普通 Slint 全局，`dark` 由 Rust 的 `set_dark()` 写。

#### 4.7.2 Rust 是唯一真相源，Slint 侧有**两条**投影路径

真相只有一处：`AppState.theme_mode`（用户选择）+ `AppState.system_dark`（系统态），
经 `resolve()` 得到布尔值。`project()` 每帧把它推给两个目标（`src/main.rs`）：

```rust
win.set_theme_mode(state.theme_mode.index());                              // ① 选中态
win.global::<Tokens>().set_dark(theme::resolve(state.theme_mode, state.system_dark)); // ② 颜色
```

| 路径 | 目标 | 覆盖谁 |
|---|---|---|
| ① `theme-mode` (`<int>`) | 设置面板的 ChipButton 选中态 | 本程序自己的控件 |
| ② `Tokens.dark` (`<bool>`) | 40 条 brush 令牌的取值 | 本程序自己的控件 |
| ③ `Palette.color-scheme` | `std-widgets` 的 Fluent 控件（`ScrollView` 滚动条、`AboutSlint`…） | **不由 Rust 推**，见下 |

**为什么②与③必须是两条**：③ 的目标 `Palette` 是 `std-widgets` 的全局，
其实现是 `Palette.color-scheme <=> FluentPalette.color-scheme`，而 `FluentPalette` **没有**对用户代码
可用的 Rust 访问器 —— 生成的 `MainWindow` 上不存在 `FluentPalette` 这个 global，
`SlintInternal.color-scheme` 更是编译期就被拒绝（见 RULINGS）。于是③只能在 `.slint` 内部完成：

```slint
property <bool> is-dark: Tokens.dark;                 // 局部镜像，绑定是活的
changed is-dark => {                                  // ← 运行期同步**只**靠这一条
    Palette.color-scheme = is-dark ? ColorScheme.dark : ColorScheme.light;
}
init => { /* 只兜构造那一刻 */ }
```

⚠ **`init` 那一行不构成同步**：它在 `MainWindow::new()` 里跑，早于 `main()` 的第一次 `set_dark()`，
那时 `Tokens.dark` 还是声明缺省值 `true` —— 所以它**永远**写 dark。浅色档的 Fluent 配色全部依赖
`changed` 处理器在之后的 `set_dark()` 上真的触发。（离屏探针实测确认它确实触发，见 VERIFICATION。
这条曾经只是"看起来对"，是本子系统里唯一一条必须先测后信的接线。）

因此**不得**从 Rust 侧另找路子写 `Palette`，也**不得**删掉 `changed` 那一支 —— 两者都会让
"本程序的颜色"与"Fluent 控件的颜色"分叉。

#### 4.7.3 `system_dark` 刻意不进 `.slint`

`system_dark`（"系统当前是不是暗色"）**没有**任何 `.slint` 属性与之对应，这是刻意的：

- 一旦把它也做成 Slint 属性，就会有两个地方能判定主题（Rust 的 `resolve()` 与 Slint 里的某个表达式），
  而**强制档下系统态变化不该改变结果**（选了"深色"就该一直是深色）—— 这种"哪个说了算"的分歧
  正是本系统要避免的双真相源。
- `resolve()` 的六格真值表已由单测钉住（`resolve_truth_table`），Slint 侧只消费它的**结果**。

**下一个人不要"顺手"把它加上去。** 需要新语义时改 `resolve()`，不是在 UI 属性上加一条。

#### 4.7.4 `UiMsg::SystemThemeChanged` 与 80 ms 排空

监视在 **Task 6** 落地，链路是：

```
uxtheme 序号 132 (system_dark)          ← 与 winit 同源，决定标题栏与主体一致
  ↑ 读值
spawn_watcher 线程                       ← 先 RegNotifyChangeKeyValue(REG_NOTIFY_CHANGE_LAST_SET)
  │                                        武装（异步：立即返回，**不阻塞**，`src/theme.rs:283`）
  │ 先武装、再读值（顺序反了会永久漏掉落在窗口里的那次变更）
  ↓ tx.send(UiMsg::SystemThemeChanged(bool))
无界 mpsc 通道
  ↓
80 ms Timer → drain()
  │ SystemThemeChanged(dark) 臂：s.system_dark = dark;   ← 只记事实，不算 was/now
  │   （不置 dirty、不改 changed：drain 进来时已 changed=true，置 dirty 只会多跑一次 project）
  ↓
drain() 返回 true → 全量 project() → set_dark(resolve(...))
```

**与 80 ms 的关系**：监视线程只负责把"系统态变了"这件事**投递**出去，不直接碰 UI ——
Slint 的属性只能在 UI 线程写。所有 `UiMsg` 都由 UI 线程上那个 80 ms `Timer` 一次性排空
（架构决策 3），于是：

- 同一次系统主题切换引发的多条消息（以及其它消息）被**合并成一帧**；
- 监视线程本身在变更之间阻塞在 `WaitForSingleObject(INFINITE)` 上，**不轮询**（NFR-4 要求空闲近零 CPU）；
- ⚠ **没有"消息幂等 ⇒ 不重绘"这回事**：`drain()` 对**每一条**收到的消息都置 `changed = true`，
  所以重复的 `SystemThemeChanged(同一个值)`（以及强制档下 resolved 不变的那种）**照样**触发一帧全量
  `project()`。`SystemThemeChanged` 臂只写 `s.system_dark`，它既不置 `dirty`、也不动 `changed`。
  真正的省帧发生在别处：稳态下没有消息，`drain` 无输出且 `take_dirty` 为假，于是根本不 `project()`。

⚠ **代价（已知并接受）**：监视线程为进程生命周期持有一个 `Sender<UiMsg>`，
于是 `msg_rx.try_recv()` 再也不会返回 `Disconnected` —— `drain()` 里 §5.2 那条"worker 已死"兜底
与 `worker_dead` 闩锁因此变成**条件可达**：只有监视线程自己 panic，或它的**两条非 panic 早退路径**
（`CreateEventW` 失败 `src/theme.rs:240`、`WaitForSingleObject` 返回非 `WAIT_OBJECT_0`
`src/theme.rs:307`）丢弃了那个 sender 时，才重新可达。
真正的崩溃保护是 worker 的整圈 `catch_unwind` + 显式 `Log`/`Failed`，不受影响。
（同一组事实另见 `docs/RULINGS.md` 的 §5.2 可达性变化与 `src/main.rs:1494-1496`。）

#### 4.7.5 继承来的既有约束

- **`Tokens` 的令牌数有下限测试**（`MIN_BRUSH_TOKENS = 40`）：`parse_palette` 只认带 `<brush>` 的行，
  整行被删会静默失去对比度覆盖，故以 `>=` 下限兜住"丢令牌"。
- **两套调色板都要过对比度门槛**（`both_palettes_meet_text_contrast_bars` /
  `translucent_tiers_stay_visible_in_both_palettes`）：暗色侧不改，浅色侧按规格 §4.3 的权威值表。
- **强制明/暗改不动系统标题栏**：标题栏由 winit 按**系统**主题绘制，本程序无公开 API 覆盖。
  这是平台限制，已在设置面板里向用户交代。

---

### 4.8 `src/plugin.rs` — 第三方插件管理（v1.7 新增）

规格：`docs/superpowers/specs/2026-09-22-plugin-manager-design.md`。需求：SRS v1.4 的 §3.9（FR-34 ~ FR-37b）与 NFR-12。

#### 4.8.1 职责边界

`plugin.rs` **只做四件事**：

| 项 | 职责 |
|---|---|
| `profile_dir_from` / `profile_dir` | 定位 `%DSH_HOME%\profiles\web`（`DSH_HOME` 缺省 `%USERPROFILE%\.dsh`） |
| `valid_spec` | 规格白名单（**信任边界**） |
| `read_installed` / `parse_latest` / `fetch_latest` / `fill_latest` | 盘点已装 + 查 registry 最新版 |
| `update_all_op` / `run_plugin`(+`_with`) / `fetch_all` | 汇总更新操作、转发 `dsh plugin` |

它**不含 UI 逻辑，也不含"何时该拉取"的决策**（与 `config.rs` 同款边界）—— 那是 `main.rs` 的 worker 的事。

#### 4.8.2 为什么是"转发 `dsh plugin`"而不是"直接调 pnpm"

`dsh plugin` 会做三件本项目不该重新实现的事（spec F1~F3，对着
`@deepseek-ai/dsh-plugin-manager` 的 `operations.js` 核实）：

1. 持 **profile 写锁**（锁文件就是 profile 的 `package.json`）—— 运行中的 `dsh web`
   的服务用的是**同一把**锁，所以转发天然与它互斥；
2. 退出码 0 后跑 **reconcile**，把新装的依赖补进 `dsh.profile.bundles`、把已移除的清掉
   —— 直接调 pnpm 会让卸载后的 bundle 列表留下悬挂项；
3. 给出 `allowBuilds` / `minimumReleaseAge` 这类 pnpm 专属提示的原文。

因此 **CON-5 的形态**是"变更一律经 `dsh plugin` 转发、只读两个 JSON 文件"，
而不是 v1.3 时的"不得以任何方式干预"。

#### 4.8.3 为什么"不停 `dsh web`"（推翻设计初稿的一条）

设计初稿要求"变更前先停 `dsh web` + 弹窗确认"。**该前提经实测被证伪**：

| 初稿的理由 | 实测 |
|---|---|
| profile 里有原生模块（node-pty 等），运行中的进程会锁住 `.node` 文件 | ❌ `find node_modules -name "*.node"` 在 web profile 里**一个都没有**；`allowBuilds` 里那三个名字是 DSH 写进模板的**白名单**，不是"已安装" |
| — | `dsh plugin` 与运行中的服务**共用同一把写锁**；README 写明 CLI 改完之后由 HMR 监视器应用（`patchReload: live`）⇒ **不停服务是既定路径**，DSH 自带的 Web 插件页本身就跑在运行中的服务器里 |
| — | 而"停 → 重启"的**真实代价**是杀掉用户正在跑的会话 |

结论：不停服务、不弹窗，`Job::Transact` 的 TR-1 路径**逐字不动**。
（这笔账完整记在 spec §13 的变更记录里，因为它演示了"一个未经核实的平台假设会怎样改变设计"。）

#### 4.8.4 为什么这里用 `latest` tag 是对的（不要照抄 FR-8）

**FR-8 禁止把 `latest` 当"最新"是针对 dsh 自身**：它有 alpha / rc / stable 三条通道，
`dist-tags.latest`（0.1.5-rc.2）实测比已装的 alpha 版（0.1.6-alpha.2）**更旧**（SRS §2.2.5）。
**第三方插件没有通道概念** —— `dist-tags.latest` 就是它语义正确的最新版。
这条写两遍（代码注释 + 这里）是因为"下一个人顺手把它改成通道内最新"是最可能发生的退化。

#### 4.8.5 并行查询与失败隔离

`fill_latest` 用 `std::thread::scope` 每包一线程（NFR-12）：本机单次 registry 请求
实测 4.1 s，6 个串行 ≈ 25 s。单包失败（超时 / 404 / 版本号非法）只让那一行显示"—"
（`join().unwrap_or(None)` 连线程 panic 也一并隔离）。

#### 4.8.6 UI 侧的两条接缝

- **卡高由 Rust 算**（`plugin_card_height(rows)`，上限 6 行）：Slint 里 `ListView`
  没有可读的内容高度，而用 `ScrollView` 包它会破坏虚拟化（日志区已记过这条坑）。
  ⚠ 常量 `ROW` 必须与 `ui/app.slint` 的 `PluginRowView.height` **同一个数** ——
  探针实测过 46 vs 40 的差别：多算 6px/行会让 `ListView` 比内容高 36px，最后一行与
  汇总行之间凭空多出 63px 空白。
- **不可见元素照样占位**：行内的「已是最新」/「最新版未知」两句用 `visible:`（尾随位置，
  留白无害），而〔更新〕按钮与操作卡右端的状态徽标用 `if`（它们贴着右缘/会挤压邻项）。

---

### 4.9 本程序自身的更新（FR-38 / FR-39，v1.8 新增）

#### 4.9.1 没有新模块

整套更新只用了既有的四层：`dsh.rs`（网络与子进程）、`model.rs`（`NewRelease` 与两个
`Job` / 两条 `UiMsg`）、`config.rs`（一栏"忽略过的版本"）、`main.rs`（编排）。
**不新增文件、不新增依赖** —— 与 FR-33（系统代理）同一取舍：能力是"已有的东西换一种用法"，
而不是新机制。`certutil.exe` 就是这一取舍的样板：算一次 SHA256 的活，够不上为它引一个
`sha2`（GC-2 的精神）。

#### 4.9.2 名字是契约：`OutputBaseFilename` ↔ 资产名 ↔ 更新器

安装包的名字**同时**是发布流水线的产物名与更新器的查找键：

```text
installer/dsh-manager.iss : OutputBaseFilename=dsh-manager-v{#AppVersion}-windows-x64-setup
.github/workflows/release.yml : 上传的资产名（gh 的资产名 = 文件名，`#label` 改不了它）
src/dsh.rs : app_setup_asset_name(&Version)  ← 唯一的构造点
```

三处必须逐字一致。任一处改名，自动更新就**静默失效**（查得到新版本、找不到要下载的东西）。
所以：① 名字只在 `app_setup_asset_name` 里拼；② 名字对不上时 `parse_app_release` 返回
**`Err`** 而不是 `Ok(None)` —— 报成"已是最新"会让这个故障**永远隐形**（用户只会以为
"最近没发新版"）。

⚠ 这条在真实数据上立刻见效：v0.2.1 是用**旧名字**（`dsh-manager-0.2.1-setup.exe`）发布的，
所以拿 `local = 0.1.0` 去查会得到
`Err: release v0.2.1 里没有资产 dsh-manager-v0.2.1-windows-x64-setup.exe` —— 正是想要的
响亮失败，而不是"已是最新"。

#### 4.9.3 数据流

```text
启动 / 帮助→检查更新
   Job::CheckAppUpdate { manual }        ← manual 只影响"失败去哪儿说"
        ↓ worker
   fetch_app_release(&local_version())   ← 15 s 超时 + FR-33 代理
        ↓
   UiMsg::AppUpdate { manual, result }   ← Ok(Some/None) / Err 三分
        ↓ drain（UI 线程）
   AppState.app_update / update_prompt_visible / 日志 / 状态栏
        ↓ project()
   徽标（update-available + update-version）与弹框（update-prompt-visible）

〔下载并安装〕
   Job::DownloadAppUpdate { release }    ← 载荷是**已解析**的 release，worker 不重查
        ↓ worker：下载安装包 → 取 SHA256SUMS.txt → 落盘 %TEMP% → 校验 → launch_setup
   UiMsg::AppUpdateLaunched { version, path }
        ↓ drain：置 AppState.quit_keep_web = true
        ↓ 80 ms timer：take(quit_keep_web) → make_quit(false) → quit_event_loop
```

**为什么"置标志 + timer 取走"而不是直接退出**：置真发生在 `drain`，而退出闭包由 `main()`
持有 —— `drain` 的签名里没有它（只有 `state` 与 `job_tx`）。80 ms 的 timer 是唯一同时
够得着"排空消息"与"持有闭包"的地方，于是退出请求经过一次显式的交接。代价是多一个
`AppState` 字段，收益是 `drain` 不必为了退出而改变签名（那会牵动它的三条既有调用路径）。

#### 4.9.4 退出语义分叉：`make_quit(stop_web)`

FR-21 要求"退出先停掉本程序启动的 `dsh web`"，而 FR-39 的更新退出**必须不停**（用户可能
正开着网页用那个会话）。实现是把原来那个内联闭包提成
`make_quit(state, job_tx, stop_web) -> Rc<dyn Fn()>`，两条路径各持一个实例：

| 入口 | `stop_web` | 行为 |
|---|---|---|
| 托盘"退出"、文件→退出、关闭时选"彻底退出" | `true` | 停 `dsh web` → 清 `running_port` → 退出（FR-21） |
| FR-39 装更新 | `false` | **不动** `dsh web` → 退出。`running_port` 早在启动时就已落盘（FR-31），重启后按 FR-22 认成"外部 dsh web"，用户可继续用或自行停止 |

⚠ **共用同一个函数而不是各写一遍**：`stop_by_pid` 前后那几段注释里的坑（"确知已停才清
`running_port`"、"队列里还有 `StartWeb` 时要记端口"）对两种退出**同样成立** —— 复制一份
必然只修其中一份。两条路径只差 `match (stop_web, pid)` 的第一个分支。

⚠ 定义位置被这次改动**上移**到了 timer 之前：更新那条路由 worker 消息触发，而 timer 需要
拿到这个闭包。原先"quit 在 timer 之后定义"的注释因此失效，已随代码改写。

#### 4.9.5 弹框可见性：唯一写入方是 Rust

`update-prompt-visible` 由 Rust 独占写入，Slint 侧**只调回调**（连点遮罩、Esc、右上角 X
也走 `update-later-clicked()`）。这与 `settings-visible` / `about-visible` 那类"纯 UI 状态"
的做法**不同**，多出来的是那个"关闭"回调，原因是：

> `project()` 是**全量投影**，只要有任何状态变化就会把 `AppState` 里的值推给界面。
> 若 Slint 自己把属性置假，`AppState` 里仍是 `true` —— 下一次任何状态变化（日志多一行、
> 状态栏换句话）都会把它**推回来**，表现为"关不掉的弹框"。多一条回调换掉这个 bug 类，
> 划算。

同理，"自动弹一次"与"忽略过就不自动弹"这两个判断都在 `drain` 里做（`update_auto_shown`
`skipped_app_version`），Slint 侧一个条件都不写。

#### 4.9.6 顶栏徽标：`alignment: start` 里没有 stretch

菜单栏那条 `HorizontalLayout` 必须 `alignment: start`（否则两个菜单标题会被摊成两根长条，
v1.7 已记）。而 `start`/`center`/`end` 下**子项的 `horizontal-stretch` 不生效** —— 所以
"加一根占位条把徽标顶到右边"是错的：徽标会紧贴着「帮助」落在左上角。

**离屏探针实测**（`target/ui-probe-update/`）：
```text
占位条方案：徽标 diff bbox x 118..230  y 8..37   ← 压在那团氛围光上，明显不对
嵌套布局后：徽标 diff bbox x 753..865  y 8..37   ← 右内边距 14px，正好收在 866
```

正确做法是外面再套一层**默认（stretch）**的 `HorizontalLayout`：里层放菜单、自己
`alignment: start`，外层把富余宽度整份给里层，徽标自然落到最右。

#### 4.9.7 已知限制与风险

| 项 | 说明 / 升级路径 |
|---|---|
| 只认 `releases/latest` | GitHub 的该端点**不返回 prerelease**，所以预发布版收不到提示。要覆盖就得改用 `releases` 列表并自己按 semver 排序（含 prerelease 的优先级），当前不值得 |
| 校验和的信任范围 | 安装包与 `SHA256SUMS.txt` 来自同一个 release，因此校验**防的是传输损坏**（下了一半、内容坏），**不是**防 GitHub 被攻破。它在本机确实有用：网络上确实见过中途断流 |
| 装更新时 `dsh web` 变成"外部实例" | 这是 FR-21 例外的代价：重启后界面显示"检测到外部 dsh web"（FR-17 的第二档），可打开、可停止，但**不再受本程序的退出联动**保护。用户若想恢复联动，停掉再启动一次即可 |
| "为所有用户安装"的副本 | 以管理员身份安装过的那份，其更新向导会要 UAC（B 档走**可见向导**，UAC 由用户处理；这也是不选静默安装的原因之一） |
| 启动时检查失败是静默的 | 只进日志。这是刻意的：用户没要求程序去查，开机甩一句"检查更新失败"是噪音；想知道结果就点〔检查更新〕 |
| 下载进度不显示 | 状态栏一句话 + 日志。要百分比得把 worker 的字节流搬过消息通道（且 ureq 的读循环要改成流式），与收益不成比例 |

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
| `parse_notes_blocks(&str)` | FR-27 | 输入真实 release notes 片段，验证 HTML 被剥离、标题成 Heading 块、列表标记被去掉、软换行合段 |
| `dsh::is_web_url(&str)` | FR-20 / FR-27b | 表驱动：真实正文取值 —— `#en-v0.1.7-alpha.2` / `#cn-…`（锚点）与 `SAFETY.md`（相对路径）**不算**；`https://…/compare\|blob`、`http://127.0.0.1:3080`、大小写混写**算**。判据只有这一份（`open_url` 的白名单与 `dispatch_notes_link` 的准入共用），别各写一遍 `starts_with` |
| `main::dispatch_notes_link(&str, &impl Fn(Job))` | FR-27b | `main.rs` 的**第一个**测试模块（这里是纯函数、无 I/O，不属于 §6.3 排除的 GUI 测试）：给一个**真通道**再断言 worker 收件箱里有没有东西 —— 站内取值（`#en-v0.1.7-alpha.2` / `#cn-…` / `#english` / `SAFETY.md`）→ 什么都没有；真链接 → `Job::OpenUrl` 且 Url 逐字透传。**判别与接线一起被覆盖**（只测一个返回 `Option` 的分类函数挡不住"接线被改回无条件派发"）。红-绿实测见 VERIFICATION §8.3 |
| `is_safe_version(&str)` | NFR-6 | 验证字符集边界 |
| `plugin::valid_spec(&str)` | FR-35 / §7 信任边界 | 表驱动：registry 规格通过；`file:` / `link:` / `git+` / `github:` / URL / 路径 / 空白 / 引号 / `^1.0.0` **逐条拒绝** |
| `plugin::parse_latest(&str)` | FR-34 | 真实 packument 片段；缺 `dist-tags` / JSON 非法 / 版本号非法 → `None`（**不是** Err） |
| `plugin::profile_dir_from(..)` | FR-34 | `DSH_HOME` 优先、回落 `USERPROFILE\.dsh`、两者皆无 → `None`、空串按"没设"处理 |
| `plugin::read_installed(&Path)` | FR-34 | 在 tempdir 造假 profile：**不在 `dependencies` 里的目录不得出现**（spec F6 的回归钉子）；版本读不到 → `None` |
| `plugin::update_all_op(&[PluginRow])` | FR-36 | 只收严格可更新的行；没有 → `None`（按钮禁用） |
| `PluginOp::args(&str)` | FR-35~FR-37 | 四种操作的**精确**参数序列（与 `pm_command_table_is_exact` 同款粒度） |
| `PluginRow::updatable()` | FR-34 | 已是最新 / 最新版未知 / 未安装 / **最新版更旧**（不得把降级当更新）四种否定情形 |
| `plugin::run_plugin_with(..)` | FR-37b | 假 `dsh.cmd`：参数**逐字**到达进程；退出码非零是**结果**不是 `Err` |
| `dsh::app_setup_asset_name(&Version)` | FR-38 / FR-39 | 安装包资产名的**唯一构造点**，与 `.iss` 的 `OutputBaseFilename` 逐字对应（发布流水线按同一个名字上传） |
| `dsh::parse_app_release(&str, &Version)` | FR-38 | 表驱动：严格大于才算新版本（相等/更旧 → `None`）、`prerelease`/`draft` → `None`、资产名不符 → **`Err`**（发布漏传必须留痕，不得报成"已是最新"）、tag 非法 → `Err` |
| `dsh::parse_certutil_hash(&str)` | FR-39 | 中/英两种 certutil 输出**都能**取到哈希 —— 按 64 位十六进制的**形状**取，不按文案（文案随系统语言变化） |
| `dsh::parse_sha256sums(&str, &str)` | FR-39 | `sha256sum` 两种写法（`<hash>  <name>` 与 `<hash> *<name>`）、CRLF、大小写不敏感、缺条目 → `None` |

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

**已闭合（2026-09-22，原为"待验证"）**：`ureq` 3.4 的默认 TLS 后端**是** `rustls`，根证书用 **`webpki-roots`**（Mozilla 的静态根列表；ureq `lib.rs:235-237` 原话 *"By default, ureq uses Mozilla's root certificates via the webpki-roots crate"*）—— **不是**系统证书库。于是：

- 原预期成立：**不需要**追加 feature 就能在没有系统证书库的环境下工作。
- ⚠ 但**反过来说**：企业 / MITM 代理注入的证书，链到的是 **Windows 证书库**里的企业根，而 `webpki-roots` **不查那个库** → TLS 校验必然失败。
- 这条风险因 **FR-33**（系统代理，SRS v1.3）从"理论"变成"**可达**"：程序现在默认把出网交给系统代理。**本机实测未触发**（2026-09-22：清空全部 `*_PROXY` 后 `dsh::fetch_catalog()` 经系统代理 `127.0.0.1:12450` 成功拿到 24 个版本，TLS 握手正常 ⇒ 该本地代理只做 CONNECT 隧道、不终止 TLS）。**换一台机器不保证**，所以留在这里。
- 升级路径：`ureq` 的 **`platform-verifier`** feature（`rustls-platform-verifier`，改走 OS 证书库）。代价是给依赖树加一个 crate —— 按 GC-2 需先走一次依赖评审，故**本次不做**。

> ⚠ 本段原写"当前环境未配置代理（SRS §2.2.5），预期无问题"。**那个前提是错的** —— 本机配了系统代理 `127.0.0.1:12450`（SRS §2.2.5 已于 v1.3 更正）。结论仍然成立，但**理由**必须换成上面这条实测：否则一旦某台机器上的代理真的做中间人，这句"预期无问题"会把人带偏。

### 7.2 明确拒绝的依赖

| 依赖 | 拒绝理由 |
|---|---|
| `tokio` / `async-std` | CON-3 明令禁止；本项目全部 I/O 可阻塞执行于 worker 线程 |
| `reqwest` | 拉入 tokio，过重 |
| markdown 解析库（`pulldown-cmark` 等） | Slint 内建 `StyledText::from_markdown`（SRS FR-27 明令不得引入） |
| `open` / `webbrowser` | stdlib 三行即可（`explorer.exe` + 单个参数 + 自己那条 http(s) 白名单；见 §4.3） |
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
| **4** | §5.2 / FR-27 | "禁止任何指向 GitHub 网页的界面链接" | 明确适用范围：**我们自己不生成** GitHub 网页链接。release notes **正文中自带的 http(s)** 链接通过 `link-clicked` 交给系统浏览器处理，不算违反（v1.6：非 http(s) 的站内链接连派发都不派发，见 FR-27b） | v1.1 |

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
| 1.3 | 2026-09-21 | **组件样式统一**（§2.4.3、§3.2）：**①** 输入框 / 下拉框改为自绘 `GlassField` / `GlassSelect`，删掉两处压制 Fluent 的 `min-width: 0px; height: 30px;` 与端口框的三层嵌套 hack；**②** 新增语义令牌（`fill` / `fill-hover` / `fill-active` / `sunk` / `solid` / `overlay` / `accent-*` / `hairline-strong` / `motion-*` / `disabled`），收敛原先散落的 6 档白百分比与 5 档时长；**③** 抽出 `Flyout` / `Divider` / `SectionHeader` / `CloseButton` 四个共用件（三个对话框的关闭按钮原先各抄一份且都没有 hover）；**④** 修掉日志列表条目的水平居中（`width: 100%` + `Text.x = 0`，实测左边缘 183/147/92 → 38/38/38）；**⑤** 删除未使用的 `Button` 导入；**⑥** 新增 `FieldLabel`（表单字段标签，与字段正文**同字号 12.5px、同高 32px**，层次只靠颜色），`Eyebrow`（9.5px）收窄为分区标题 / 元信息键专用 —— 原先字段行拿 9.5px 眉标当标签，压在 12.5px 的字段文字旁边字号差一大截；**⑦** 下拉框与输入框**角色分开**：输入框是下凹槽（`sunk` + `hairline-strong`），下拉框是凸起控件面（`fill` + `hairline` + 悬停提亮），原先两者同一个壳、看起来都能打字；**⑧** 下拉箭头补 `cross-axis-alignment: center`（漏了它会被顶到字段上沿，同一坑 PillButton 注释里已记过） |
| 1.4 | 2026-09-22 | **更新说明改为 GitHub 式分块排版**（FR-27，§2.2 数据流 / §3.2 要点 3 / §4.x 纯函数 / §8 测试表）：`preprocess_notes(&str) -> String` 换成 `parse_notes_blocks(&str) -> Vec<NoteBlock>`，属性 `notes-text: styled-text` 换成 `notes-blocks: [NoteBlock]`，新增 `NoteKind` / `NoteBlock` 与 Slint 侧的 `NoteBlockView`。**根因**：Slint 的 `StyledText` 不支持标题（官方 Currently Unsupported），也没有字重属性 ——单段文字做不到"标题比正文大"与"列表悬挂缩进"，只能把块结构交给 Rust 侧解析。随之删掉"标题前补空行"的 hack（块间距现在由布局给） |
| 1.5 | 2026-09-22 | **新增主题子系统**（`feat/theme-system`）：**①** 新增 **§4.7**（职责边界、Rust 唯一真相源、`Tokens.dark` 与 `Palette.color-scheme` **两条投影路径**及为何是两条、`system_dark` 刻意不进 `.slint`、`UiMsg::SystemThemeChanged` 与 80 ms 排空、§5.2 兜底变成条件可达的代价）；**②** `global Tokens` 的 40 条 brush 令牌改为双值（`dark ? #暗 : #浅`），暗色侧逐行不动；**③** `ThemeMode` 三档 + `resolve()` 唯一判据 + 与 winit 同源的 `system_dark()` + 无竞态监视线程；**④** `state.json` 增 `theme_mode`（缺 key = 跟随系统，旧三字段文件照常可用）；**⑤** 设置面板新增「主题」组（浅色/深色/跟随系统，强制档改不动系统标题栏已在面板内交代）；**⑥** 删掉全部 6 处过渡期 `#[allow(dead_code)]`，删除后仍是 0 警告、无真实死代码 |
| 1.6 | 2026-09-22 | **新增出网代理（SRS v1.3 的 FR-33）并闭合 §7.1 的 TLS 待验证项**：**①** §7.1 的"待验证：`ureq` 3.4 的默认 TLS 后端"**已闭合** —— 默认是 `rustls` + **`webpki-roots`**（Mozilla 静态根，**非**系统证书库），随之记下它的反面：企业/MITM 代理注入的企业根**不被信任**，而该风险因 FR-33 从理论变为可达（本机实测未触发）；升级路径是 ureq 的 `platform-verifier` feature。**②** 更正本段原来引用的错误前提"当前环境未配置代理（SRS §2.2.5）"。**③** §7.1 依赖表**不变** —— FR-33 读系统代理走的是 `windows-sys` **已启用**的 `Win32_System_Registry` feature，**未新增依赖、未改 `Cargo.toml`**（GC-2 当初挡住的只有 `winreg` 那条路） |
| 1.7 | 2026-09-22 | **新增插件卡（SRS v1.4 的 §3.9 / FR-34~FR-37b / NFR-12）**，并把同期的两处界面改版一并落档：**①** §1.3 模块图加 `plugin.rs`（叶子模块，依赖 `model`/`pm`/`dsh`，无人依赖它），§1.4 数据流加插件一条；**②** 新增 **§4.8 `src/plugin.rs`**（职责边界、为何转发 `dsh plugin` 而非直调 pnpm、为何用 `latest` tag 是对的、并行查询与失败隔离、UI 侧两条接缝）；**③** `Job::FetchPlugins` / `Job::PluginOp` / `UiMsg::Plugins` / `UiMsg::PluginOpDone` 与 `PluginsState`；**④** 属性契约去掉 PM 三件套与 `version-is-current`/`latest-version`，加 `pm-label`、`reinstall-*`、`plugin-*` 共 9 项与 6 个回调；**⑤** §6.1 纯函数表补 7 行；**⑥** §7.1 依赖表**不变**（零新增依赖，`serde_json`/`ureq`/`semver`/`windows-sys` 都已在）。**⚠ 本条同时记录一笔被证伪的设计前提**：初稿要求"插件变更前先停 `dsh web`"，理由是"profile 里有原生模块、运行中的进程会锁住 `.node` 文件"—— 实测该前提为假（profile 内零个 `.node`），而 `dsh plugin` 与运行中的服务共用同一把 profile 写锁、CLI 改完由 HMR 应用 ⇒ 不停服务才是既定路径。详见 §4.8.3 与 spec §13 |
| 1.8 | 2026-09-23 | **新增本程序自身的更新（SRS v1.5 的 §3.10 / FR-38 / FR-39）**：**①** 新增 **§4.9**（无新模块的取舍、`OutputBaseFilename` ↔ 资产名 ↔ `app_setup_asset_name` 的三方名字契约与"名字不符必须 `Err`"、完整数据流、"置标志 + timer 交接退出"的理由、`make_quit(stop_web)` 的退出语义分叉、弹框可见性为何必须由 Rust 独占写入、**`alignment: start` 下 stretch 不生效导致徽标错位的探针实测**、6 条已知限制）；**②** §3.2 属性契约加 `update-available` / `update-version` / `update-prompt-visible` 与 5 个回调；**③** §6.1 纯函数表加 4 行；**④** `Job::CheckAppUpdate` / `Job::DownloadAppUpdate` / `UiMsg::AppUpdate` / `UiMsg::AppUpdateLaunched` / `model::NewRelease`；**⑤** `state.json` 增 `skipped_app_version`（缺 key / 非字符串 = 没忽略过）；**⑥** 依赖表**不变**：哈希用系统自带 `certutil.exe`，下载复用 `ureq`（只调两个参数：全局超时 120 s、体积上限 64 MB —— ureq 默认 10 MB，而安装包已 8.7 MB） |
| 1.9 | 2026-09-23 | **说明正文的站内链接不再当浏览器地址派发（SRS v1.6 的 FR-27b 补充；用户报的 bug）**：`on_link_clicked` 原先无条件把 `link-clicked` 的字符串派发成 `Job::OpenUrl`，而 DSH 的 release 正文第一行就是语言导航行 `[中文](#cn-…) \| [English](#en-…)`（实测 20 个 release 共 40 条锚点链接，另有 6 条 `SAFETY.md` 类相对路径）—— 于是每点一次就写一条 `打开网页失败: 拒绝打开非 http(s) 链接: #en-v0.1.7-alpha.2`。**①** 新增纯函数 `dsh::is_web_url`（判据只有这一份）+ `main::dispatch_notes_link`（准入：站内链接一个任务都不派发）；**②** §3.2 属性契约 `link-clicked` 那行注释同步，**§4.3 的 `open_url` 代码块同时更正**（它还画着 Ruling 65 之前的 `cmd /c start`，并误称"URL 不含外部输入"）；**③** §6.1 纯函数表加 2 行 —— 含 `main.rs` 的**第一个**测试模块（纯函数、真通道，不是 §6.3 排除的 GUI 测试），判别与接线一起覆盖；**④** 依赖表**不变**（零新增依赖：判据是前缀，不引 url crate）。**⚠ 白名单本身未动**（Ruling 65 仍是 `open_url` 的第二道），改的是"什么算可打开的目标"这层准入；面板里**不做**锚点跳转（与"不做中英切换"同一条裁决）。红-绿实测与真实正文的取值统计见 VERIFICATION §8 |
| 1.10 | 2026-09-23 | **日志正文可选中复制（SRS v1.7 的 FR-28 修订；用户要求"输出中的文字可以选中复制"）**：`LogLine` 里那个 `Text` 换成 `read-only: true` 的 `TextInput` —— Slint 的 `Text` 没有任何选区支持，只有 `TextInput` 有，而 `read-only` 的语义正是"能选不能改"。**①** §3.2 设计要点 2 补三处必知（按下即 `GrabMouse` ⇒ 拖动改成划选、`Ctrl+A`/`Ctrl+C` 不受 read-only 影响 ⇒ 零 Rust 零依赖、`accessible-role: text` 保住读屏语义）；**②** 选区配色复用既有令牌（底色 `Tokens.accent-line`、前景色 = 本行语义色，与 `GlassField` 同款），**未新增令牌**；**③** 依赖表**不变**；**④** 离屏探针实测（VERIFICATION §9）：三行日志的行几何与墨色与改动前**逐像素一致**（y 393/413/433、x0=30、20px 行距、三种墨色），375 字长行的墨迹右边界也一致（x 30..416 —— 长行仍是"画出去被卡片裁掉"，`TextInput` 没有引入内部横向滚动）；拖选→`Ctrl+C` 得 `probe-ok-two`、`Ctrl+A`+`Ctrl+C` 得 `probe-err-three`、只拖不复制则剪贴板为空、打字不改内容。**⚠ 代价已落档**：日志区按住拖动不再滚动（滚轮/滚动条/PageUp 仍可） |
