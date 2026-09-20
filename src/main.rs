// GC-8：不加这行，GUI 背后会挂一个黑窗口 —— 正是本项目要消灭的东西。
// 用 cfg_attr(not(test)) 而非裸属性：避免测试构建也被标为 GUI 子系统而吞掉
// 测试输出。Task 2 的 Step 2 会验证这个写法确实让 `cargo test` 有输出。
#![cfg_attr(not(test), windows_subsystem = "windows")]

// ⚠ 临时（Task 2 ~ Task 16 期间存在）
//
// 各模块按依赖顺序逐步落地，先定义的类型/函数要到很晚才被消费。
// 在【二进制 crate】中未使用的 pub 项会触发 dead_code 警告 ——
// 实测确认：bin crate 不会因为是 pub 就豁免这个 lint。
//
// 受影响的不止 model.rs：pm.rs 的 sorted_desc 要到 Task 11 才被消费、
// probe_env 要到 Task 18；dsh.rs 的 fetch_catalog 要到 Task 18；
// txn.rs 的 run 要到 Task 18。因此抑制必须是 crate 级的 ——
// 单独给 `mod model;` 加属性解决不了其余模块。
//
// 若不抑制，Task 2~16 的构建输出会持续带着十几个无关警告 ——
// 既让"输出必须干净"的检查失效，也会掩盖真实警告。
//
// 【Task 20 必须删除本行，并确认 cargo build 与 cargo test 都零警告】
// 本行会掩盖真实死代码（它覆盖面很宽），只应短期存在。
#![allow(dead_code)]

mod config;
mod dsh;
mod model;
mod pm;
mod txn;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};

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
            probed: false,
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
    // ⚠ `VecModel` 只有 `remove(index)`（单个），没有 `remove(index, count)` ——
    // 简报里的 `model.remove(0, len - LOG_CAP)` 不存在（E0061）。改成循环删最旧。
    // 稳态下每次只删 1 行（push 1 删 1），循环体不会重复执行。
    while model.row_count() > LOG_CAP {
        model.remove(0);
    }
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
                        s.probed = true;
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
                        // 探测失败也必须置真：否则界面会永远停在"检测中…"，
                        // 那只是把一种错误结论换成另一种更难排查的。
                        // 上下文串按 Task 18 的实际取值（task-18-brief.md:71）。
                        if context == "环境探测" {
                            s.probed = true;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // §3.8 / FR-30：启动时读取持久化的偏好端口
    //
    // ⚠ FR-32/V-26 要的"记日志"**不能**用 eprintln!：本进程是
    // windows_subsystem="windows"（GC-8），GetStdHandle(STD_ERROR_HANDLE) 返回
    // NULL，std 把写失败直接吞掉 —— eprintln! 在这里是**空操作**，消息无影无踪。
    // 日志面板是本程序唯一的日志出口，所以把话带出 match，等 state 建好再 push_log。
    // （别用 `cargo run` 验证这条：它会给子进程一个继承来的控制台，从而假通过。）
    let (preferred_port, startup_note) = match config::load() {
        config::Loaded::Ok(s) => (s.preferred_port.unwrap_or(3080), None),
        config::Loaded::Missing => (3080, None),
        // FR-32：记日志但不阻止启动 —— 端口回落缺省值
        config::Loaded::Corrupt(e) => (
            3080,
            Some(format!("state.json 损坏，使用缺省端口 3080：{e}")),
        ),
        config::Loaded::NoLocation => (3080, None),
    };

    let win = MainWindow::new()?;
    let tray = AppTray::new()?;

    let state = Rc::new(RefCell::new(AppState::new(preferred_port)));
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
