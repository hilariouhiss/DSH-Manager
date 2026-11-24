mod dsh;
mod npm;

use npm::Versions;
use serde::Serialize;
use std::fs::File;
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

pub struct AppState {
    child: Mutex<Option<Child>>,
    external_pid: Mutex<Option<u32>>,
    /// 端口已探测过一次外部 pid，避免 2 秒轮询反复拉起 PowerShell。
    probed: Mutex<bool>,
    npm_root: Mutex<Option<PathBuf>>,
    versions: Mutex<Option<(Instant, Versions)>>,
    npm_job: Mutex<bool>,
    log_dir: PathBuf,
}

impl AppState {
    fn new(log_dir: PathBuf) -> Self {
        Self {
            child: Mutex::new(None),
            external_pid: Mutex::new(None),
            probed: Mutex::new(false),
            npm_root: Mutex::new(None),
            versions: Mutex::new(None),
            npm_job: Mutex::new(false),
            log_dir,
        }
    }

    fn log_file(&self, kind: &str) -> PathBuf {
        self.log_dir.join(format!("{kind}.log"))
    }

    /// 全局 npm 安装根目录（`npm root -g`），结果缓存。
    fn npm_root(&self) -> Option<PathBuf> {
        if let Some(root) = self.npm_root.lock().unwrap().clone() {
            return Some(root);
        }
        let npm = dsh::resolve_npm()?;
        let args = vec!["root".to_string(), "-g".to_string()];
        let output = dsh::npm_command(&npm, &args).output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            return None;
        }
        let root = PathBuf::from(text);
        *self.npm_root.lock().unwrap() = Some(root.clone());
        Some(root)
    }

    fn dsh_bin(&self) -> Option<PathBuf> {
        self.npm_root()
            .map(|root| root.join("@deepseek-ai").join("dsh").join("lib").join("bin.js"))
    }

    fn installed(&self) -> Option<Installed> {
        let root = self.npm_root()?;
        npm::read_installed_version(&root).map(|version| Installed {
            version,
            path: root
                .join("@deepseek-ai")
                .join("dsh")
                .display()
                .to_string(),
        })
    }
}

#[derive(Serialize, Clone)]
pub struct EnvInfo {
    node: Option<String>,
    node_version: Option<String>,
    npm: Option<String>,
    npm_root: Option<String>,
    registry: Option<String>,
    dsh_home: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct Installed {
    version: String,
    path: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct Status {
    state: String,
    managed: bool,
    pid: Option<u32>,
    url: Option<String>,
    port: u16,
}

#[derive(Serialize, Clone)]
pub struct LogTail {
    text: String,
    active: bool,
}

fn url_for(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn running(status: &str) -> bool {
    status == "running"
}

fn status_now(state: &Arc<AppState>, port: u16) -> Status {
    let mut managed_pid: Option<u32> = None;
    {
        let mut guard = state.child.lock().unwrap();
        let mut alive = false;
        if let Some(child) = guard.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                alive = true;
                managed_pid = Some(child.id());
            }
        }
        if !alive && guard.is_some() {
            *guard = None;
        }
    }
    if let Some(pid) = managed_pid {
        return Status {
            state: "running".to_string(),
            managed: true,
            pid: Some(pid),
            url: Some(url_for(port)),
            port,
        };
    }
    if dsh::tcp_probe(port) {
        let known = *state.external_pid.lock().unwrap();
        if known.is_none() && !*state.probed.lock().unwrap() {
            *state.probed.lock().unwrap() = true;
            spawn_external_probe(state, port);
        }
        return Status {
            state: "running".to_string(),
            managed: false,
            pid: known,
            url: Some(url_for(port)),
            port,
        };
    }
    *state.external_pid.lock().unwrap() = None;
    *state.probed.lock().unwrap() = false;
    Status {
        state: "stopped".to_string(),
        managed: false,
        pid: None,
        url: None,
        port,
    }
}

/// 外部实例的 pid 用 PowerShell 惰性探测一次，避免 2 秒轮询里反复拉起进程。
fn spawn_external_probe(state: &Arc<AppState>, port: u16) {
    let state = state.clone();
    std::thread::spawn(move || {
        let pid = dsh::dsh_pid_on_port(port).filter(|pid| {
            dsh::cmdline_of(*pid)
                .map(|line| dsh::is_dsh_cmdline(&line))
                .unwrap_or(false)
        });
        *state.external_pid.lock().unwrap() = pid;
    });
}

fn open_in_browser(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| url.strip_prefix("http://localhost:"))
        .ok_or_else(|| "只允许打开本机 DSH 界面".to_string())?;
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return Err("非法的界面地址".to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(dsh::CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开浏览器失败: {error}"))
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("打开浏览器失败: {error}"))
    }
}

