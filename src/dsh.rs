//! 版本拉取、更新说明与 `dsh web` 进程监督。

// ⚠ 只有 `parse_catalog` 里那段被 `#[cfg(test)]` 门住的 dist-tags 解析用它
// （见那里的说明）—— 不加门的话 `cargo build` 会报 unused_imports。
#[cfg(test)]
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::model::*;
use crate::pm;

pub const REGISTRY_URL: &str = "https://registry.npmjs.org/@deepseek-ai/dsh";
pub const GITHUB_REPO: &str = "deepseek-ai/deepseek-harness";
/// GC-5：只允许 registry.npmjs.org 与 api.github.com。github.com 实测不可达。
pub const USER_AGENT: &str = "dsh-manager";

/// 出网失败时挂给用户的**自我解释**提示（控制器裁决）。
///
/// 依据（评审轮 1 实测）：`ureq` 的默认 feature 集**不做系统代理发现**
/// （它只读 `HTTP_PROXY`/`HTTPS_PROXY` 环境变量），而 GC-2 禁止为此引入
/// 读注册表 / WinINET 的依赖。本机就撞上了这个组合：直连 api.github.com 得到
/// HTTP 403（`X-RateLimit-Remaining: 0`，本机出口 IP 的小时配额），而系统代理
/// `127.0.0.1:12450`（HKCU ProxyServer）返回 200 —— 于是**只有本程序**失败，
/// 用户完全无从判断原因。既然不能自动发现代理，失败信息就必须自己说清楚。
pub const PROXY_HINT: &str =
    "（若本机仅允许通过系统代理出网，请设置 HTTPS_PROXY 后重启本程序）";

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

    // dist-tags：**只有测试读它** —— 生产代码刻意不读（GC-14：绝不拿 registry 的
    // tag 当"最新版本"，FR-8 要的是通道内最新）。唯一消费者是下面的 GC-14 回归，
    // 它需要真实的 tag 数据才能证明"标签在场也不会被采用"。因此整段解析与
    // `Catalog::tags` 字段一起加了 `#[cfg(test)]` 门；去掉门就是死代码。
    #[cfg(test)]
    let mut tags: BTreeMap<String, Version> = BTreeMap::new();
    #[cfg(test)]
    if let Some(obj) = v.get("dist-tags").and_then(|x| x.as_object()) {
        for (key, val) in obj {
            if let Some(s) = val.as_str() {
                if let Ok(ver) = s.parse::<Version>() {
                    tags.insert(key.clone(), ver);
                }
            }
        }
    }

    Ok(Catalog {
        versions: pm::sorted_desc(versions),
        #[cfg(test)]
        tags,
    })
}

