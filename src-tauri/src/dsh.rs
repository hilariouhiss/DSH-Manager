//! 进程与命令行辅助函数（Windows 优先实现）。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 本机回环端口是否有进程在监听。
pub fn tcp_probe(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

pub fn resolve_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let direct = dir.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        #[cfg(windows)]
        for ext in ["exe", "cmd", "bat"] {
            let candidate = dir.join(format!("{name}.{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn resolve_node() -> Option<PathBuf> {
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        let candidate = PathBuf::from(pf).join("nodejs").join("node.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    resolve_in_path("node")
}

pub fn resolve_npm() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let candidate = PathBuf::from(pf).join("nodejs").join("npm.cmd");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        resolve_in_path("npm.cmd")
    }
    #[cfg(not(windows))]
    {
        resolve_in_path("npm")
    }
}

/// 构造 npm 调用：Windows 上 .cmd 包装必须经 cmd.exe 执行。
pub fn npm_command(npm: &Path, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new("cmd");
        cmd.arg("/C")
            .raw_arg(format!("\"{}\" {}", npm.display(), args.join(" ")))
            .creation_flags(CREATE_NO_WINDOW);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new(npm);
        cmd.args(args);
        cmd
    }
}

/// 启动 dsh web；stdout/stderr 直接写入日志文件（不留管道，避免关应用时 EPIPE 打死 DSH）。
pub fn spawn_dsh(node: &Path, bin: &Path, port: u16, log: &Path) -> io::Result<Child> {
    let file = File::create(log)?;
    let stderr = file.try_clone()?;
    let mut cmd = Command::new(node);
    cmd.arg(bin)
        .arg("web")
        .arg("--port")
        .arg(port.to_string())
        .arg("--no-open");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(stderr));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.spawn()
}

pub fn powershell(script: &str) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = script;
        None
    }
}

/// 监听指定端口的进程 pid。
pub fn dsh_pid_on_port(port: u16) -> Option<u32> {
    let script = format!(
        "(Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty OwningProcess)"
    );
    powershell(&script)?.trim().parse().ok()
}

pub fn cmdline_of(pid: u32) -> Option<String> {
    let script = format!(
        "(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\" -ErrorAction SilentlyContinue | Select-Object -ExpandProperty CommandLine)"
    );
    let out = powershell(&script)?;
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// 命令行是否属于 DSH web 进程（纯函数，便于单测）。
pub fn is_dsh_cmdline(line: &str) -> bool {
    let normalized = line.replace("\\", "/").to_lowercase();
    normalized.contains("@deepseek-ai/dsh/lib/bin.js")
}

pub fn kill_tree(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    }
    #[cfg(not(windows))]
    {
        Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status()
            .is_ok()
    }
}

/// 读取日志文件尾部。
pub fn tail(path: &Path, max_bytes: u64) -> String {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(_) => return String::new(),
    };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    if start > 0 && file.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_real_dsh_cmdline() {
        let line = r#""C:\Software\nodejs\node.exe" C:\Users\xueyu\AppData\Roaming\npm/node_modules/@deepseek-ai/dsh/lib/bin.js web"#;
        assert!(is_dsh_cmdline(line));
    }

    #[test]
    fn detects_forward_slash_cmdline() {
        assert!(is_dsh_cmdline(
            "/usr/local/bin/node /home/u/.npm/global/node_modules/@deepseek-ai/dsh/lib/bin.js web --port 3080"
        ));
    }

    #[test]
    fn rejects_other_node_processes() {
        assert!(!is_dsh_cmdline(
            r#""node" "C:\Users\x\AppData\Roaming\npm\\node_modules\\@colbymchenry\\codegraph\\npm-shim.js" serve --mcp"#
        ));
        assert!(!is_dsh_cmdline(r#"node -e "setInterval(()=>{},1000)""#));
        assert!(!is_dsh_cmdline(
            r"C:\Software\nodejs\node.exe C:\tools\dsh\lib\bin.js web"
        ));
    }
}