fn start_impl(state: Arc<AppState>, port: u16, open_browser: bool) -> Result<Status, String> {
    if port == 0 {
        return Err("端口无效".to_string());
    }
    let current = status_now(&state, port);
    if running(&current.state) {
        if current.managed {
            return Ok(current);
        }
        if let Some(pid) = dsh::dsh_pid_on_port(port) {
            let is_dsh = dsh::cmdline_of(pid)
                .map(|line| dsh::is_dsh_cmdline(&line))
                .unwrap_or(false);
            if is_dsh {
                return Ok(current);
            }
            return Err(format!("端口 {port} 已被其他程序占用（PID {pid}）"));
        }
        return Err(format!("端口 {port} 已被占用"));
    }
    let node = dsh::resolve_node().ok_or("找不到 node.exe，请先安装 Node.js")?;
    let bin = state
        .dsh_bin()
        .ok_or("找不到全局安装的 @deepseek-ai/dsh，请先 npm i -g @deepseek-ai/dsh")?;
    if !bin.is_file() {
        return Err(format!("未找到 DSH 入口文件: {}", bin.display()));
    }
    let log = state.log_file("web");
    let mut child =
        dsh::spawn_dsh(&node, &bin, port, &log).map_err(|error| format!("启动 DSH 失败: {error}"))?;
    let pid = child.id();
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut ready = false;
    while Instant::now() < deadline {
        if dsh::tcp_probe(port) {
            ready = true;
            break;
        }
        if let Ok(Some(exit)) = child.try_wait() {
            let tail = dsh::tail(&log, 4000);
            return Err(format!("DSH 启动后立即退出（{exit:?}）。\n{tail}"));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    if !ready {
        dsh::kill_tree(pid);
        return Err(format!("等待端口 {port} 就绪超时（90 秒）"));
    }
    *state.child.lock().unwrap() = Some(child);
    *state.external_pid.lock().unwrap() = None;
    if open_browser {
        let _ = open_in_browser(&url_for(port));
    }
    Ok(Status {
        state: "running".to_string(),
        managed: true,
        pid: Some(pid),
        url: Some(url_for(port)),
        port,
    })
}

fn stop_impl(state: Arc<AppState>, port: u16) -> Result<Status, String> {
    let taken = state.child.lock().unwrap().take();
    if let Some(mut child) = taken {
        let pid = child.id();
        dsh::kill_tree(pid);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        *state.external_pid.lock().unwrap() = None;
        return Ok(status_now(&state, port));
    }
    if !dsh::tcp_probe(port) {
        *state.external_pid.lock().unwrap() = None;
        return Ok(status_now(&state, port));
    }
    let pid = dsh::dsh_pid_on_port(port)
        .ok_or_else(|| format!("端口 {port} 正在监听，但找不到对应进程"))?;
    let cmdline = dsh::cmdline_of(pid).ok_or("无法读取该进程的命令行")?;
    if !dsh::is_dsh_cmdline(&cmdline) {
        return Err(format!("端口 {port} 上的进程不是 DSH，已拒绝停止：{cmdline}"));
    }
    dsh::kill_tree(pid);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if !dsh::tcp_probe(port) {
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    *state.external_pid.lock().unwrap() = None;
    if dsh::tcp_probe(port) {
        return Err(format!("已向 PID {pid} 发送终止，但端口 {port} 仍在监听"));
    }
    Ok(status_now(&state, port))
}

#[tauri::command]
async fn env_info(state: tauri::State<'_, Arc<AppState>>) -> Result<EnvInfo, String> {
    let node_path = dsh::resolve_node();
    let node_version = node_path.as_ref().and_then(|node| {
        let output = std::process::Command::new(node).arg("--version").output().ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    });
    let npm_path = dsh::resolve_npm();
    let registry = npm_path.as_ref().and_then(|npm| {
        let args = vec!["config".to_string(), "get".to_string(), "registry".to_string()];
        let output = dsh::npm_command(npm, &args).output().ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    });
    Ok(EnvInfo {
        node: node_path.map(|path| path.display().to_string()),
        node_version,
        npm: npm_path.map(|path| path.display().to_string()),
        npm_root: state.npm_root().map(|path| path.display().to_string()),
        registry,
        dsh_home: std::env::var("DSH_HOME").ok(),
    })
}

#[tauri::command]
async fn installed(state: tauri::State<'_, Arc<AppState>>) -> Result<Option<Installed>, String> {
    Ok(state.installed())
}

#[tauri::command]
async fn versions(
    state: tauri::State<'_, Arc<AppState>>,
    force: bool,
) -> Result<Versions, String> {
    if !force {
        if let Some((at, cached)) = state.versions.lock().unwrap().as_ref() {
            if at.elapsed() < Duration::from_secs(60) {
                return Ok(cached.clone());
            }
        }
    }
    let npm = dsh::resolve_npm().ok_or("找不到 npm，请先安装 Node.js")?;
    let args = vec![
        "view".to_string(),
        "@deepseek-ai/dsh".to_string(),
        "versions".to_string(),
        "dist-tags".to_string(),
        "--json".to_string(),
    ];
    let output = dsh::npm_command(&npm, &args)
        .output()
        .map_err(|error| format!("执行 npm view 失败: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "npm view 失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let parsed = npm::parse_versions(&String::from_utf8_lossy(&output.stdout))?;
    *state.versions.lock().unwrap() = Some((Instant::now(), parsed.clone()));
    Ok(parsed)
}

#[tauri::command]
async fn status(
    state: tauri::State<'_, Arc<AppState>>,
    port: u16,
) -> Result<Status, String> {
    let inner = state.inner().clone();
    Ok(status_now(&inner, port))
}

#[tauri::command]
async fn start(
    state: tauri::State<'_, Arc<AppState>>,
    port: u16,
    open_browser: bool,
) -> Result<Status, String> {
    start_impl(state.inner().clone(), port, open_browser)
}

#[tauri::command]
async fn stop(state: tauri::State<'_, Arc<AppState>>, port: u16) -> Result<Status, String> {
    stop_impl(state.inner().clone(), port)
}

#[tauri::command]
async fn install(
    state: tauri::State<'_, Arc<AppState>>,
    version: String,
    restart: bool,
    port: u16,
    open_browser: bool,
) -> Result<Option<Installed>, String> {
    if !npm::validate_spec(&version) {
        return Err(format!("非法版本号: {version}"));
    }
    let state = state.inner().clone();
    if running(&status_now(&state, port).state) {
        stop_impl(state.clone(), port)?;
        std::thread::sleep(Duration::from_millis(800));
    }
    let npm = dsh::resolve_npm().ok_or("找不到 npm")?;
    let log = state.log_file("npm");
    let args = vec![
        "install".to_string(),
        "-g".to_string(),
        format!("@deepseek-ai/dsh@{version}"),
    ];
    let file = File::create(&log).map_err(|error| format!("无法创建 npm 日志: {error}"))?;
    let stderr = file.try_clone().map_err(|error| error.to_string())?;
    let mut command = dsh::npm_command(&npm, &args);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(stderr));
    *state.npm_job.lock().unwrap() = true;
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            *state.npm_job.lock().unwrap() = false;
            return Err(format!("执行 npm 失败: {error}"));
        }
    };
    let deadline = Instant::now() + Duration::from_secs(900);
    let mut timed_out = false;
    let exit = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(error) => {
                *state.npm_job.lock().unwrap() = false;
                return Err(format!("等待 npm 失败: {error}"));
            }
        }
        if Instant::now() > deadline {
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(300));
    };
    *state.npm_job.lock().unwrap() = false;
    let exit = match exit {
        Some(exit) => exit,
        None => {
            let pid = child.id();
            dsh::kill_tree(pid);
            let _ = timed_out;
            return Err("npm 安装超过 15 分钟，已终止".to_string());
        }
    };
    let log_text = dsh::tail(&log, 6000);
    if !exit.success() {
        return Err(format!("npm 安装失败（{exit:?}）：\n{log_text}"));
    }
    let after = state.installed();
    let looks_exact = version
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false);
    if looks_exact {
        let actual = after.as_ref().map(|installed| installed.version.clone());
        if actual.as_deref() != Some(version.as_str()) {
            return Err(format!(
                "安装命令已结束，但检测到的版本是 {:?}（期望 {version}）：\n{log_text}",
                actual
            ));
        }
    }
    *state.versions.lock().unwrap() = None;
    if restart {
        start_impl(state.clone(), port, open_browser)?;
    }
    Ok(after)
}