/// FR-6。精简 packument 请求头可显著减小响应体积 —— 本需求只需要
/// `versions` 的键集合与 `dist-tags`，不需要每个版本的完整元数据。
pub fn fetch_catalog() -> Result<Catalog, String> {
    let body = agent()
        .get(REGISTRY_URL)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("registry 请求失败: {e}{PROXY_HINT}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("registry 响应读取失败: {e}"))?;
    parse_catalog(&body)
}

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

/// 这一行是否是 HTML 标题（`<h1>` ~ `<h6>`）？
///
/// 必须单独识别：DSH 的 release notes **同时**使用两种标题写法 ——
/// 语言段用 `<h3 id="cn-...">新增功能</h3>`，小节用 `### 体验优化`。
/// 若只处理 ATX 一种，HTML 那批（恰恰是层级最高的段标题）会以纯文本出现，
/// 与小节标题的粗体**视觉不一致** —— 那正是 FR-27 要消除的"垃圾文本"问题。
fn is_html_heading(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    (1..=6).any(|n| lower.contains(&format!("<h{n}")))
}

/// FR-27 的强制预处理。
///
/// Slint 的 `StyledText` 官方 Currently Unsupported 列表包含 **Headings** 与
/// **Other HTML tags**。而 DSH 的 release notes 通篇是 `### 新增功能` 这类 ATX
/// 标题，且混有 `<h3 id="...">` 裸 HTML。不预处理就会原样显示成垃圾文本。
///
/// 只做两件事：剥离 HTML 标签、标题降级为粗体。
/// 其余语法（粗体 / 斜体 / 行内代码 / **链接** / 列表）由 StyledText 原生支持，
/// **不做干预** —— 尤其不要破坏链接。
pub fn preprocess_notes(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 32);
    for line in md.lines() {
        let stripped = strip_html(line);
        // 两种标题来源都要认：ATX（`### x`）与 HTML（`<h3>x</h3>`）。
        // 先判 HTML —— 它在 strip_html 之后就认不出来了。
        let heading = if is_html_heading(line) {
            Some(stripped.trim())
        } else {
            heading_body(stripped.trim_start()).map(|r| r.trim())
        };
        match heading {
            Some(text) if !text.is_empty() => {
                out.push_str("**");
                out.push_str(text);
                out.push_str("**\n");
            }
            _ => {
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
        // ⚠ 403 既可能是"配额用尽"也可能是"只有系统代理能出网" —— 后者见 PROXY_HINT。
        Err(e) => return Err(NotesError::Net(format!("{e}{PROXY_HINT}"))),
    };
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| NotesError::Net(e.to_string()))?;
    parse_release_body(&body)
}

/// FR-17：端口占用探测。stdlib 实现，无依赖。
///
/// ⚠ "连接被拒"与"连接超时"必须区别对待，不能只看 `is_ok()`。
/// 依据（Task 18 的**实测**假阴性）：监听者的 accept 队列被塞满时，回环连接会
/// **超时**而非被拒 —— 只看 `is_ok()` 就会把"有人在监听"读成"端口空闲"。后果有二：
///   - FR-31：凭这条判断清掉 `running_port`，等于把孤儿进程**永久遗忘**（FR-22）；
///   - FR-16：`wait_port_ready` 在慢机器上误报"启动超时"。
///
/// ⚠⚠ 但"超时 ⇒ 占用"单独用是**错的**，Task 19 实测撞到：本机（Windows 防火墙
/// 三个 profile 全开）对**空闲**回环端口的拒绝延迟约 **2.0 秒**（3097/3098/3099/
/// 3100/3081/8080 逐一实测：300 ms 与 1000 ms 内均未完成，2042 ms 才以
/// ConnectionRefused 结束），而本探测的超时是 300 ms —— 于是**每个空闲端口都超时**，
/// 被读成"占用"。后果比原 bug 更严重：`start_web` 的前置检查立刻走
/// `find_listener_pid` 的 Err 分支，报"启动 dsh web失败: 未找到监听端口 X 的进程"，
/// **FR-16 在这台机器上完全无法启动**（实测 3099 上点击"启动"必失败）；
/// 此外 `Transact` 会被误判"端口被非 node 占用"而拒绝安装，`wait_port_ready`
/// 还会在 dsh web 实际未就绪时立刻返回 true（假"运行中"）。
///
/// 结论：超时是**歧义**信号，必须回落到权威来源 —— 系统监听表（`find_listener_pid`）。
/// 这条分支同时保住两个方向：
///   - 真监听者（含 accept 队列塞满的）在 netstat 里有 LISTENING 行 → true；
///   - 空闲端口即使被延迟拒绝 → netstat 无行 → false。
/// 代价：仅在超时路径上多一次 `netstat.exe`（约 40 ms）。GC-16 无碍 —— `port_in_use`
/// 的调用方全在 worker / 后台线程（`execute`、`start_web`、孤儿探测线程）；
/// GC-8 无碍 —— `pm::run_cmd` 用 CREATE_NO_WINDOW（实测全程 0 个控制台窗口）。
pub fn port_in_use(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    match TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => find_listener_pid(port).is_ok(),
        Err(_) => false,
    }
}

