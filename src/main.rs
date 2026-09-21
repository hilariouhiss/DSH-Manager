// GC-8：不加这行，GUI 背后会挂一个黑窗口 —— 正是本项目要消灭的东西。
// 用 cfg_attr(not(test)) 而非裸属性：避免测试构建也被标为 GUI 子系统而吞掉
// 测试输出。Task 2 的 Step 2 会验证这个写法确实让 `cargo test` 有输出。
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod config;
mod dsh;
mod model;
mod pm;
mod txn;

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};

use config::CloseBehavior;
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
    /// 首批探测数据是否已到达；未到达时界面**必须**显示"检测中…"，
    /// 不得给出"未检测到"这类结论（SRS §6.1.1 / SRS:786）。
    ///
    /// ⚠ 这个空窗不是毫秒级：`pm::probe_env` 要探测四个 PM、每个两次，
    /// 约九个真实 shim 进程，实测是**秒级**。没有这个标志时，一个明明装了
    /// dsh 的用户会在启动后的头几秒被明确告知"未检测到 dsh" —— 正是
    /// SRS:786 禁止的"未检测先下结论"。
    probed: bool,
    /// §5.2 的"worker 已死"是否**已经报过**。见 `drain` 的 `Disconnected` 分支：
    /// 该分支一旦可达就会每个 tick 重复进入，没有闩锁就会变成 12.5 Hz 的
    /// 全量 `project()`（NFR-4 要求空闲接近零），并把之后的状态文案覆盖掉。
    worker_dead: bool,
    /// UI 回调**直接改了状态、却没有任何 `Job`/消息**时要置真。
    ///
    /// ⚠ 这是必须的：`project()` 原先只在 `drain()` 返回 true 时跑，而
    /// `on_install_clicked` 的两条拒绝路径（"未检测到已安装的 dsh"、
    /// "请先选择一个目标版本"）只写 `status` 就 `return` —— 稳态下没有任何消息
    /// 可排空，于是提示永远停在状态里，**界面上看不到**，按钮点起来像坏的。
    dirty: bool,
    /// 是否已经接受了启动请求、但 worker 还没回报任何 `WebState`/`Failed`。
    ///
    /// ⚠ 光看 `WebState` 挡不住重复启动（评审轮 4）：从点击到 `Starting` 落地之间
    /// 状态仍是 `Stopped` —— 端口预检本身就要 300ms 连接 + 一次 netstat，
    /// worker 积压时这段窗口可达十几秒。窗口内的第二次点击会被接受并排在第一个
    /// `StartWeb` 之后；届时第一份已占住端口，第二次的预检便把**我们自己的**实例
    /// 判成"外部 dsh web" → `web_pid` 被清空 → 退出不再停止我们自己启动的子进程
    /// （FR-21），且该进程死掉时状态会卡在 `External`。这个标志把窗口关掉：
    /// 接受时置真，直到 worker 回报**这次启动**的结果才清掉（见 `drain`：
    /// 任一 `WebState`，或语境为启动的 `Failed`）。
    start_pending: bool,
    /// 被接受的那次启动所用的端口（与 `start_pending` 同生共死）。
    ///
    /// ⚠ 必须在**接受启动时**记下来，不能等退出时再读 `preferred_port`：
    /// 队列可能积压好几秒（端口预检 = 300ms 连接 + 一次 netstat，worker 忙时更久），
    /// 用户完全来得及在这期间改端口输入框。退出时读到的是**新**端口，而 worker
    /// 真正 spawn 的是**旧**端口上的子进程 —— 记错端口等于没记，孤儿依旧失联
    /// （FR-22 / FR-31 同时失效）。评审轮 5 的 Ruling 93。
    start_port: Option<u16>,
    catalog: Option<Catalog>,
    notes: NotesState,
    /// 已经派发出去、还没收到回复的那次"取更新说明"。见 `request_notes`：
    /// 探测与目录两个消息都会试着补说明，没有它就会为同一个版本发两次请求。
    notes_inflight: Option<Version>,
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
    /// 关闭主窗口的行为（FR-23 修订）。缺省 `Ask` = 首次关闭弹询问框，
    /// 见 `config::CloseBehavior` 与 `on_close_requested`。
    close_behavior: CloseBehavior,
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
    fn new(preferred_port: u16, close_behavior: CloseBehavior) -> Self {
        Self {
            env: PmEnv::default(),
            probed: false,
            worker_dead: false,
            dirty: false,
            start_pending: false,
            start_port: None,
            catalog: None,
            notes: NotesState::default(),
            notes_inflight: None,
            web: WebState::Stopped,
            busy: false,
            busy_label: String::new(),
            status: String::new(),
            preferred_port,
            web_pid: None,
            selected_pm: None,
            selected_version: None,
            close_behavior,
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

    /// 是否"已是最新"。判据是**通道内最新已知，且不严格新于已安装版本**。
    ///
    /// ⚠ 原先写成 `*cur == newest`：装了目录里没有的版本时（刚发布的版本、
    /// 或 `Channel::Other` 的非 alpha/rc 预发布版）`==` 为假，UI 会把**更旧**
    /// 的版本说成"↓ 可更新" —— 那正是 GC-14 存在的意义（防止把用户往下带）。
    /// 现在只有"严格更新"才提示可更新；目录缺项时宁可不提示，也不指错方向。
    /// `newest_in_channel()` 本身不动 —— 它是对的，问题只在这个比较。
    fn is_up_to_date(&self) -> bool {
        match (self.env.installed.as_ref(), self.newest_in_channel()) {
            (Some(cur), Some(newest)) => *cur >= newest,
            _ => false,
        }
    }

    /// 下拉里只放 PM 名字。归属标记**不在这里**：它现在由行尾那句"DSH 在此！"
    /// 承担（见 ui/app.slint 的包管理行），塞进选项文字会把下拉撑得很长，
    /// 而且只有选中 owner 时才有意义。
    fn pm_labels(&self) -> Vec<slint::SharedString> {
        self.env
            .available
            .iter()
            .map(|info| slint::SharedString::from(info.kind.label()))
            .collect()
    }

    /// 行尾那个"当前"的判据：下拉里选中的版本是不是本机已装的那个。
    /// ⚠ 与版本卡上的"已是最新"不是一回事：那个问的是"通道内还有没有更新的"，
    /// 这个问的是"我选中的是不是现在装着的"（选了旧版本时它就不亮）。
    fn version_is_current(&self) -> bool {
        match (self.selected_version.as_ref(), self.env.installed.as_ref()) {
            (Some(sel), Some(cur)) => sel == cur,
            _ => false,
        }
    }

    /// 行尾那句"DSH 在此！"的判据：当前选中的 PM 是不是 owner。
    /// 还没选过时下标回落到 owner（见 `selected_pm_index`），所以也算命中。
    fn pm_is_owner(&self) -> bool {
        let Some(owner) = self.env.owner else { return false };
        self.selected_pm.unwrap_or(owner) == owner
    }

    fn version_labels(&self) -> Vec<slint::SharedString> {
        let Some(catalog) = self.catalog.as_ref() else {
            return vec![];
        };
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
                // ⚠ 这里**不再**拼 "← 当前"：那个标记已经移到行尾（见 app.slint 的
                // 目标版本行）。留在选项文字里会把下拉撑长，而且它描述的是
                // "本机装的是哪个"，跟"我要装哪个"混在同一句话里。
                slint::SharedString::from(format!("{v}  ({ch})"))
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
    // ⚠ `VecModel` 只有 `remove(index)`（单个），没有 `remove(index, count)` ——
    // 简报里的 `model.remove(0, len - LOG_CAP)` 不存在（E0061）。改成循环删最旧。
    // 稳态下每次只删 1 行（push 1 删 1），循环体不会重复执行。
    while model.row_count() > LOG_CAP {
        model.remove(0);
    }
}

/// 取出并清除"回调改过状态"标志。
///
/// 与 `drain` 并列：`drain` 负责"worker 说了什么"，本函数负责"回调自己改了什么"。
/// 两者任一为真都必须 `project()`，否则会出现"状态里有、界面上没有"的静默失败。
fn take_dirty(state: &Rc<RefCell<AppState>>) -> bool {
    std::mem::take(&mut state.borrow_mut().dirty)
}

/// 唯一的状态投影点。**所有** UI 更新必须经过此函数。
///
/// FR-25：托盘实例与窗口实例不共享 global，因此必须推两份。
/// 这也是把两处赋值放在同一个函数里的原因 —— 分开写迟早会漏掉一处。
fn project(state: &AppState, win: &MainWindow, tray: &AppTray) {
    // ── 推给窗口 ──
    win.set_installed_version(
        if !state.probed {
            // SRS:786：探测未完成时不得下结论。首帧的 project() 就发生在
            // win.show() 之前，所以用户看到的第一个画面就是这一帧 ——
            // 它必须是"检测中…"（与 ui/app.slint:22 的默认值一致）。
            "检测中…".into()
        } else {
            state
                .env
                .installed
                .as_ref()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "未检测到 dsh".into())
                .into()
        },
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
    win.set_version_known(
        state.probed && state.env.installed.is_some() && state.newest_in_channel().is_some(),
    );
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
    win.set_pm_is_owner(state.pm_is_owner());
    win.set_version_options(ModelRc::from(Rc::new(VecModel::from(state.version_labels()))));
    win.set_version_index(state.selected_version_index());
    win.set_version_is_current(state.version_is_current());

    win.set_web_running(state.web.is_running());
    // ⚠ I-2：`web-running` 只认 Running/External，而 `Starting`（正常约 2 秒，超时路径
    // 最长 20 秒）期间它仍是 false —— 于是界面把"正在启动"渲染成"已停止"，且
    // `安装` 按钮在此期间可点。那正是 TR-1 最危险的窗口：此刻**还没有任何监听者**，
    // 事务的端口探测会得出"端口空闲"的结论，然后在我们自己的 `dsh web` 正启动时
    // 去动全局包 —— 就是 TR-1 要避开的 Windows 文件锁场景。
    // 这个属性把"启动中"这一事实投影到 UI：按钮变灰 + 卡片显示"启动中…"。
    win.set_web_starting(state.start_pending || matches!(state.web, WebState::Starting { .. }));
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
    // ⚠ `from_markdown` 返回 `Result<StyledText, StyledTextFromMarkdownError>`，
    // 不是 `StyledText`（签名已核实）。必须处理。
    // 解析失败时降级为**纯文本而不是空** —— 否则会出现"状态显示 ok 但说明区一片空白"
    // 的矛盾。`from_plain_text` 不返回 Result（同样已核实）。
    win.set_notes_text(
        slint::StyledText::from_markdown(&state.notes.body)
            .unwrap_or_else(|_| slint::StyledText::from_plain_text(&state.notes.body)),
    );

    win.set_log_lines(ModelRc::from(state.log.clone()));
    win.set_busy(state.busy);
    win.set_busy_label(state.busy_label.clone().into());
    win.set_status_text(state.status.clone().into());

    // ── 关于对话框的四项（FR-29）──
    // Task 15 的修复轮补上了这四个只读属性：FR-29 要求对话框含
    // 程序版本 / dsh 安装路径 / owner PM / 当前端口，缺一即不合规。
    win.set_app_version(env!("CARGO_PKG_VERSION").into());
    win.set_dsh_path(
        state
            .env
            .dsh_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
            .into(),
    );
    win.set_owner_pm(state.env.owner.map(|pm| pm.label().to_string()).unwrap_or_default().into());
    // ⚠ 必须是【实际在运行】的端口，不是用户在输入框里敲的 port-text ——
    // 后者只是偏好值。否则"关于"会与 FR-30 的持久化状态互相矛盾。
    win.set_current_port(state.web.port().unwrap_or(state.preferred_port).to_string().into());

    // ── 关闭行为（FR-23 修订：可在设置里改）──
    // ⚠ 必须每帧都推：设置面板改变的是 AppState，而"唯一点投影点"是本函数。
    win.set_close_behavior_index(state.close_behavior.index());

    // ── 推给托盘（独立实例，必须再推一次）──
    tray.set_web_running(state.web.is_running());
    tray.set_busy(state.busy);
}

/// 排空 worker 消息。返回是否发生了状态变化（决定要不要 project）。
///
/// `tx` 用于一个内部例外：目录到达后要**主动**去取当前选中版本的更新说明
/// （否则没人点下拉就永远停在"加载中"，见 `request_notes`）。
fn drain(rx: &Receiver<UiMsg>, state: &Rc<RefCell<AppState>>, tx: &Sender<Job>) -> bool {
    let mut changed = false;
    loop {
        // 本轮是否需要补一次"取说明"。⚠ 必须在 `s`（RefMut）**释放之后**再动作：
        // request_notes 自己要 borrow，带着可变借用调它会 panic。
        let mut want_notes: Option<Version> = None;
        match rx.try_recv() {
            Ok(msg) => {
                changed = true;
                let mut s = state.borrow_mut();
                match msg {
                    UiMsg::Probed(env) => {
                        s.probed = true;
                        s.env = env;
                        s.status = "环境探测完成".into();
                        // 首次探测后，默认选中当前通道的最新版
                        let newest = s.newest_in_channel();
                        if s.selected_version.is_none() {
                            s.selected_version = newest;
                        }
                        // 探测与目录谁先到不一定，两边都要试着补说明（重复由
                        // request_notes 的"同一版本正在飞就不重发"挡掉）。
                        want_notes = s.selected_version.clone();
                    }
                    UiMsg::Catalog(c) => {
                        s.catalog = Some(c);
                        let newest = s.newest_in_channel();
                        if s.selected_version.is_none() {
                            s.selected_version = newest;
                        }
                        want_notes = s.selected_version.clone();
                    }
                    UiMsg::Notes { version, result } => {
                        if s.notes_inflight.as_ref() == Some(&version) {
                            s.notes_inflight = None;
                        }
                        // ⚠ 只认【当前选中】版本的回复（评审轮 1 裁决）：worker 是串行
                        // FIFO，连续切换版本时队列里会积压好几个请求，迟到的回复若被
                        // 无条件采信，说明区就会显示**另一个版本**的正文/状态 —— 而说明区
                        // 没有任何版本标签，看起来同样权威（正是 FR-26 要避免的误报）。
                        // 规则：最新选择赢，旧回复一律丢弃。
                        if s.selected_version.as_ref() == Some(&version) {
                            s.notes.version = Some(version.clone());
                            match result {
                                Ok(body) => {
                                    // 拉到的正文进缓存（原样存，读的时候再预处理）——
                                    // 这就是"同一版本一周内不再联网"的全部实现。
                                    if let Err(e) = config::store_notes(&version.to_string(), &body)
                                    {
                                        push_log(&s, format!("更新说明缓存写入失败（不影响本次显示）：{e}"));
                                    }
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
                        // FR-21：退出语义要靠 `web_pid` 才能"只停掉本程序启动的实例"。
                        // 单一投影点在这里 —— 所有 WebState 变更都经过本臂，
                        // 于是状态与 pid 不可能不同步（例如 External/Stopped 必须清空，
                        // 否则退出时会去 taskkill 一个早已不属于我们的 pid）。
                        //
                        // ⚠ `Starting` 也必须写入 pid（评审轮 1）：从 `spawn_web`
                        // 成功到就绪为止（最长 20 秒超时窗口）只有它持有子进程 pid，
                        // 漏掉就会出现"启动后 2 秒内点退出 → 静默孤儿"（FR-21）。
                        s.web_pid = match &ws {
                            WebState::Starting { pid, .. } | WebState::Running { pid, .. } => {
                                Some(*pid)
                            }
                            _ => None,
                        };
                        // worker 对每次启动都必回一条 WebState —— 收到它就说明
                        // `start_pending` 这一段已经走完，守卫恢复看状态。
                        // `start_port` 与它同生共死（见字段说明）。
                        s.start_pending = false;
                        s.start_port = None;
                        s.web = ws;
                    }
                    UiMsg::WebExited { pid, code } => {
                        if s.web_pid.is_some_and(|cur| cur != pid) {
                            // ⚠ 只有在"当前在册的是**另一个** pid"时才忽略 —— 那是上一代
                            // 实例迟到的重复通知，不能让它踩掉新一代的状态。
                            // 反过来，`web_pid` 为 None 时必须**接受**：退出是权威事实，
                            // 忽略它会让状态永远卡在 Running/External —— 启动被禁用、
                            // 停止又定位不到，用户只能重启程序（评审轮 1 引入的回归）。
                            push_log(&s, format!("忽略旧实例（pid {pid}）的退出通知"));
                        } else {
                            push_log(&s, format!("dsh web（pid {pid}）已退出，退出码 {code:?}"));
                            s.web = WebState::Stopped;
                            s.web_pid = None;
                            let _ = config::update(|f| f.running_port = None);
                        }
                    }
                    UiMsg::Failed { context, message } => {
                        // 探测失败也必须置真：否则界面会永远停在"检测中…"，
                        // 那只是把一种错误结论换成另一种更难排查的。
                        // 上下文串按 Task 18 的实际取值（task-18-brief.md:71）。
                        if context == "环境探测" {
                            s.probed = true;
                        }
                        // ⚠ Ruling 93：**只有启动语境**的 `Failed` 才算这次启动结束。
                        //
                        // `Failed` 还承载版本列表拉取 / 环境探测 / 停止 web / 打开网页
                        // 等**与启动无关**的失败。原先无条件清除 `start_pending`，
                        // 于是：刷新 → 启动（排队）→ 版本列表拉取失败 → 守卫提前打开，
                        // 而队列里的 `StartWeb` 还在；第二次点"启动"被接受并排在它后面，
                        // 第一份已占住端口 → 第二份把我们**自己**的实例判成外部
                        // → `web_pid` 被清空 → 退出不再停自己的子进程（FR-21）。
                        //
                        // "任务执行" 也必须算：`start_web` 内部 panic 时 worker 外层
                        // 守卫发的是它，而那条路径**不会**产生任何 WebState ——
                        // 不认它就永远卡在 start_pending，启动按钮直到重启都点不动。
                        if context == "启动 dsh web" || context == "任务执行" {
                            s.start_pending = false;
                            s.start_port = None;
                        }
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
                //
                // ⚠ 闩锁（Ruling 77）：所有发送端都丢弃之后，`try_recv` **永远**
                // 返回 Disconnected，所以本分支一旦可达就会**每个 tick** 重新进入。
                // 返回 true 就是 12.5 Hz 的全量 `project()`（NFR-4 要求空闲时
                // 接近零 CPU），还会把此后任何状态文案覆盖掉。只报一次。
                //
                // 没有这个闩锁，`drop(msg_tx)` 只是把"一条永不执行的安全网"
                // 换成"一个每 80ms 烧一次 CPU 的循环"。
                if state.borrow().worker_dead {
                    break;
                }
                let mut s = state.borrow_mut();
                s.worker_dead = true;
                s.busy = false;
                s.status = "后台工作线程已停止，请重启程序".into();
                changed = true;
                break;
            }
        }
        // `s` 已在本轮结束处释放（它的作用域就是上面那个 match 块）。
        if let Some(v) = want_notes {
            request_notes(state, |j| {
                let _ = tx.send(j);
            }, v);
        }
    }
    changed
}

/// 取某个版本的更新说明：**先看缓存**，只有没缓存或已过期（一周）才联网。
///
/// 需求（FR-27 修订）：同一个版本的说明只拉一次，一周内不再联网。
/// 缓存写在 config::load_notes / store_notes（notes-cache.json），这里是唯一的
/// "什么时候该拉"的决策点 —— 与 config.rs 只做 I/O 的分工一致。
fn request_notes(state: &Rc<RefCell<AppState>>, send: impl Fn(Job), version: Version) {
    if let Some(body) = config::load_notes(&version.to_string()) {
        let mut s = state.borrow_mut();
        s.notes.version = Some(version);
        s.notes.body = dsh::preprocess_notes(&body);
        s.notes.status = Some(NotesStatus::Ok);
        // ⚠ 这条路径**不发任何消息**：不置 dirty 的话，说明区会一直停在
        // Loading（回调已经把 notes 重置过了），缓存等于没命中。
        s.dirty = true;
        return;
    }
    // 同一版本已经有一个请求在飞时不要重发：探测与目录谁先到不一定，
    // 两边都会试着补说明；重复请求会白跑一次网络（NFR-3 的 15 秒超时也在排队）。
    if state.borrow().notes_inflight.as_ref() == Some(&version) {
        return;
    }
    state.borrow_mut().notes_inflight = Some(version.clone());
    send(Job::FetchNotes { version });
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

/// `txn::Backend` 的真实实现。单元测试用 `txn::FakeBackend`，
/// 生产用这个 —— 这是全项目唯一使用 trait 的地方，理由见 ARCHITECTURE §4.2.2。
struct SystemBackend {
    /// 事务引擎内的日志（FR-28 明确要求的"执行的命令"）经此进入日志面板。
    ///
    /// trait 方法取 `&self`，所以后端可以持有 Sender —— `txn` 本身仍然
    /// 完全不依赖 UI（ARCHITECTURE §4.2.2）。这里原本是空实现，
    /// 于是 `txn.rs` 里那些 `$ npm.cmd install -g …` 行全部被丢掉了。
    tx: Sender<UiMsg>,
}

impl SystemBackend {
    fn new(tx: Sender<UiMsg>) -> Self {
        Self { tx }
    }
}

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
    /// FR-28：把事务引擎执行的命令原样送进日志面板。
    fn log(&self, line: &str) {
        let _ = self.tx.send(UiMsg::Log(line.to_string()));
    }
}

/// 从 `catch_unwind` 的载荷里取一句可读消息。
///
/// ⚠ 三处守卫（worker 外层、`Job::Probe`、事务后重新探测）都必须用它：本进程是
/// GUI 子系统（GC-8），默认 panic hook 的输出去了 NULL 的 stderr，载荷**不取出来
/// 写进日志就等于什么都没留下**，第一次崩溃完全无法诊断。
/// 载荷只可能是 `&str` 或 `String` 两种具体类型。
fn panic_detail(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "未知 panic（载荷不是字符串）".into())
}

/// 唯一的 worker 线程。**无状态** —— 它需要的一切都在 Job 载荷里。
/// 串行执行天然满足 FR-15（禁止并发事务）。
///
/// ⚠ 唯一的例外是 I-4 的**队列合并**：`FetchNotes` 是纯读取，且它的回复只有
/// "当前选中版本"那一条会被采信（见 `drain`），所以同一个出队批次里更早的同类请求
/// 必然是废的 —— 合并掉它们不会改变任何可观测结果，只是不再让它们各阻塞 worker
/// 最长 15 秒（NFR-3）。合并逻辑本身是纯函数 `model::coalesce_notes`，可直接单测。
fn spawn_worker(rx: Receiver<Job>, tx: Sender<UiMsg>) {
    std::thread::spawn(move || {
        // 合并后待执行的任务（只有"当前是 FetchNotes 且后面还排着队"时才会非空）。
        let mut pending: VecDeque<Job> = VecDeque::new();
        loop {
            // ⚠ Ruling 77 的另一半：所有发送端丢弃后 `recv` 返回 Err，线程退出 ——
            // 这正是 `drain` 里"worker 已死"分支可达的前提。
            let job = match pending.pop_front() {
                Some(j) => j,
                None => match rx.recv() {
                    Ok(j) => j,
                    Err(_) => break,
                },
            };

            // ⚠ I-4：合并**只对 FetchNotes 生效**，且只丢掉"已被更晚的同类请求取代"的那些。
            // 做法：把此刻已排队的任务一次取空（try_recv 非阻塞），交给纯函数合并 ——
            // 结果是原队列的子序列（非 notes 任务一个不丢、顺序不变，存活的说明请求
            // 留在原位），所以这里只是把合并后的批次按原序放回队首逐个执行。
            //
            // ⚠ 批次必须按**入队顺序（旧→新）**交给 `coalesce_notes`：它保留的是**末位**，
            // 而"最新选择赢"要求末位正是最后那次选择。`job` 是这一批里**最早**出队的一个
            // （`drained` 里全是它之后才入队的），所以只能插到队首 —— 写成
            // `drained.push(job)` 会把最旧的那个顶到末位，于是活下来的说明请求属于
            // **已被放弃**的版本；它的回复又因 `version != selected_version` 被丢弃，
            // 而 `on_version_changed` 早已把面板重置成 Loading ⇒ 说明区永久停在"加载中"。
            //
            // 出队时这个子队列本来就是空的，所以"没东西可合并"时直接执行当前任务，
            // 不绕一圈（否则 `pending` 会把同一个任务反复推回队首，空转）。
            if matches!(job, Job::FetchNotes { .. }) {
                let mut drained: Vec<Job> = Vec::new();
                while let Ok(next) = rx.try_recv() {
                    drained.push(next);
                }
                if !drained.is_empty() {
                    drained.insert(0, job);
                    for j in model::coalesce_notes(drained).into_iter().rev() {
                        pending.push_front(j);
                    }
                    continue;
                }
            }

            // ⚠ 单个 worker 线程意味着**一次 panic 就会让此后所有 Job 石沉大海** ——
            // 队列还在收，却再也没人取。而 §5.2 的 Disconnected 兜底接不住这种情形：
            // `dsh::spawn_web` 的两个 reader 线程和一个等待线程各自持有
            // `Sender<UiMsg>` 克隆（dsh.rs），只要 dsh web 子进程还活着，通道就
            // 不会断开，于是既没有兜底提示、也没有任何日志。
            //
            // 本进程是 windows_subsystem="windows"（GC-8）：没有 stderr，panic 的
            // 默认输出去了空处，用户看到的是一个"界面正常但按什么都没反应"的程序。
            // 所以必须在这里兜住并把失败**变成可见的消息**。
            // ⚠ 评审给的片段是 `|| execute(job, tx)` —— 那不能编译：`execute` 的
            // 第二个参数是 `&Sender<UiMsg>`，而自由函数的实参不做自动借用，闭包会
            // 按「移动」捕获 `tx`（于是循环第二轮就用不了了）。最小修正：显式写
            // `&tx`，闭包改为按共享引用捕获。
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(job, &tx)));
            if let Err(payload) = result {
                let detail = panic_detail(&*payload);
                let _ = tx.send(UiMsg::Log(format!("任务执行时发生 panic：{detail}")));
                let _ = tx.send(UiMsg::Failed {
                    context: "任务执行",
                    message: format!("内部错误，请查看日志：{detail}"),
                });
            }
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
            // ⚠ 与 worker 外层守卫同理（GC-8）：载荷必须取出来写进日志，
            // 否则"探测崩了"和"探测返回空"在界面上完全一样，无法诊断。
            Err(payload) => {
                let detail = panic_detail(&*payload);
                send(UiMsg::Log(format!("环境探测发生 panic：{detail}")));
                send(UiMsg::Failed {
                    context: "环境探测",
                    message: format!("内部错误：{detail}"),
                });
            }
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

                        // 等端口【真正释放】再开始事务 —— 停止只是发了 taskkill，
                        // 进程退出、句柄释放、端口关闭都需要时间。抢跑会撞上
                        // Windows 文件锁，正是 TR-1 要避开的东西。
                        //
                        // ⚠ 不能用 wait_port_ready —— 那个函数检测的是"端口变成
                        // 被占用"，方向正好相反（它内部 alive 回调的语义也不同）。
                        let deadline = std::time::Instant::now() + Duration::from_secs(10);
                        while dsh::port_in_use(port) && std::time::Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(200));
                        }
                        if dsh::port_in_use(port) {
                            send(UiMsg::Failed {
                                context: "停止 dsh web",
                                message: format!("端口 {port} 在 10 秒内未释放，已放弃本次变更"),
                            });
                            return;
                        }
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
            // 后端持有 Sender 克隆，事务引擎执行的每条命令（FR-28）因此能进日志面板。
            let outcome = txn::run(&SystemBackend::new(tx.clone()), origin, target);
            send(UiMsg::TxDone(outcome));

            // TR-7：事务后重新探测（owner PM 可能已改变）
            send(UiMsg::Log("事务结束，重新探测环境".into()));
            match std::panic::catch_unwind(pm::probe_env) {
                Ok(env) => send(UiMsg::Probed(env)),
                // 同样地，载荷进日志而不是被丢掉（GC-8：没有 stderr）。
                Err(payload) => {
                    send(UiMsg::Log(format!(
                        "事务后重新探测发生 panic：{}",
                        panic_detail(&*payload)
                    )));
                }
            }

            // 事务前在运行 → 重新启动
            if was_running {
                send(UiMsg::Log("事务前 dsh web 在运行，重新启动".into()));
                start_web(port, tx);
            }
        }

        Job::StartWeb { port } => start_web(port, tx),

        Job::StopWeb { port, own_pid } => {
            // NFR-7 的守卫就放在 stop_by_pid 旁边 —— 它不再依赖调用方自觉执行。
            //
            // ⚠ `own_pid` 分支【不能】跑 is_node 守卫：spawn_web 返回的是 cmd.exe
            // 包装器的 pid，`is_node` 对它实测为假 —— 套上守卫等于永远停不掉自家实例。
            // 反过来，现场按端口定位时端口**可能已被别的程序接管**，必须挡住，
            // 否则 taskkill /T /F 会把无辜进程连同其子进程一起杀掉（NFR-7 的全部理由）。
            let pid = match own_pid {
                Some(pid) => pid,
                None => {
                    // ⚠ I-1：**外部实例没有存活监控**（`dsh::spawn_web` 的三个监督线程
                    // 只属于本程序自己启动的那个实例），所以它自己退出后 `External{port}`
                    // 会一直留着 —— 界面显示"运行中"，`启动` 被 `is_running()` 挡住，
                    // 而 `停止` 又定位不到进程（`find_listener_pid` 报"未找到监听端口"），
                    // 用户唯一的出路是退出重开。
                    //
                    // 按 TR-11 的同一原则处理：**先探测，再动作**。"这个端口上已经
                    // 没有监听者"本身就是"停止"要达成的目标状态，此时该做的是清除状态，
                    // 而不是报一个用户无法处理的错误。
                    if !dsh::port_in_use(port) {
                        let _ = config::update(|f| f.running_port = None);
                        send(UiMsg::Log(format!("端口 {port} 上已无 dsh web，状态已清除")));
                        send(UiMsg::WebState(WebState::Stopped));
                        return;
                    }
                    match dsh::find_listener_pid(port) {
                        Ok(pid) if dsh::is_node(pid) => pid,
                        Ok(pid) => {
                            send(UiMsg::Failed {
                                context: "停止 dsh web",
                                message: format!("端口 {port} 被非 node 进程占用（pid {pid}），拒绝停止"),
                            });
                            return;
                        }
                        Err(e) => {
                            send(UiMsg::Failed { context: "定位 dsh web", message: e });
                            return;
                        }
                    }
                }
            };
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
    // ⚠ `Starting` 不再在这之前发（它现在必须带 pid，而 pid 要 `spawn_web` 成功才有）。
    // 前置探测只要几百毫秒，界面在此期间保持原状态（已停止）即可 —— 反过来，
    // 先发一个没有 pid 的 Starting 就等于在 20 秒超时窗口里给 FR-21 留了个洞。

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
                send(UiMsg::WebState(WebState::Failed));
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
            // ⚠ FR-21：子进程一存在就必须让 UI 侧拿到它的 pid —— 就绪可能要等
            // 20 秒（`wait_port_ready` 超时），这段时间里退出若不带上 pid，
            // 子进程就没人认领（没有 job object 兜底，`running_port` 也还没写，
            // FR-31 同样救不回来）。
            send(UiMsg::WebState(WebState::Starting { port, pid }));

            // 就绪确认后才写运行态端口（FR-31）
            if dsh::wait_port_ready(port, || true, Duration::from_secs(20)) {
                let _ = config::update(|f| f.running_port = Some(port));
                send(UiMsg::WebState(WebState::Running { port, pid }));
                send(UiMsg::Log(format!("dsh web 已就绪：http://127.0.0.1:{port}")));
            } else {
                // ⚠ 光发 WebState::Failed 是**看不见的**：界面只把 web-running 渲染成
                // "已停止"，这个变体也没有任何字段可承载原因 —— 于是用户等了整整
                // 20 秒却什么提示都没有。补一条 UiMsg::Failed，让它落到状态栏与日志面板。
                send(UiMsg::Failed {
                    context: "启动 dsh web",
                    message: "启动超时：端口未在 20 秒内就绪".into(),
                });
                send(UiMsg::WebState(WebState::Failed));
                // ⚠ 半启动的子进程若**杀不掉**，而 `running_port` 又只在就绪后才写，
                // 这个进程就成了谁都找不回的孤儿 —— 下次启动的孤儿探测没有记录可查
                // （FR-22/FR-31 同时失效）。所以失败时补记端口：探测按端口进行，
                // 有记录就还有机会认领它（评审轮 4）。
                if let Err(e) = dsh::stop_by_pid(pid) {
                    let _ = config::update(|f| f.running_port = Some(port));
                    send(UiMsg::Log(format!(
                        "启动超时且未能停止子进程（pid {pid}）：{e}；已记录端口 {port} 以便下次启动恢复"
                    )));
                }
            }
        }
        Err(e) => {
            send(UiMsg::Failed { context: "启动 dsh web", message: e });
            send(UiMsg::WebState(WebState::Failed));
        }
    }
}

/// 把 Slint 回调接到 Job 派发上。
///
/// ⚠ 这里是 GC-16 的边界：回调运行在 UI 线程，**不得**做任何子进程/网络动作。
/// 于是所有回调都只有两种形态 —— "读 AppState → 打包 Job → send"，
/// 或"写回纯内存状态"。定位 pid、校验进程名、开浏览器、拉版本全部在 worker 上。
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
                // ⚠ 必须置 dirty：本条拒绝**不发 Job**，稳态下没有任何消息可排空，
                // 只写 status 的话 timer 永远不会投影，用户看不到提示（按钮像坏的）。
                s.dirty = true;
                return;
            };
            // ⚠ 目标必须来自【用户的选择】。不能取 catalog.versions.first()
            // （那是全局最新，可能是更旧的 rc），也不能取 owner PM
            // （用户可能在下拉里切到了别的 PM）。
            let Some(target_version) = s.selected_version.clone() else {
                s.status = "请先选择一个目标版本".into();
                s.dirty = true; // 同上：这条路径也没有 Job
                return;
            };
            let target_pm = s.selected_pm.unwrap_or(owner);
            // ⚠ I-2：TR-1 在 `Starting` 窗口里**必须自己挡住**（守卫 + UI 变灰两道）。
            // 理由见 `project()` 里 `web-starting` 的说明：这段窗口里"端口探测"必然
            // 得出"空闲"，而我们的 `dsh web` 正在启动 —— 事务会在文件锁上撞车。
            //
            // ⚠ 这条守卫不能只靠 UI 的 `enabled`：`project()` 要等下一次 80ms tick，
            // 点击与变灰之间存在一拍的空隙，且托盘/未来入口不受按钮约束。
            if s.start_pending || matches!(s.web, WebState::Starting { .. }) {
                s.status = "dsh web 正在启动，请稍候再执行变更".into();
                s.dirty = true; // 同下面两条：这条路径也不发 Job，不置 dirty 就投影不出来
                return;
            }
            // ⚠ 必须传【实际在运行】的端口（若有），而不是偏好端口。
            // 若 dsh web 跑在非偏好端口上（例如上次用了 8080、偏好仍是 3080），
            // 传偏好端口会让 TR-1 去停一个空端口 —— 真正持锁的实例还在，
            // 事务照样撞上 Windows 文件锁，TR-1 就形同虚设。
            let port = s.web.port().unwrap_or(s.preferred_port);
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
        // ⚠ 不要 clone `state`：本回调只发 Job，多出来的克隆会变成
        // "unused variable" 警告（构建要求零警告）。
        let send = send.clone();
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
                // ⚠ 立刻把说明区退回 Loading 并清空正文（评审轮 1）：否则在新版本的
                // 回复到达前，面板会**继续显示上一个版本**的正文/状态 —— 而面板没有
                // 版本标签，看起来同样权威。配合 drain 里"只认当前选择"的过滤，
                // 面板要么是 Loading，要么就是当前版本的内容。
                s.notes = NotesState::default();
                // ⚠ 这个重置本身**没有消息**可排空（FetchNotes 的回复要等网络，
                // 最长十几秒），不置 dirty 就投影不出来 —— 面板会一直停在旧版本的
                // 正文上直到回复到达，等于重置没做（评审轮 4）。
                s.dirty = true;
                picked
            };
            if let Some(v) = picked {
                // ⚠ 走 request_notes 而不是直接发 Job：一周内看过的版本直接从
                // notes-cache.json 读回来，不联网（FR-27 修订）。
                request_notes(&state, |j| send(j), v);
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
            let mut s = state.borrow_mut();
            // ⚠ 两道守卫缺一不可（评审轮 4）：
            //  - `start_pending`：点击到 `Starting` 落地之间 WebState 仍是 `Stopped`
            //    （端口预检 = 300ms 连接 + 一次 netstat，worker 积压时可达十几秒），
            //    只看状态就会放进第二次点击 —— 它排在第一个 StartWeb 之后执行，届时
            //    第一份已占住端口，于是**我们自己的实例**被误判成"外部 dsh web"，
            //    `web_pid` 清空 → 退出不再停止自己启动的子进程（FR-21）。
            //  - 状态检查：已在启动/运行中时拒绝。`is_running()` 只认 Running/External，
            //    所以 Starting 期间 web-running 仍是 false，而这里也不设 busy ——
            //    "启动"按钮在整个就绪窗口（正常约 2 秒，超时路径最长 20 秒）里都可点。
            if s.start_pending || !matches!(s.web, WebState::Stopped | WebState::Failed) {
                s.status = "已在启动或运行中，忽略重复的启动请求".into();
                s.dirty = true; // 状态栏也要看得见，不能只进日志
                push_log(&s, "dsh web 已在启动或运行中，忽略重复的启动请求");
                return;
            }
            s.start_pending = true;
            // ⚠ 必须一并置 dirty：这条路径**不发 Job**（`StartWeb` 要等 worker 腾出手，
            // 积压时可达十几秒），而 tick 只在"排空到消息或 dirty"时投影 —— 不置 dirty
            // 的话，`web-starting` 整整一个积压窗口都投影不出去，启动/安装按钮看起来仍可点。
            s.dirty = true;
            let port = s.preferred_port;
            // ⚠ 端口必须与 `start_pending` 同时落账：退出路径要记的就是**这个**
            // 值，而不是退出那一刻的 `preferred_port`（Ruling 93）。
            s.start_port = Some(port);
            drop(s);
            send(Job::StartWeb { port });
        });
    }

    {
        let send = send.clone();
        let state = state.clone();
        win.on_stop_web(move || {
            let s = state.borrow();
            if let Some(port) = s.web.port() {
                let own_pid = s.web_pid;
                drop(s);
                // 只发端口 + own_pid：定位监听者（netstat）与 NFR-7 的
                // is_node 守卫（tasklist）都在 worker 上做 —— 它们是子进程，
                // 放这里就是 GC-16 禁止的"UI 线程可感知阻塞"，
                // 而且会把同一条不变量复制到每个调用方。
                send(Job::StopWeb { port, own_pid });
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
        let send = send.clone();
        win.on_link_clicked(move |url| {
            // FR-27b：说明正文自带的链接透传给系统浏览器
            send(Job::OpenUrl { url: url.to_string() });
        });
    }

    // ── 关闭行为（FR-23 修订）──
    //
    // 只有一条投影路径：`close_behavior` 落在 AppState 里，再由 `project()`
    // 推回 `close-behavior-index`。面板上的高亮因此**必须**置 dirty ——
    // 设置面板的点击不发任何 Job，稳态下没有消息可排空。
    {
        let state = state.clone();
        win.on_settings_changed(move |idx| {
            let Some(behavior) = CloseBehavior::from_index(idx) else { return };
            {
                let mut s = state.borrow_mut();
                s.close_behavior = behavior;
                s.dirty = true;
            }
            // FR-30 同款：偏好立即落盘，设置面板里没有"保存"按钮。
            // 写失败只记日志、不回滚 —— 回滚界面上的选择会让"点了没反应"，
            // 而这次选择在本次运行里已经生效。
            if let Err(e) = config::update(|f| f.close_behavior = Some(behavior)) {
                push_log(&state.borrow(), format!("关闭行为保存失败（本次运行仍生效）：{e}"));
            }
        });
    }

    // 首次关闭时那个询问框：两个单选项各带一次"是否记住"。
    {
        let state = state.clone();
        let q = quit.clone();
        let win_weak = win_weak.clone();
        win.on_close_choice(move |idx, remember| {
            let Some(behavior) = CloseBehavior::from_index(idx) else { return };
            if remember {
                {
                    let mut s = state.borrow_mut();
                    s.close_behavior = behavior;
                    s.dirty = true;
                }
                // FR-30 同款：偏好立即落盘，没有"保存"按钮。
                // 写失败只记日志、不回滚 —— 回滚界面上的选择会让"点了没反应"，
                // 而这次选择在本次运行里已经生效。
                if let Err(e) = config::update(|f| f.close_behavior = Some(behavior)) {
                    push_log(&state.borrow(), format!("关闭行为保存失败（本次运行仍生效）：{e}"));
                }
            }
            // ⚠ 没勾"记住选择"时**什么都不写**：AppState 里仍是 `Ask`，state.json 不动 ——
            // 于是设置面板继续显示"每次询问"，下次关闭还会问。这正是勾选框的语义。
            let Some(w) = win_weak.upgrade() else { return };
            w.set_close_prompt_visible(false); // 框必须先关掉，否则退出/隐藏都像卡住
            if behavior == CloseBehavior::Quit {
                q();
            } else {
                let _ = w.hide();
                push_log(&state.borrow(), "主窗口已隐藏，程序仍在托盘运行");
            }
        });
    }

    // FR-23 修订：关闭窗口做什么由用户定（隐藏 / 退出 / 每次询问）。
    // ⚠ 两个已实测确认的易错点（ARCHITECTURE §2.4.2）：
    //   1. on_close_requested 挂在 slint::Window 上（win.window()），
    //      不在生成的组件上（win.on_close_requested 不存在）
    //   2. 返回 KeepWindowShown 会【取消】关闭，返回 HideWindow 才是
    //      "接受关闭并隐藏"（表现是点 X 毫无反应的那种错，就是把两者写反了）
    {
        let state = state.clone();
        let q = quit.clone();
        let win_weak = win_weak.clone();
        win.window().on_close_requested(move || {
            // ⚠ 先把行为拷出来（`CloseBehavior` 是 Copy）。写成
            // `match state.borrow().close_behavior` 也能编译 —— 但那个临时的
            // `Ref` 会活到整个 match 结束，将来任何一个分支里加一句 `borrow_mut`
            // 就会在**点关闭按钮时**panic（运行期才发现）。
            let behavior = state.borrow().close_behavior;
            match behavior {
                CloseBehavior::Hide => {
                    push_log(&state.borrow(), "主窗口已隐藏，程序仍在托盘运行");
                    slint::CloseRequestResponse::HideWindow
                }
                CloseBehavior::Quit => {
                    // ⚠ 必须返回 KeepWindowShown：退出是自己把事件循环停掉，
                    // 若先返回 HideWindow，窗口会先消失再由 quit 收尾 —— 用户看到的是
                    // "窗口没了但进程还在"的一帧，与"退出"的预期不符。
                    q();
                    slint::CloseRequestResponse::KeepWindowShown
                }
                CloseBehavior::Ask => {
                    // ⚠ 询问框只能挂在一个**还开着**的窗口上，所以这里必须取消这次关闭
                    // （KeepWindowShown）。真正的隐藏/退出发生在上面的 close-choice 里。
                    if let Some(w) = win_weak.upgrade() {
                        w.set_close_prompt_visible(true);
                    }
                    slint::CloseRequestResponse::KeepWindowShown
                }
            }
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
        // V-9：左键单击托盘图标 → 显示主窗口（与菜单第一项同一动作）。
        // ⚠ 只能用自定义的 `tray-clicked`：Slint 1.18 **不把内建 `clicked` 暴露到
        // 生成的 Rust API 上**（`tray.on_clicked` 不存在，已实测），故 Task 16 在
        // 组件体内把它转成了这个回调。
        let win_weak = win_weak.clone();
        tray.on_tray_clicked(move || {
            if let Some(w) = win_weak.upgrade() {
                let _ = w.show();
            }
        });
    }
    {
        let send = send.clone();
        let state = state.clone();
        tray.on_start_web(move || {
            let mut s = state.borrow_mut();
            // ⚠ 与窗口"启动"同一道守卫：托盘菜单项在 Starting 期间**仍是可用的**
            // （菜单的 enabled 绑定只看 web-running/busy，二者此时都为假），
            // 所以这里同样必须挡住重复启动，否则绕开窗口按钮就能造出第二个子进程。
            // `start_pending` 的理由见 `on_start_web`。
            if s.start_pending || !matches!(s.web, WebState::Stopped | WebState::Failed) {
                s.status = "已在启动或运行中，忽略重复的启动请求".into();
                s.dirty = true;
                push_log(&s, "dsh web 已在启动或运行中，忽略重复的启动请求");
                return;
            }
            s.start_pending = true;
            s.dirty = true; // 与窗口"启动"同理：不发 Job，不置 dirty 就投影不出 web-starting
            let port = s.preferred_port;
            // 与窗口"启动"同样要与 `start_pending` 同时落账（Ruling 93）。
            s.start_port = Some(port);
            drop(s);
            send(Job::StartWeb { port });
        });
    }
    {
        let send = send.clone();
        let state = state.clone();
        tray.on_stop_web(move || {
            let s = state.borrow();
            if let Some(port) = s.web.port() {
                let own_pid = s.web_pid;
                drop(s);
                // 与窗口"停止"同一条路径：托盘不再区分自启/外部，
                // 一律把 own_pid（可能为 None）交给 worker —— 是 None 时那边
                // 现场查端口并跑 is_node 守卫，端口被别的程序接管时会被挡住（NFR-7）。
                send(Job::StopWeb { port, own_pid });
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

    // 应用菜单「文件 → 退出」：与托盘"退出"同一条路（停掉自己启动的 dsh web 再退）。
    {
        let q = quit.clone();
        win.on_quit_app(move || q());
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // §3.8 / FR-30：启动时读取持久化的偏好端口
    //
    // ⚠ FR-32/V-26 要的"记日志"**不能**用 eprintln!：本进程是
    // windows_subsystem="windows"（GC-8），GetStdHandle(STD_ERROR_HANDLE) 返回
    // NULL，std 把写失败直接吞掉 —— eprintln! 在这里是**空操作**，消息无影无踪。
    // 日志面板是本程序唯一的日志出口，所以把话带出 match，等 state 建好再 push_log。
    // （别用 `cargo run` 验证这条：它会给子进程一个继承来的控制台，从而假通过。）
    // FR-23 修订：关闭窗口的行为（隐藏 / 退出 / 每次询问）与端口同源，都读 state.json。
    // 没有记录时是 `Ask` —— "首次关闭要询问"这条需求不需要额外的"是否问过"标志：
    // 没值就是没问过。
    let (preferred_port, close_behavior, startup_note) = match config::load() {
        config::Loaded::Ok(s) => (
            s.preferred_port.unwrap_or(3080),
            s.close_behavior.unwrap_or_default(),
            None,
        ),
        config::Loaded::Missing => (3080, CloseBehavior::default(), None),
        // FR-32：记日志但不阻止启动 —— 端口回落缺省值
        config::Loaded::Corrupt(e) => (
            3080,
            CloseBehavior::default(),
            Some(format!("state.json 损坏，使用缺省端口 3080：{e}")),
        ),
        config::Loaded::NoLocation => (3080, CloseBehavior::default(), None),
    };

    let win = MainWindow::new()?;
    let tray = AppTray::new()?;

    let state = Rc::new(RefCell::new(AppState::new(preferred_port, close_behavior)));
    push_log(&state.borrow(), "DSH Manager 启动");
    if let Some(note) = startup_note {
        push_log(&state.borrow(), note);
    }

    // 首帧：显示加载态，而不是误导性的"未检测到"。
    // ⚠ 这里原先写"约 290ms 空窗期"是**错的**：290ms（SRS:782）是 80ms timer
    // 回调首次跑到的里程碑，不是数据到达时间。第一条 UiMsg::Probed 要等
    // pm::probe_env 跑完（四个 PM 各探两次、约九个 shim 进程，秒级）。
    // 本函数不依赖任何耗时假设：画什么由 AppState::probed 决定 ——
    // 未探测完时 project() 必定渲染"检测中…"，探测完成后才可能出现结论。
    project(&state.borrow(), &win, &tray);

    let (job_tx, job_rx) = mpsc::channel::<Job>();
    let (msg_tx, msg_rx) = mpsc::channel::<UiMsg>();
    spawn_worker(job_rx, msg_tx.clone());

    // 决策 3：80ms Timer 排空。实测 timer 精度 ±1.2ms，未被节流。
    // ⚠ GC-15：timer 必须存活到事件循环结束，且其捕获的 Rc 永不离开 UI 线程。
    let timer = Timer::default();
    {
        let win_w = win.as_weak();
        let tray_w = tray.as_weak();
        let state = state.clone();
        let job_tx = job_tx.clone();
        timer.start(TimerMode::Repeated, Duration::from_millis(80), move || {
            // ⚠ 两个来源都要看：除了排空消息，**回调也可能直接改了状态而没有消息**
            // （`on_install_clicked` 的两条拒绝路径就是这样）。只看 drain 的话，
            // 那些提示会写进 AppState 却永远不投影 —— 界面上看不到，按钮像坏的。
            // 不能写成 `drain(..) || take_dirty(..)`：`||` 会短路，drain 为真时
            // dirty 残留，下一 tick 白投影一次。所以先把标志取出来。
            let dirty = take_dirty(&state);
            if drain(&msg_rx, &state, &job_tx) || dirty {
                if let (Some(w), Some(t)) = (win_w.upgrade(), tray_w.upgrade()) {
                    project(&state.borrow(), &w, &t);
                }
            }
        });
    }

    // 启动时的初始任务（Q-3：每次启动查询一次更新，不做后台轮询）
    let _ = job_tx.send(Job::Probe);
    let _ = job_tx.send(Job::FetchCatalog);

    // FR-31：若上次有未清除的运行态端口，探测它 —— 这正是 FR-22 的
    // 孤儿恢复机制入口。无法确认时**保留**记录，只有确知端口空闲才清除。
    //
    // ⚠ **不要**加 "仅当 rp != preferred_port 才探测" 之类的守卫。
    // 那个方向是反的：用户通常**不会**改端口，所以孤儿最常出现在
    // **偏好端口**上；而启动时没有任何其他路径探测该端口
    // （`Job::Probe` 只做 PM 探测，不碰 web 端口）。加了守卫就变成
    // "只在少见情况下能发现孤儿" —— 恰恰与 FR-22 的意图相反。
    if let config::Loaded::Ok(s) = config::load() {
        if let Some(rp) = s.running_port {
            push_log(&state.borrow(), format!("上次的运行端口为 {rp}，探测中…"));
            let tx = msg_tx.clone();
            std::thread::spawn(move || {
                if dsh::port_in_use(rp) {
                    match dsh::find_listener_pid(rp) {
                        Ok(pid) if dsh::is_node(pid) => {
                            let _ = tx.send(UiMsg::Log(
                                format!("检测到外部 dsh web 运行在端口 {rp}（pid {pid}）"),
                            ));
                            let _ = tx.send(UiMsg::WebState(WebState::External { port: rp }));
                            return;
                        }
                        // ⚠ 端口被占用，但持有者不是 node.exe —— **不能清除记录**。
                        // 这条兜底覆盖好几种情况，而它们都无法确认"这不是 dsh web"：
                        //   - bun 托管的 dsh web：`is_node` 判据是"进程名含 node.exe"，
                        //     而 bun 生成的是 bun.exe，于是它会被读成"非 node"；
                        //   - `is_node` 内部 tasklist 执行失败时返回 false。
                        // 清除 running_port 等于把这个孤儿**永久遗忘** —— 正是
                        // FR-22 / FR-31 存在的理由（静默失联）。反过来，留下一条
                        // 陈旧但被占用的记录只多花一次廉价探测。
                        Ok(pid) => {
                            let _ = tx.send(UiMsg::Log(format!(
                                "上次的运行端口 {rp} 被非 node 进程占用（pid {pid}），无法确认是否为 dsh web，保留记录"
                            )));
                            return;
                        }
                        // 已确知端口被占用，却定位不到持有者（netstat 解析失败、
                        // tasklist/netstat 本身跑不起来…）—— 同样不能清除。
                        Err(_) => {
                            let _ = tx.send(UiMsg::Log(format!(
                                "上次的运行端口 {rp} 仍被占用但无法定位进程，保留记录"
                            )));
                            return;
                        }
                    }
                }
                // 确知空闲：视为过期，清除并回落到偏好端口（FR-32）
                let _ = config::update(|f| f.running_port = None);
                let _ = tx.send(UiMsg::Log("上次的运行端口已空闲，状态已清除".into()));
            });
        }
    }

    // ⚠ Ruling 77：这个发送端必须在这里丢弃。
    //
    // `try_recv` 只有在**所有**发送端都丢弃之后才返回 `Disconnected`。`msg_tx`
    // 若活到 `main` 结束，`drain` 里那个"worker 已死"的分支（§5.2 的崩溃检测）
    // 就**永远不可达** —— 一条只在提交信息里存在的安全网。
    //
    // 位置是刻意的：必须在上面那段孤儿探测【之后】—— 那段自己 clone 了一个
    // 发送端（此刻它还活着，正在后台探测）；此处之后 main 只发 `Job`，不再需要它。
    // 配套的 `worker_dead` 闩锁在 `AppState` 里，否则这条分支一旦可达就会
    // 每个 tick 重复触发。
    drop(msg_tx);

    // FR-23 修订版的关闭语义（隐藏 / 退出 / 每次询问）连同询问框一起挂在
    // wire_callbacks 里 —— 退出路径 `quit` 必须已经就绪，而它在这之下才定义。

    // FR-21：退出必须先停掉本程序启动的 dsh web，不留孤儿 node 进程。
    // ⚠ 用 AppState.web_pid（由 drain 的 WebState 臂维护：Starting 与 Running 都带
    // pid）而**不是** find_listener_pid：后者会拿到外部实例的 pid，退出时把它一起杀掉。
    // ⚠ 这里的 stop_by_pid 是同步的子进程调用，落在 UI 线程上 —— 它是
    // 【关机路径】上的有界动作（一次 taskkill），且必须在 quit_event_loop()
    // 之前完成；改成 worker 往返需要一个"停完再退"的握手，与收益不成比例。
    let quit: Rc<dyn Fn()> = {
        let state = state.clone();
        let job_tx = job_tx.clone();
        Rc::new(move || {
            let s = state.borrow();
            let pid = s.web_pid;
            // ⚠ Ruling 93：读的是**接受启动时**记下的端口，不是此刻的 `preferred_port`。
            // 排队期间用户改了端口输入框的话，后者指向一个 worker 根本不会用的端口 ——
            // 记录就白记了，真正被 spawn 的实例依旧失联（FR-31 的全部意义）。
            let pending_port = s.start_port;
            drop(s);
            if let Some(pid) = pid {
                // ⚠ 只有**确知已停**才清 running_port（评审轮 1）。无条件清除的话，
                // taskkill 失败时孤儿还活着、而它唯一的恢复信号（FR-31 的运行态端口）
                // 已经被抹掉 —— FR-22 的孤儿恢复与 FR-31 会同时失效，下次启动再也
                // 认不出这个进程。worker 的 Job::StopWeb 臂也正是这么写的（Ok 才清）。
                match dsh::stop_by_pid(pid) {
                    Ok(()) => {
                        let _ = config::update(|f| f.running_port = None);
                        push_log(
                            &state.borrow(),
                            format!("退出前已停止本程序启动的 dsh web（pid {pid}）"),
                        );
                    }
                    Err(e) => push_log(
                        &state.borrow(),
                        format!(
                            "退出前停止 dsh web（pid {pid}）失败：{e}；保留运行态记录，下次启动会重新探测"
                        ),
                    ),
                }
            } else if let Some(port) = pending_port {
                // ⚠ 队列里还有 StartWeb（或它正在跑）时退出：worker 可能在我们退出
                // **之后**才真正 spawn 出子进程，而那时界面已经没了 pid —— 于是留下
                // 一个既不认识（quit 只认 web_pid）又找不回（state.json 里没有
                // running_port）的孤儿。pid 在 UI 线程上无从得知，但端口可以记下来，
                // 让 FR-31 下次启动认出它。
                //
                // 分支条件用 `start_port` 而不是 `start_pending`：两者同生共死
                // （见字段说明），而这里需要的恰恰是端口本身。
                let _ = config::update(|f| f.running_port = Some(port));
                push_log(
                    &state.borrow(),
                    format!("退出时有启动任务未完成；已记录运行态端口 {port}，下次启动会探测"),
                );
            }
            let _ = job_tx; // 不再接受新任务
            slint::quit_event_loop().ok();
        })
    };

    // 托盘"退出"也走同一条路径
    wire_callbacks(&win, &tray, &job_tx, &state, win.as_weak(), quit);

    // ⚠ 简报漏了这一行（Task 16 的临时 main 里有，Task 17/19 的简报里都没有）。
    // `slint::run_event_loop_until_quit()` **不会** show 任何窗口 —— 只有
    // `ComponentHandle::run()` 才 "first calls show()"（slint-1.18.0/lib.rs:140 文档）。
    // 不加这行，IsWindowVisible 实测为 false：启动后只剩托盘图标、窗口永不出现，
    // 既不满足简报 Step 3 的预期 1，也不满足 NFR-1。
    win.show()?;

    slint::run_event_loop_until_quit()?; // FR-23：不是 run_event_loop()
    drop(timer);
    Ok(())
}