#[tauri::command]
async fn read_log(
    state: tauri::State<'_, Arc<AppState>>,
    kind: String,
) -> Result<LogTail, String> {
    let kind = match kind.as_str() {
        "web" | "npm" => kind,
        _ => return Err("未知日志类型".to_string()),
    };
    let path = state.log_file(&kind);
    let text = dsh::tail(&path, 65536);
    let active = if kind == "npm" {
        *state.npm_job.lock().unwrap()
    } else {
        let mut guard = state.child.lock().unwrap();
        let mut alive = false;
        if let Some(child) = guard.as_mut() {
            alive = matches!(child.try_wait(), Ok(None));
        }
        if !alive && guard.is_some() {
            *guard = None;
        }
        alive
    };
    Ok(LogTail { text, active })
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    open_in_browser(&url)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let log_dir = app
                .path()
                .app_local_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir())
                .join("logs");
            std::fs::create_dir_all(&log_dir).ok();
            app.manage(Arc::new(AppState::new(log_dir)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            env_info,
            installed,
            versions,
            status,
            start,
            stop,
            install,
            read_log,
            open_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod smoke {
    use super::*;
    use std::fs;

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    /// 伪造一个全局安装目录，bin.js 用一个只监听端口的 Node 进程冒充 dsh web。
    fn fake_dsh_root(port: u16) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dsh-manager-test-{}-{port}", std::process::id()));
        let lib = root.join("@deepseek-ai").join("dsh").join("lib");
        fs::create_dir_all(&lib).unwrap();
        let server = format!(
            "const http = require('http');\nhttp.createServer((req, res) => res.end('ok')).listen({port}, '127.0.0.1');\nsetInterval(() => {{}}, 1000);\n"
        );
        fs::write(lib.join("bin.js"), server).unwrap();
        fs::write(
            root.join("@deepseek-ai").join("dsh").join("package.json"),
            r#"{"name":"@deepseek-ai/dsh","version":"9.9.9"}"#,
        )
        .unwrap();
        root
    }

    fn test_state(root: PathBuf) -> Arc<AppState> {
        let logs = std::env::temp_dir().join("dsh-manager-test-logs");
        fs::create_dir_all(&logs).unwrap();
        let state = Arc::new(AppState::new(logs));
        *state.npm_root.lock().unwrap() = Some(root);
        state
    }

    #[test]
    fn starts_and_stops_managed_instance() {
        let port = free_port();
        let state = test_state(fake_dsh_root(port));
        let started = start_impl(state.clone(), port, false).expect("启动托管实例");
        assert_eq!(started.state, "running");
        assert!(started.managed);
        assert!(started.pid.is_some());
        assert!(dsh::tcp_probe(port));
        assert_eq!(status_now(&state, port).state, "running");
        let stopped = stop_impl(state.clone(), port).expect("停止托管实例");
        assert_eq!(stopped.state, "stopped");
        assert!(!dsh::tcp_probe(port));
    }

    #[test]
    fn refuses_foreign_listener() {
        let port = free_port();
        let node = dsh::resolve_node().expect("需要 node");
        let dir = std::env::temp_dir().join(format!("dsh-manager-test-foreign-{port}"));
        fs::create_dir_all(&dir).unwrap();
        let script = dir.join("server.js");
        fs::write(
            &script,
            format!(
                "require('http').createServer((req, res) => res.end('x')).listen({port}, '127.0.0.1'); setInterval(() => {{}}, 1000);"
            ),
        )
        .unwrap();
        let mut foreign = std::process::Command::new(node).arg(&script).spawn().unwrap();
        for _ in 0..50 {
            if dsh::tcp_probe(port) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let state = test_state(fake_dsh_root(port));
        let start_error = start_impl(state.clone(), port, false).unwrap_err();
        assert!(start_error.contains("占用"), "unexpected: {start_error}");
        let stop_error = stop_impl(state.clone(), port).unwrap_err();
        assert!(stop_error.contains("不是 DSH"), "unexpected: {stop_error}");
        assert!(dsh::tcp_probe(port), "外来进程不应被终止");
        dsh::kill_tree(foreign.id());
        let _ = foreign.wait();
    }

    /// 只读验证：真实 3080 实例能被识别为 DSH（绝不停止它）。
    #[test]
    fn detects_live_external_instance_read_only() {
        if !dsh::tcp_probe(3080) {
            return;
        }
        if let Some(pid) = dsh::dsh_pid_on_port(3080) {
            let line = dsh::cmdline_of(pid).unwrap_or_default();
            assert!(dsh::is_dsh_cmdline(&line), "3080 上出现了非 DSH 进程: {line}");
        }
    }
}