/// 解析 `netstat -ano`，找出监听指定端口的 PID。纯函数。
///
/// 三个必须处理的细节：
/// 1. 必须用 LISTENING 过滤 —— TIME_WAIT 行也含该端口，但其 PID 列是 0
/// 2. 必须按 ':' 切到末段后**整体**比较 —— 否则 "3080" 会命中 ":13080" 的端口数字后缀
///    （注意：带 ':' 锚点的 contains(":3080") 并不会误匹配 —— 真正的危险是无锚的末段匹配）
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
/// 按 `SHIM_NAMES` 顺序尝试 shim 全名（GC-7）：npm / pnpm / yarn 生成 `dsh.cmd`，
/// 而 **bun 生成 `dsh.exe`** —— 写死 `.cmd` 会让 bun 用户的 FR-16 直接失败。
/// 顺序与 `pm::find_dsh_on_path` 一致（也是 PATHEXT 的顺序）；失败的 `spawn`
/// 无副作用（进程根本没起来）。
///
/// **不传 `--no-open`**：让 `dsh web` 自己打开浏览器，本项目零代码实现 FR-20
/// 的自动打开。
pub fn spawn_web(port: u16, tx: Sender<UiMsg>) -> Result<u32, String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let port_arg = port.to_string();
    let mut child = None;
    let mut last_err = String::new();
    for name in SHIM_NAMES {
        let mut cmd = Command::new(name); // GC-7：必须是 shim 全名
        cmd.args(["web", "--port", &port_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        cmd.creation_flags(pm::CREATE_NO_WINDOW); // GC-8
        match cmd.spawn() {
            Ok(c) => {
                child = Some(c);
                break;
            }
            Err(e) => last_err = format!("{name}: {e}"),
        }
    }
    let mut child = child.ok_or_else(|| {
        format!("无法启动 dsh web —— 已尝试 {}：{last_err}", SHIM_NAMES.join(" / "))
    })?;
    let pid = child.id();

    let out = child.stdout.take();
    let err = child.stderr.take();

    // 必须两个独立线程读：管道缓冲区满时写端会阻塞，单线程串行读另一个
    // 管道会被写满，导致子进程卡死 —— 经典死锁。
    let h_out = spawn_reader(out, tx.clone());
    let h_err = spawn_reader(err, tx.clone());

    // waiter：**先 wait() 再 join** 两个 reader。
    //
    // 顺序理由：管道里已缓冲的字节在写端关闭后仍可读，而两个 reader 线程本来就
    // 并发排空 —— 所以先 wait() 不会截尾日志。反之，若先 join 再 wait()，只要有孙
    // 进程仍持有我们的 stdout/stderr 写句柄，reader 就永远读不到 EOF，wait() 便
    // 永不执行：子进程不被回收、WebExited 永不发送，界面会一直停在"运行中"。
    // 先 wait() 保证"回收 + 通知"一定发生，读者线程继续把日志排空。
    // ⚠ 通知必须带上 `pid`：它是 UI 侧判断"这是当前实例还是上一个实例的迟到消息"
    // 的唯一依据（迟到消息若无条件被采信，会清掉新实例的 `web_pid` —— FR-21 的孤儿）。
    thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        let _ = tx.send(UiMsg::WebExited { pid, code });
        let _ = h_out.join();
        let _ = h_err.join();
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
///
/// **不经 shell**：`cmd.exe /c start "" <url>` 会把 URL 交给 cmd 自己的解析器，
/// 而 Rust 的参数编码**不会**转义 `& | ^ < > % !` —— 它只在参数含空格、制表符或
/// 为空时才加引号（引号本身触发的是反斜杠转义，不是加引号）——
/// 于是 `https://a/&calc.exe` 这类链接会被 cmd 拆成两条命令。这里的 URL 来自
/// **网络**（release notes 里的链接），属信任边界，因此不能用 shell。
/// `explorer.exe` + 单个参数没有 shell，也就没有可注入的解析层。
///
/// 协议白名单是**防御性**的第二道：本程序只打开 http/https，其它 scheme
/// （`file:`、裸可执行文件路径等）一律在 spawn 之前就拒绝 —— 不给
/// `explorer.exe` 任何机会去解释一个非 Web 的目标。
pub fn open_url(url: &str) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(format!("拒绝打开非 http(s) 链接: {url}"));
    }

    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new("explorer.exe"); // GC-7：全名；无 shell
    cmd.arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(pm::CREATE_NO_WINDOW); // GC-8
    cmd.spawn().map_err(|e| format!("打开浏览器失败: {e}"))?;
    Ok(())
}

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
        // 不能写成 r#"..."#：JSON 里 `"## 标题` 的 "# 会提前终止 raw string。
        // 升到 r##"..."## 也没用 —— 内容里恰好也有它的终止序列 "##（实测在
        // edition 2021 与 2024 下同样编译失败，与 edition 无关）。故拆成 concat! 拼接。
        let json = concat!(
            r#"{"tag_name":"dsh-v0.1.6-alpha.2","body":""#,
            "## 标题\\n内容",
            r#""}"#
        );
        assert_eq!(parse_release_body(json).unwrap(), "## 标题\n内容");
    }

    #[test]
    fn parse_release_body_treats_null_body_as_missing() {
        // 有些 release 的 body 是 null
        let json = r#"{"tag_name":"dsh-v0.0.1-rc.1","body":null}"#;
        assert!(matches!(parse_release_body(json), Err(NotesError::Missing)));
    }

    // TIME_WAIT 行必须排在 LISTENING 之前 —— 否则"不做 LISTENING 过滤"的错误实现照样会先命中 LISTENING 行并返回 13432，该测试永远不能失败。
    /// 取自 `netstat -ano` 的真实输出形状（本机实测 3080 被 dsh web 占用）
    const NETSTAT: &str = "\
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1768
  TCP    127.0.0.1:3080         127.0.0.1:1716         TIME_WAIT       0
  TCP    127.0.0.1:3080         0.0.0.0:0              LISTENING       13432
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
}
