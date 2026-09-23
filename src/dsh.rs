//! 版本拉取、更新说明与 `dsh web` 进程监督。

// ⚠ 只有 `parse_catalog` 里那段被 `#[cfg(test)]` 门住的 dist-tags 解析用它
// （见那里的说明）—— 不加门的话 `cargo build` 会报 unused_imports。
#[cfg(test)]
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

// ⚠ 这行 import 有消费者、别在清理里删掉：`system_proxy` 要把注册表子键编成宽字符串，
// 而 `encode_wide` 是 `OsStrExt` 的 **trait 方法**、不是固有方法 —— 少了它报 E0599。
// （`src/theme.rs` 顶部那条同款注释记的是同一个坑，那边是给 `spawn_watcher` 用的。）
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::model::*;
use crate::pm;

pub const REGISTRY_URL: &str = "https://registry.npmjs.org/@deepseek-ai/dsh";
pub const GITHUB_REPO: &str = "deepseek-ai/deepseek-harness";
/// FR-38：**本程序自己**的仓库。与 `GITHUB_REPO`（被管理的那个 CLI）刻意分开：
/// 两者都会拼 `api.github.com` 的地址，混用会去 dsh 的 release 里找本程序的安装包。
pub const APP_REPO: &str = "hilariouhiss/DSH-Manager";
/// GC-5：只允许 registry.npmjs.org 与 api.github.com。github.com 实测不可达。
pub const USER_AGENT: &str = "dsh-manager";

/// NFR-3 的超时上限：元数据请求（目录、说明、更新检查）用 15 秒。
const METADATA_TIMEOUT_SECS: u64 = 15;

/// FR-39：安装包的下载超时。**不能沿用 15 秒** —— 安装包已经 8.7 MB，
/// 15 秒等于要求 4.6 Mbps，慢一点的线路必然失败；而失败的代价是用户
/// 点了〔下载并安装〕之后眼睁睁看着它报错。
const DOWNLOAD_TIMEOUT_SECS: u64 = 120;

/// 单个资产的大小上限。ureq 的默认上限是 10 MB，而安装包已经 8.7 MB ——
/// 再长几版就会撞上，且报的是 "body too large" 这种与"下载失败"毫无相似之处的错。
const ASSET_LIMIT_BYTES: u64 = 64 * 1024 * 1024;

/// 出网失败时挂给用户的**自我解释**提示（控制器裁决）。
///
/// 依据（评审轮 1 实测）：`ureq` 的默认 feature 集**不做系统代理发现**
/// （它只读 `HTTP_PROXY`/`HTTPS_PROXY` 环境变量）。本机就撞上了这个组合：
/// 直连 api.github.com 得到 HTTP 403（`X-RateLimit-Remaining: 0`，本机出口 IP 的
/// 小时配额），而系统代理 `127.0.0.1:12450`（HKCU ProxyServer）返回 200 ——
/// 于是**只有本程序**失败，用户完全无从判断原因。
///
/// ⚠ Ruling 89 之后本程序自己会读系统代理了（见 `system_proxy()`，默认就开着），
/// 所以这句提示现在指向**两个**出口：设置面板那一档，和 `HTTPS_PROXY`。
/// 仍然要留着 —— 用户把它关掉、或目标机器压根没配系统代理时，
/// 这句话是唯一能说明"为什么只有这个程序上不了网"的东西。
pub const PROXY_HINT: &str =
    "（若本机仅允许通过代理出网，请在「设置」里选“使用系统代理”，或设置 HTTPS_PROXY 后重启本程序）";

/// NFR-3：全局超时上限，超时后进入失败路径而非无限等待。
pub fn agent() -> ureq::Agent {
    build_agent(Duration::from_secs(METADATA_TIMEOUT_SECS))
}

/// FR-39：下载安装包用的 agent，超时比元数据请求宽得多。
///
/// ⚠ 复用同一条代理解析路径（`resolve_proxy`）是**必须的**：本机在 Ruling 89
/// 之前唯一的出网方式就是系统代理，给下载单独写一个 builder 等于把那条路掐掉，
/// 表现为"检查得到新版本、却永远下不下来"。
fn build_agent(timeout: Duration) -> ureq::Agent {
    let mut builder = ureq::Agent::config_builder().timeout_global(Some(timeout));
    // ⚠ 只有在**确实解析出代理**时才覆盖 config：`config_builder()` 的缺省
    // 是 `Proxy::try_from_env()`，显式传 `None` 会把环境变量那条路一起掐掉 ——
    // 而 `HTTPS_PROXY` 正是 V-6 验过的、以及本机在 Ruling 89 之前唯一的出网方式。
    if let Some(proxy) = resolve_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    builder.build().into()
}

/// 决定这次出网走哪个代理。**优先级：环境变量 > 系统代理 > 直连。**
///
/// 环境变量优先是刻意的：它在本需求之前就是唯一生效的方式（V-6 靠注入
/// `HTTPS_PROXY` 才验成），任何已经在用它的人升级后行为必须**逐字不变**。
/// 于是这个开关在语义上是纯加法 —— 它只可能**多给**一个候选，
/// 不会从任何人手里拿走已经能用的那条路。
fn resolve_proxy() -> Option<ureq::Proxy> {
    ureq::Proxy::try_from_env().or_else(|| {
        if crate::config::use_system_proxy() {
            system_proxy()
        } else {
            None
        }
    })
}

/// 读 Windows「Internet 选项」里的系统代理。
///
/// ⚠ **不新增依赖、不改 `Cargo.toml`**：走的是 `HKCU\...\Internet Settings`
/// 下的 `ProxyEnable` / `ProxyServer`，用的是**已启用**的
/// `Win32_System_Registry` feature —— 与 `src/theme.rs` 的注册表监视同一套 API。
/// （Ruling 89 里"被 GC-2 挡住"的只有 `winreg` crate 那条路；本函数走的这条
/// 从来就没被挡住，当时只是没被挑出来。）
///
/// 任何一步失败都返回 `None`（= 这次出网不用代理），**绝不 panic**：
/// 代理读不到是"退化成直连"，不是"上不了网"。
#[cfg(windows)]
fn system_proxy() -> Option<ureq::Proxy> {
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegOpenKeyExW,
    };

    /// 与 `theme::PERSONALIZE_KEY` 同级的平台常量。
    const INTERNET_SETTINGS: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    /// `&str` → 带结尾 NUL 的宽字符串（注册表 API 一律要 `PCWSTR`）。
    fn wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    // ⚠ 两个 `read_*` 里的 `use` 不能提到函数顶部：嵌套 `fn` 是**独立的 item**，
    // 看不见外层块的 `use`（实测 E0425 "cannot find value REG_SZ in this scope"）。
    // 同理，`(unsafe { … }) == 0` 的括号是**必需**的 —— 块表达式在运算符左侧
    // 会被解析成语句，去掉括号就报 "expected `()`, found `u32`"。

    /// 读一个 `REG_SZ`。缺失 / 类型不对 / 空串都返回 `None`。
    ///
    /// 两段式（先问长度、再取内容）而不是写死缓冲区：`ProxyServer` 在多协议
    /// 分列时（`http=…;https=…;ftp=…`）可以相当长，写死一个"够大"的值就是
    /// 一个会静默截断的魔数 —— 截断出来的代理地址只会连不上，不会报错。
    unsafe fn read_string(hkey: HKEY, name: &[u16]) -> Option<String> {
        use windows_sys::Win32::System::Registry::{REG_SZ, RegQueryValueExW};

        let mut ty: u32 = 0;
        let mut len: u32 = 0;
        let probe = unsafe {
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                std::ptr::null_mut(),
                &mut len,
            )
        };
        if probe != 0 || ty != REG_SZ {
            return None;
        }
        // ⚠ `len` 是**字节**数（含结尾 NUL），不是字符数 —— 直接当 u16 个数用
        // 会开出两倍大的缓冲区（无害），当元素数除以 2 才对（这里就是）。
        let mut buf: Vec<u16> = vec![0; (len as usize).div_ceil(2)];
        let read = unsafe {
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                buf.as_mut_ptr().cast(),
                &mut len,
            )
        };
        if read != 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf);
        let s = s.trim_end_matches('\0').trim();
        (!s.is_empty()).then(|| s.to_string())
    }

    /// 读 `ProxyEnable`（`REG_DWORD`）。**只有恰好为 1 才算开** —— 别写成
    /// `!= 0`：注册表里手改出来的 2/3 之类不是"开"的合法编码。
    unsafe fn read_enabled(hkey: HKEY, name: &[u16]) -> bool {
        use windows_sys::Win32::System::Registry::{REG_DWORD, RegQueryValueExW};

        let mut ty: u32 = 0;
        let mut val: u32 = 0;
        let mut len = std::mem::size_of::<u32>() as u32;
        let read = unsafe {
            RegQueryValueExW(
                hkey,
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut ty,
                (&raw mut val).cast(),
                &mut len,
            )
        };
        read == 0 && ty == REG_DWORD && val == 1
    }

    let mut hkey: HKEY = std::ptr::null_mut();
    let opened = unsafe {
        RegOpenKeyExW(HKEY_CURRENT_USER, wide(INTERNET_SETTINGS).as_ptr(), 0, KEY_READ, &mut hkey)
    };
    if opened != 0 {
        return None;
    }

    // ⚠ 句柄必须成对释放 —— 本函数会被每次出网调用一次（见 `config::use_system_proxy`
    // 的说明），漏掉就是每请求一个内核句柄泄漏。
    let out = unsafe {
        if read_enabled(hkey, &wide("ProxyEnable")) {
            read_string(hkey, &wide("ProxyServer"))
        } else {
            None
        }
    };
    unsafe { RegCloseKey(hkey) };

    let raw = out?;
    let addr = parse_proxy_server(&raw)?;
    // Windows 的 `ProxyServer` 从**不带 scheme**（形如 `127.0.0.1:12450`），
    // 而 ureq 需要它。补 `http://` 而不是 `https://`：这是"用 HTTP CONNECT
    // 去建隧道"的普通代理，不是"代理服务器本身跑 TLS"。
    let uri = if addr.contains("://") { addr } else { format!("http://{addr}") };
    // 读到了但解析不了（用户手改坏了注册表）→ 退化成直连。
    // 失败是**自我解释**的：直连不上时那两个 fetch_* 的错误串尾部就挂着
    // PROXY_HINT，指回设置面板 —— 所以这里不需要再单独把原因兜出来。
    ureq::Proxy::new(&uri).ok()
}

/// 非 Windows：没有"系统代理"这个东西（GC-1 只在 Windows 上验证）。
#[cfg(not(windows))]
fn system_proxy() -> Option<ureq::Proxy> {
    None
}

/// 从 `ProxyServer` 的取值里挑出这次要用的那个地址。纯函数，可单测。
///
/// 两种形状都要认：
/// - `127.0.0.1:12450` —— 所有协议共用一个（Ruling 89 实测的本机取值就是这个形状）；
/// - `http=a:1;https=b:2;ftp=c:3` —— 按协议分列。
///
/// ⚠ **https 优先、http 兜底**：本程序只出 https（registry.npmjs.org 与
/// api.github.com）。分列时不看 https 就会把 http 那项当成 https 的代理用。
/// `ftp=` 之类的其它键直接忽略 —— 本程序不跑那些协议。
///
/// `ponytail:` 未解析 `ProxyOverride`（绕过列表）。上限：用户若把目标域名列进了
/// 绕过列表，我们仍会走代理（该直连的走了代理，不是"该走代理的直连了"，
/// 是安全的那一侧）。升级路径：ureq 的 `ProxyBuilder::no_proxy()` 能接住，
/// 等真有人报这个问题再接。
///
/// ⚠ 同一族的第二个缺口，一并记在这里：**系统代理那条路上 `NO_PROXY` 不生效**
/// （`Proxy::new` 不带 no_proxy）。只有当用户"没设任何 *_PROXY、只设了 NO_PROXY、
/// 又配了系统代理"时才会撞上 —— 而那正是 `try_from_env()` 返回 `None`、
/// 由系统代理接手的那一格。真要补，两处一起补。
pub fn parse_proxy_server(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // 没有 `=` 就是"一个地址管所有协议"的形状。也不要按 `:` 去猜 ——
    // IPv6 字面量（`[::1]:8080`）里全是冒号。
    if !raw.contains('=') {
        return Some(raw.to_string());
    }
    let mut http = None;
    for part in raw.split(';') {
        let Some((key, val)) = part.split_once('=') else { continue };
        let val = val.trim();
        if val.is_empty() {
            continue;
        }
        match key.trim().to_ascii_lowercase().as_str() {
            "https" => return Some(val.to_string()),
            "http" => http = Some(val.to_string()),
            _ => {}
        }
    }
    http
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
///
/// ⚠ 只有**本行内还存在配对 `>`** 的 `<` 才算标签（M-5）。逐字符状态机早先无条件
/// 进入"标签态"，于是没有闭合尖括号的正文被整段吞掉：`支持 <1s 启动` 渲染成 `支持 `
/// —— FR-27 面板里的**静默内容丢失**，比显示垃圾文本更难发现（用户看不出少了什么）。
/// 未配对的 `<` 与它后面的内容一律按正文保留。
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('>') {
            // 配对：整段（含尖括号）丢弃，但内部文字在上面/下面照常保留
            Some(j) => rest = &after[j + 1..],
            // 未配对：这个 '<' 不是标签，连同本行剩余内容原样保留
            None => {
                out.push_str(&rest[i..]);
                return out;
            }
        }
    }
    out.push_str(rest);
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

/// 更新说明里一个块的角色。
///
/// ⚠ 下标与 `ui/app.slint` 的 `NoteBlock.kind` **一一对应**，改顺序要两边一起改
/// （同 `CloseBehavior` 与 `close-behavior-index` 的约定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// 标题：比正文大一档 + 上方留白
    Heading = 0,
    /// 无序列表项：圆点由 Slint 侧单独一列画，文字悬挂缩进
    Bullet = 1,
    /// 普通段落
    Paragraph = 2,
}

/// 更新说明的一个排版块。
///
/// 为什么不再是一整段 `styled-text`：Slint 的 `StyledText` 官方 Currently
/// Unsupported 列表里就有 **Headings**，而且它**没有字重属性、只有一个字号** ——
/// 表达不了"标题比正文大"。列表同样不行：它把 markdown 列表渲染成行内的 `• `
/// 前缀，换行后第二行会退回左边缘（没有悬挂缩进）。
/// 于是块结构在 Rust 侧解析，Slint 侧按 kind 分别排版。
///
/// `text` 保留块内的**行内** markdown（粗体 / 链接 / 行内代码），由 StyledText
/// 逐块解析 —— 这几种语法它原生支持，不要破坏。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteBlock {
    pub kind: NoteKind,
    pub text: String,
}

/// FR-27 的分块预处理：把 release notes 切成"标题 / 列表项 / 段落"三种块。
///
/// 这里只做结构识别，不做排版：字号、缩进、块间距都由 Slint 侧用设计令牌决定。
/// 于是原先那个"在标题前补空行"的 hack 也删掉了 —— 块间距现在是布局给的，
/// 不再依赖"空行能渲染出约 14px"这种间接效果。
///
/// 已知局限（诚实记录，未处理）：只认无序列表（`-` / `*` / `+`），且**不区分嵌套层级** ——
/// 嵌套项会与顶层项排成一样。DSH 的 release notes 目前全是单层无序列表（"新增功能 /
/// 体验优化 / 问题修复"三节），真需要时再补一个 `level` 字段。
/// 有序列表（`1. `）按普通段落处理，编号原样留在文字里。
pub fn parse_notes_blocks(md: &str) -> Vec<NoteBlock> {
    let mut out: Vec<NoteBlock> = Vec::new();
    // 连续的非列表、非标题行合并成**同一个**段落（markdown 的软换行），
    // 空行才是段落边界 —— 这正是 GitHub 的分段规则。
    let mut para = String::new();

    fn flush(para: &mut String, out: &mut Vec<NoteBlock>) {
        let text = para.trim_end();
        if !text.is_empty() {
            out.push(NoteBlock { kind: NoteKind::Paragraph, text: text.to_string() });
        }
        para.clear();
    }

    for line in md.lines() {
        let stripped = strip_html(line);
        // 两种标题来源都要认：ATX（`### x`）与 HTML（`<h3>x</h3>`）。
        // 先判 HTML —— 它在 strip_html 之后就认不出来了。
        let heading = if is_html_heading(line) {
            Some(stripped.trim())
        } else {
            heading_body(stripped.trim_start()).map(|r| r.trim())
        };
        if let Some(text) = heading {
            if !text.is_empty() {
                flush(&mut para, &mut out);
                // 粗体只能靠 markdown 给：StyledText 没有字重属性，`**` 是唯一的加粗途径
                out.push(NoteBlock { kind: NoteKind::Heading, text: format!("**{text}**") });
                continue;
            }
        }
        let trimmed = stripped.trim();
        // 空行 = 段落边界
        if trimmed.is_empty() {
            flush(&mut para, &mut out);
            continue;
        }
        // 无序列表项：扔掉标记本身（圆点由 Slint 画，才做得出悬挂缩进）
        if let Some(rest) = bullet_body(trimmed) {
            flush(&mut para, &mut out);
            out.push(NoteBlock { kind: NoteKind::Bullet, text: rest.to_string() });
            continue;
        }
        if !para.is_empty() {
            para.push('\n');
        }
        para.push_str(trimmed);
    }
    flush(&mut para, &mut out);
    out
}

/// 无序列表项：`- x` / `* x` / `+ x`，返回标记之后的正文。
/// `-` 后面必须有空白，否则 `-5 度` 这类正文会被误判成列表。
fn bullet_body(s: &str) -> Option<&str> {
    match s.chars().next() {
        Some('-') | Some('*') | Some('+') => {}
        _ => return None,
    }
    let rest = &s[1..];
    if !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    Some(rest.trim_start())
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

// ═══════════════════ FR-38 / FR-39：本程序自己的更新 ═══════════════════
//
// 与上面那套（FR-6 / FR-26）刻意分开：那套管的是**被管理的 `dsh`**，走 npm registry
// 与 `deepseek-ai/deepseek-harness` 的 release；这套管的是**本程序自己**，
// 走本仓库的 `releases/latest`。两者只共用 `agent()` 与 `PROXY_HINT`。

/// 安装包资产名的**唯一构造点**。
///
/// ⚠ 与 `installer/dsh-manager.iss` 的
/// `OutputBaseFilename=dsh-manager-v{#AppVersion}-windows-x64-setup` 逐字对应，
/// 而发布流水线也按同一个名字把文件传上 Release。三处只要有一处改名，自动更新就
/// **静默失效**（查得到新版本、却找不到要下载的东西），所以名字只在这里拼。
pub fn app_setup_asset_name(version: &Version) -> String {
    format!("dsh-manager-v{version}-windows-x64-setup.exe")
}

/// 校验和资产名。发布流水线固定用它，格式与 `sha256sum` 的输出一致。
pub const APP_SUMS_ASSET_NAME: &str = "SHA256SUMS.txt";

/// 允许下载的资产地址前缀。资产 URL 来自**响应体**，属信任边界：
/// 只认 api.github.com —— 把 GC-5 从"约定"变成代码里的守卫。
const ASSET_URL_PREFIX: &str = "https://api.github.com/repos/";

/// FR-38：查本程序的最新 release。
///
/// `local` 由调用方给（main.rs 传 `env!("CARGO_PKG_VERSION")`），于是"有没有更新"
/// 这件事可以在测试里用任意本地版本驱动，不必真的把程序降级。
pub fn fetch_app_release(local: &Version) -> Result<Option<NewRelease>, String> {
    let url = format!("https://api.github.com/repos/{APP_REPO}/releases/latest");
    let mut resp = match agent()
        .get(&url)
        .header("User-Agent", USER_AGENT) // GitHub API 对无 UA 的请求返回 403
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        // 一个 release 都没有（新仓库、或全被删了）—— 不是错误，是"没有新版本"
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(format!("{e}{PROXY_HINT}")),
    };
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("响应读取失败: {e}"))?;
    parse_app_release(&body, local)
}

/// FR-38 的纯函数部分：release JSON → 有没有比 `local` 新的版本。
///
/// 判据与 FR-34 的"可更新"**逐字一致**：远端**严格大于**本地才算新版本 ——
/// 相等不提示；更旧（远端被回滚过）也不提示，否则点下去就是给用户降级。
/// `draft` / `prerelease` 一律不算：`releases/latest` 本就不该返回它们，
/// 这里再挡一道，免得将来换了端点就把测试版推给所有人。
pub fn parse_app_release(json: &str, local: &Version) -> Result<Option<NewRelease>, String> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("release 响应不是 JSON: {e}"))?;
    if v.get("draft").and_then(|x| x.as_bool()) == Some(true)
        || v.get("prerelease").and_then(|x| x.as_bool()) == Some(true)
    {
        return Ok(None);
    }
    let tag = v
        .get("tag_name")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "release 响应里没有 tag_name".to_string())?;
    // tag 形如 `v0.3.0`；不带 `v` 也认（GitHub 上两种写法都存在）
    let version = Version::parse(tag.trim_start_matches('v'))
        .map_err(|e| format!("release tag `{tag}` 不是合法版本号: {e}"))?;
    if version <= *local {
        return Ok(None);
    }

    let assets = v.get("assets").and_then(|x| x.as_array());
    let find = |name: &str| -> Option<String> {
        assets?
            .iter()
            .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))
            .and_then(|a| a.get("url"))
            .and_then(|u| u.as_str())
            .map(str::to_string)
    };
    // ⚠ 资产名对不上时返回 `Err` 而不是 `Ok(None)`：名字对不上意味着**这次发布漏传了
    // 安装包**（改名、流水线出错），那是真问题，必须留痕；报成"已是最新"会让它
    // 彻底隐形 —— 用户永远发现不了自动更新已经坏了。
    let setup_name = app_setup_asset_name(&version);
    let setup_url = find(&setup_name)
        .ok_or_else(|| format!("release {tag} 里没有资产 {setup_name}"))?;
    Ok(Some(NewRelease {
        version,
        setup_name,
        setup_url,
        // 校验和是**可选**的：缺了只降级成"不校验"，不该让整个更新不可用。
        sums_url: find(APP_SUMS_ASSET_NAME),
    }))
}

/// FR-39：下载一个资产到内存。
pub fn download_asset(url: &str) -> Result<Vec<u8>, String> {
    // 信任边界：URL 来自响应体，不是本程序拼的。
    if !url.starts_with(ASSET_URL_PREFIX) {
        return Err(format!("拒绝下载非 api.github.com 的资产地址: {url}"));
    }
    let mut resp = build_agent(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        .get(url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/octet-stream")
        .call()
        .map_err(|e| format!("下载失败: {e}{PROXY_HINT}"))?;
    resp.body_mut()
        .with_config()
        .limit(ASSET_LIMIT_BYTES)
        .read_to_vec()
        .map_err(|e| format!("下载失败（读取响应体）: {e}"))
}

/// FR-39：算文件的 SHA256。**零新增依赖** —— 用系统自带的 `certutil.exe`，
/// 而不是为一个只跑一次的校验引入 `sha2`（GC-2 的精神）。
pub fn sha256_file(path: &Path) -> Result<String, String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new("certutil.exe"); // GC-7：全名
    cmd.arg("-hashfile").arg(path).arg("SHA256");
    #[cfg(windows)]
    cmd.creation_flags(pm::CREATE_NO_WINDOW); // GC-8
    let out = cmd.output().map_err(|e| format!("无法执行 certutil.exe: {e}"))?;
    if !out.status.success() {
        return Err(format!("certutil.exe 退出码 {:?}", out.status.code()));
    }
    parse_certutil_hash(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| "certutil.exe 的输出里没有 64 位十六进制哈希".to_string())
}

/// 纯函数：从 `certutil -hashfile` 的输出里取哈希。
///
/// ⚠ **按形状取，不按文案取**：certutil 的首行与末行都随系统语言变化
/// （中文是「SHA256 的 <文件> 哈希:」/「CertUtil: -hashfile 命令成功完成。」），
/// 按文案匹配的实现在另一种语言的系统上必然失败。哈希本身永远是独占一行的
/// 64 位十六进制 —— 只有这一条与语言无关。
pub fn parse_certutil_hash(out: &str) -> Option<String> {
    out.lines()
        .map(str::trim)
        .find(|l| l.len() == 64 && l.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|l| l.to_ascii_lowercase())
}

/// 纯函数：从 `SHA256SUMS.txt` 里取指定文件的哈希。
///
/// 认 `sha256sum` 的两种写法（`<hash>  <name>` 与 `<hash> *<name>`）以及 CRLF
/// 行尾。文件名按**大小写不敏感**比较 —— Windows 的文件系统如此，
/// 在这里比出个假阴性只会让校验白白失败。
pub fn parse_sha256sums(text: &str, file_name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut it = line.split_whitespace();
        let (hash, name) = (it.next()?, it.next()?);
        (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| name.trim_start_matches('*'))
            .filter(|n| n.eq_ignore_ascii_case(file_name))
            .map(|_| hash.to_ascii_lowercase())
    })
}

/// FR-39：启动安装向导。**不等待** —— 向导必须活到本程序退出之后，
/// 而它要替换的正是本程序自己的 exe。
pub fn launch_setup(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new(path); // 绝对路径：调用方给的是 %TEMP% 下我们自己的目录
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(pm::CREATE_NO_WINDOW); // GC-8
    cmd.spawn().map_err(|e| format!("启动安装向导失败: {e}"))?;
    Ok(())
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
///
/// ⚠ 坏行跳过，**其他错误一律结束**本管道的读取（M-4 的两半，缺一不可）：
/// - `InvalidData`（一行里混了非 UTF-8 字节）只丢掉那一行：`lines()` 此时缓冲区已
///   前移，继续读是安全的。这正是 `map_while(Result::ok)` 做不到的 —— 它会让一行
///   坏字节导致**这个管道此后的全部日志消失**（GC-8：没有 stderr，日志面板是唯一
///   出口，等于静默失聪）。
/// - 其余读错误是**持久**的（管道已关 / 句柄失效……，每次都立即返回同一个错误），
///   `filter_map(Result::ok)` 会把它们全吞掉、让循环空转 —— 所以这里仍然要退出循环。
///   老代码里这层"任何错误即结束"的网是 `map_while` 顺带提供的，换成 filter_map 后
///   必须显式写出来。
/// EOF 仍由 `read` 返回 0 长度表示，循环照常结束。
fn spawn_reader<R: Read + Send + 'static>(
    src: Option<R>,
    tx: Sender<UiMsg>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let Some(src) = src else { return };
        for line in BufReader::new(src).lines() {
            match line {
                Ok(line) => {
                    if tx.send(UiMsg::Log(line)).is_err() {
                        break; // UI 侧已关闭
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {}
                Err(_) => break,
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
    if out.code == 0 || taskkill_says_gone(&out) {
        return Ok(());
    }
    Err(format!(
        "taskkill 退出码 {}: {}",
        out.code,
        format!("{}{}", out.stdout, out.stderr).trim()
    ))
}

/// `taskkill` 的输出是否表示"这个进程已经不存在"（= 停止目标已达成，幂等成功）。
///
/// 抽成**纯函数**是为了让它可单测：这个判据早先是错的，而错的后果很严重
/// （I-1：外部实例自然退出后 `停止` 永远失败，状态永久停在"运行中"，
/// 唯一的出路是退出重开）。两条判据各有分工：
///
/// - **退出码 128** —— 本机**实测**：`taskkill /PID <已退出的子进程> /F` 的退出码就是 128，
///   而且它**与区域设置无关**，因此是主判据；
/// - 文本匹配 —— ⚠ 它是**区域相关**的：taskkill 的错误文本来自系统消息表。本机实测是
///   英文 ASCII（`ERROR: The process "11356" not found.`），能被 `"not found"` 匹配到；
///   但在消息表为中文的 Windows 上，同样的文本是 GBK 字节，而 `pm::run_cmd` 用
///   `from_utf8_lossy` 解码，中文会全变成 U+FFFD —— 代码里那几个中文串**永远匹配不上**。
///   所以文字判据只对英文（及恰好能被这几个词命中的）区域设置有效，**绝不能**是唯一判据。
fn taskkill_says_gone(out: &pm::CmdOut) -> bool {
    if out.code == 128 {
        return true;
    }
    let msg = format!("{}{}", out.stdout, out.stderr);
    msg.contains("not found") || msg.contains("没有找到") || msg.contains("找不到")
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

    // ══════════════════ FR-38 / FR-39：本程序自身的更新 ══════════════════

    /// 真实 `releases/latest` 响应的**精简形态**（只留用到的字段：tag_name、
    /// draft/prerelease、assets 的 name 与 url）。与 registry 那组夹具同款做法。
    fn app_release_json(tag: &str, draft: bool, prerelease: bool, assets: &[(&str, &str)]) -> String {
        let assets: Vec<String> = assets
            .iter()
            .map(|(n, u)| format!(r#"{{"name":"{n}","url":"{u}"}}"#))
            .collect();
        format!(
            r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":{prerelease},"assets":[{}]}}"#,
            assets.join(",")
        )
    }

    fn api_asset(name: &str, id: u32) -> (String, String) {
        (
            name.to_string(),
            format!("https://api.github.com/repos/hilariouhiss/DSH-Manager/releases/assets/{id}"),
        )
    }

    /// U-3：判据是**严格大于**（相等/更旧都不得提示），资产**按名字**定位。
    ///
    /// ⚠ 三条否定用例各自对应一个真实故障：
    /// - 相等 → 提示了就是"每次开机都让你装同一个版本"；
    /// - 更旧 → 提示了就是**给用户降级**（与 FR-34 同一条判据）；
    /// - prerelease / draft → 把测试版推给所有人。
    #[test]
    fn parse_app_release_strictly_greater_and_asset_by_name() {
        let (n, u) = api_asset(&app_setup_asset_name(&v("0.3.0")), 1);
        let (sn, su) = api_asset(APP_SUMS_ASSET_NAME, 2);
        let json = app_release_json("v0.3.0", false, false, &[(&sn, &su), (&n, &u)]);

        let got = parse_app_release(&json, &v("0.2.1")).unwrap().expect("0.2.1 < 0.3.0 应当有新版本");
        assert_eq!(got.version, v("0.3.0"));
        assert_eq!(got.setup_name, "dsh-manager-v0.3.0-windows-x64-setup.exe");
        // 用的是 API 地址，不是 browser_download_url（那是 github.com，本机不可达）
        assert!(got.setup_url.starts_with("https://api.github.com/repos/"));
        assert_eq!(got.sums_url.as_deref(), Some(su.as_str()));

        // 相等 / 更旧 / prerelease / draft → 都不算"有新版本"
        assert!(parse_app_release(&json, &v("0.3.0")).unwrap().is_none(), "相等不得提示");
        assert!(parse_app_release(&json, &v("0.3.1")).unwrap().is_none(), "更旧不得提示（那是降级）");
        let pre = app_release_json("v0.3.0", false, true, &[(&n, &u)]);
        assert!(parse_app_release(&pre, &v("0.2.1")).unwrap().is_none(), "prerelease 不得提示");
        let draft = app_release_json("v0.3.0", true, false, &[(&n, &u)]);
        assert!(parse_app_release(&draft, &v("0.2.1")).unwrap().is_none(), "draft 不得提示");

        // 没有 SHA256SUMS.txt 的 release 仍然可用（校验降级为"不校验"）
        let no_sums = app_release_json("v0.3.0", false, false, &[(&n, &u)]);
        assert_eq!(parse_app_release(&no_sums, &v("0.2.1")).unwrap().unwrap().sums_url, None);

        // tag 不带 v 也认
        let bare = app_release_json("0.3.0", false, false, &[(&n, &u)]);
        assert_eq!(parse_app_release(&bare, &v("0.2.1")).unwrap().unwrap().version, v("0.3.0"));
    }

    /// U-3：**资产名不符必须 `Err`**，不得报成"已是最新"。
    ///
    /// 这是本模块最重要的一条断言：发布流水线改名/漏传安装包时，若这里返回
    /// `Ok(None)`，用户只会看到"已是最新"，而这个故障将**永远隐形**。
    /// 真机已经撞到过一次（v0.2.1 用的是旧资产名）。
    #[test]
    fn parse_app_release_rejects_unusable_shapes() {
        let (n, u) = api_asset(&app_setup_asset_name(&v("0.3.0")), 1);

        // 资产名不符（例如历史上的 dsh-manager-0.2.1-setup.exe）
        let wrong = app_release_json("v0.3.0", false, false, &[("dsh-manager-0.2.1-setup.exe", &u)]);
        let e = parse_app_release(&wrong, &v("0.2.1")).unwrap_err();
        assert!(e.contains("dsh-manager-v0.3.0-windows-x64-setup.exe"), "错误里必须有期望的资产名: {e}");

        // 完全没有 assets
        let none = app_release_json("v0.3.0", false, false, &[]);
        assert!(parse_app_release(&none, &v("0.2.1")).is_err());

        // tag 不是版本号 / 缺 tag_name / JSON 坏了
        let bad_tag = app_release_json("nightly", false, false, &[(&n, &u)]);
        assert!(parse_app_release(&bad_tag, &v("0.2.1")).is_err());
        assert!(parse_app_release(r#"{"assets":[]}"#, &v("0.2.1")).is_err());
        assert!(parse_app_release("not json", &v("0.2.1")).is_err());
    }

    /// U-4：`SHA256SUMS.txt` 的行匹配。
    ///
    /// 三种写法都要认：`sha256sum` 的两个空格、二进制标记 `*`、以及 Windows 上写出来的
    /// CRLF 行尾。文件名按**大小写不敏感**比较（Windows 的文件系统如此）。
    #[test]
    fn parse_sha256sums_matches_exact_file_name() {
        const H: &str = "bc4007dd9d3512e5227e8ad0426a6a2fa131ac3770e771a34aa04c687211feaa";
        const OTHER: &str = "0000000000000000000000000000000000000000000000000000000000000000";
        let text = format!(
            "{OTHER}  dsh-manager-v0.3.0-windows-x64.exe\r\n\
             {H}  dsh-manager-v0.3.0-windows-x64-setup.exe\r\n\
             {OTHER} *dsh-manager-v0.4.0-windows-x64-setup.exe\n"
        );
        assert_eq!(
            parse_sha256sums(&text, "dsh-manager-v0.3.0-windows-x64-setup.exe").as_deref(),
            Some(H)
        );
        // 大小写不敏感
        assert_eq!(
            parse_sha256sums(&text, "DSH-MANAGER-V0.3.0-WINDOWS-X64-SETUP.EXE").as_deref(),
            Some(H)
        );
        // 二进制标记写法
        assert_eq!(
            parse_sha256sums(&text, "dsh-manager-v0.4.0-windows-x64-setup.exe").as_deref(),
            Some(OTHER)
        );
        // 缺条目 / 空文件 / 哈希长度不对
        assert_eq!(parse_sha256sums(&text, "dsh-manager-v0.9.0-windows-x64-setup.exe"), None);
        assert_eq!(parse_sha256sums("", "x.exe"), None);
        assert_eq!(parse_sha256sums("deadbeef  x.exe\n", "x.exe"), None);
        // 同名前缀的另一个资产不得误命中
        assert_eq!(
            parse_sha256sums(&text, "dsh-manager-v0.3.0-windows-x64.exe"),
            Some(OTHER.to_string())
        );
    }

    /// U-5：certutil 的解析**不依赖系统语言**。
    ///
    /// 夹具是中文与英文两种真实输出形状：首行与末行都随语言变化，只有哈希那一行
    /// （独占一行的 64 位十六进制）与语言无关 —— 按文案匹配的实现在另一种语言上必然失败。
    #[test]
    fn parse_certutil_hash_reads_both_locales() {
        const H: &str = "bc4007dd9d3512e5227e8ad0426a6a2fa131ac3770e771a34aa04c687211feaa";
        let zh = format!("SHA256 的 C:/Temp/x.exe 哈希:\r\n{H}\r\nCertUtil: -hashfile 命令成功完成。\r\n");
        let en = format!(
            "SHA256 hash of C:/Temp/x.exe:\r\n{H}\r\nCertUtil: -hashfile command completed successfully.\r\n"
        );
        for (name, out) in [("zh", &zh), ("en", &en)] {
            assert_eq!(parse_certutil_hash(out).as_deref(), Some(H), "{name} 输出应能取到哈希");
        }
        // 大写要归一成小写（与 SHA256SUMS.txt 里的小写做字符串比较）
        let upper = format!("SHA256 hash:\n{}\n", H.to_uppercase());
        assert_eq!(parse_certutil_hash(&upper).as_deref(), Some(H));
        // 失败输出里没有 64 位十六进制 → None（调用方据此降级为"未校验"）
        assert_eq!(parse_certutil_hash("CertUtil: -hashfile FAILED\n"), None);
        assert_eq!(parse_certutil_hash(""), None);
    }

    /// U-6：资产地址是**信任边界** —— URL 来自响应体，不是本程序拼的。
    #[test]
    fn download_asset_rejects_non_api_github_hosts() {
        for url in [
            "https://github.com/hilariouhiss/DSH-Manager/releases/download/v0.3.0/x.exe",
            "https://evil.example.com/x.exe",
            "http://api.github.com/repos/x/y",
            "file:///C:/x.exe",
        ] {
            let e = download_asset(url).unwrap_err();
            assert!(e.contains("拒绝下载"), "{url} 必须被拒绝，得到: {e}");
        }
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

    /// ★ M-5：没有配对 `>` 的 `<` 是**正文**，不是标签。
    ///
    /// 判别性：旧的逐字符状态机在这里返回 `"支持 "`（`<1s 启动` 整段消失），
    /// 而 FR-27 面板上表现为"内容凭空少了一半" —— 用户看不出来，评审也难发现。
    #[test]
    fn strip_html_keeps_text_after_unmatched_angle_bracket() {
        assert_eq!(
            strip_html("支持 <1s 启动"),
            "支持 <1s 启动",
            "未配对的 '<' 必须连同本行剩余内容一起保留"
        );
        assert_eq!(strip_html("<未闭合"), "<未闭合");
        assert_eq!(strip_html("a < b"), "a < b", "两侧都是普通文本时不得吞掉后半句");
        // 同一行里先有一个真标签、再有一个未配对的 '<'：真标签照常剥离，尾部保留
        assert_eq!(strip_html("<b>粗</b> 支持 <1s"), "粗 支持 <1s");
    }

    /// I-1：`停止` 的幂等判据必须认**退出码 128**。
    ///
    /// 判据 ① 是**本机实测**的形状（2026-09-20）：把一个子进程杀掉后再 `taskkill /PID <它> /F`，
    /// 退出码 **128**、stderr 是英文 `ERROR: The process "11356" not found.`。
    /// 注意本机这条**英文**文本本来就能被 `"not found"` 匹配到 —— 也就是说
    /// "只看文本"的旧实现在**本机**是能通过的，I-1 的第二个漏洞在本机不显形。
    ///
    /// 判据 ② 是**构造的**最坏形状（不是本机实测）：消息表为中文的 Windows 上，
    /// 同一句话是 GBK 字节，经 `pm::run_cmd` 的 `from_utf8_lossy` 解码后中文全变 U+FFFD，
    /// 于是三个文本分支全部落空。这正是"必须有语言无关判据"的理由 ——
    /// 把它钉在测试里，比写一句"中文系统上会失败"的注释有用得多。
    #[test]
    fn taskkill_says_gone_accepts_exit_code_128() {
        // ① 本机实测：英文文本 + 退出码 128
        assert!(taskkill_says_gone(&pm::CmdOut {
            code: 128,
            stdout: String::new(),
            stderr: "ERROR: The process \"11356\" not found.".into(),
        }));

        // ② 构造的中文消息表形状：GBK 被 lossy 解码成 U+FFFD，文本判据完全失效
        let gbk_lossy = pm::CmdOut {
            code: 128,
            stdout: String::new(),
            stderr: "\u{fffd}\u{fffd}: \u{fffd}\u{fffd}\u{fffd}\u{fffd}\u{fffd} \"1\"\u{fffd}".into(),
        };
        assert!(taskkill_says_gone(&gbk_lossy), "退出码 128 = 进程不存在，必须视为已停止");

        // 文本判据仍在（对英文区域设置有效），但它不是主判据
        assert!(taskkill_says_gone(&pm::CmdOut {
            code: 1,
            stdout: String::new(),
            stderr: "ERROR: The process \"1\" not found.".into(),
        }));

        // 真正的失败（拒绝访问等）不许被吞掉
        assert!(!taskkill_says_gone(&pm::CmdOut {
            code: 5,
            stdout: String::new(),
            stderr: "ERROR: Access is denied.".into(),
        }));
    }

    /// ⚠ 上面那条测的是**纯判据**，不是 `stop_by_pid` 本身：后者要带一个真实 pid 起
    /// `taskkill.exe`，而测试里唯一拿得到的 pid 是"已退出子进程"的 pid —— 它有被系统
    /// 复用的极小可能，届时测试会杀掉一个无辜进程。接线只有一行
    /// （`if out.code == 0 || taskkill_says_gone(&out)`），刻意如此，便于肉眼核对。

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
    fn blocks_convert_headings_and_strip_html() {
        let input = "<h3 id=\"cn-x\">新增功能</h3>\n\n### 体验优化\n\n- 某条目\n";
        let blocks = parse_notes_blocks(input);
        assert_eq!(blocks.len(), 3, "两个标题 + 一条列表项，得到 {blocks:?}");
        assert_eq!(blocks[0], NoteBlock { kind: NoteKind::Heading, text: "**新增功能**".into() });
        assert_eq!(blocks[1], NoteBlock { kind: NoteKind::Heading, text: "**体验优化**".into() });
        // ⚠ 列表标记必须被**去掉**：圆点由 Slint 单独一列画，留着标记就没法悬挂缩进
        assert_eq!(blocks[2], NoteBlock { kind: NoteKind::Bullet, text: "某条目".into() });
        for b in &blocks {
            assert!(!b.text.contains('<'), "HTML 标签应被剥离，得到 {b:?}");
            assert!(!b.text.contains("###"), "ATX 标记不该留在正文里，得到 {b:?}");
        }
    }

    #[test]
    fn blocks_preserve_bilingual_nav_and_links() {
        // StyledText 原生支持链接，不该破坏它们
        let blocks = parse_notes_blocks("[中文](#cn-x) | [English](#en-x)\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "[中文](#cn-x) | [English](#en-x)");
    }

    #[test]
    fn blocks_handle_empty_input() {
        assert!(parse_notes_blocks("").is_empty());
        assert!(parse_notes_blocks("\n\n  \n").is_empty(), "只有空白的输入不产出任何块");
    }

    /// 分段规则：空行才是段落边界，连续普通行属于**同一个**段落（markdown 软换行）。
    ///
    /// 判别性：这条钉住的是"块间距由布局给"这个改动 —— 若把每一行都当成独立段落，
    /// 一条长说明会散成几十个块，每块之间都多一跳间距，读起来比原来更散。
    #[test]
    fn blocks_merge_soft_wrapped_lines_into_one_paragraph() {
        let blocks = parse_notes_blocks("第一行\n第二行\n\n另一段\n");
        assert_eq!(blocks.len(), 2, "得到 {blocks:?}");
        assert_eq!(blocks[0].text, "第一行\n第二行", "软换行应留在同一个块里");
        assert_eq!(blocks[0].kind, NoteKind::Paragraph);
        assert_eq!(blocks[1].text, "另一段");
    }

    /// `-` 后面没有空白就不是列表项，否则会吃掉正常正文。
    #[test]
    fn blocks_do_not_mistake_dashes_in_prose_for_bullets() {
        let blocks = parse_notes_blocks("- 真列表项\n-5 度不是列表\n");
        assert_eq!(blocks.len(), 2, "得到 {blocks:?}");
        assert_eq!(blocks[0].kind, NoteKind::Bullet);
        assert_eq!(blocks[0].text, "真列表项");
        assert_eq!(blocks[1].kind, NoteKind::Paragraph);
        assert_eq!(blocks[1].text, "-5 度不是列表");
    }

    /// 列表标记的三种写法与有序列表的回退行为。
    #[test]
    fn blocks_accept_all_unordered_markers_and_keep_ordered_literal() {
        let blocks = parse_notes_blocks("* 星号\n+ 加号\n1. 有序\n");
        assert_eq!(blocks.len(), 3, "得到 {blocks:?}");
        assert_eq!(blocks[0], NoteBlock { kind: NoteKind::Bullet, text: "星号".into() });
        assert_eq!(blocks[1], NoteBlock { kind: NoteKind::Bullet, text: "加号".into() });
        // 有序列表按普通段落处理：编号原样留在文字里（已知局限，见函数注释）
        assert_eq!(blocks[2], NoteBlock { kind: NoteKind::Paragraph, text: "1. 有序".into() });
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

    // ── 系统代理（Ruling 89）────────────────────────────────────────────────

    /// 形状 ①：一个地址管所有协议 —— Ruling 89 实测的本机取值就是这个形状
    /// （`127.0.0.1:12450`）。必须**原样**返回，不能按 `:` 去切。
    #[test]
    fn parse_proxy_server_passes_through_single_address() {
        assert_eq!(parse_proxy_server("127.0.0.1:12450").as_deref(), Some("127.0.0.1:12450"));
        assert_eq!(parse_proxy_server("  proxy.corp:8080  ").as_deref(), Some("proxy.corp:8080"));
        // IPv6 字面量全是冒号 —— 只按 `=` 判形状的理由就在这一行
        assert_eq!(parse_proxy_server("[::1]:7890").as_deref(), Some("[::1]:7890"));
    }

    /// ★ 判别性：形状 ②（按协议分列）必须挑 **https**，不是顺手取第一项。
    ///
    /// 它捕获的变异：把 `return Some(val)` 改成"取第一个非空项" —— 那样
    /// `http=` 在前时会被当成 https 的代理用，而本程序只出 https。
    /// 本机正好撞不上这个 bug（单地址形状），所以只有这条测试拦得住。
    #[test]
    fn parse_proxy_server_prefers_https_over_http_in_a_protocol_list() {
        let raw = "http=10.0.0.1:8080;https=10.0.0.2:8443;ftp=10.0.0.3:21";
        assert_eq!(parse_proxy_server(raw).as_deref(), Some("10.0.0.2:8443"));
        // 大小写与空格都不得影响判定（注册表里的键名不保证大小写）
        assert_eq!(
            parse_proxy_server("HTTP=a:1; HTTPS = b:2").as_deref(),
            Some("b:2")
        );
        // 顺序反过来也必须还是 https
        assert_eq!(
            parse_proxy_server("https=b:2;http=a:1").as_deref(),
            Some("b:2")
        );
    }

    /// 分列但**没有 https 项**时回落 `http=`；两者都没有则 `None`（→ 直连）。
    #[test]
    fn parse_proxy_server_falls_back_to_http_then_none() {
        assert_eq!(parse_proxy_server("http=a:1;ftp=c:3").as_deref(), Some("a:1"));
        assert_eq!(parse_proxy_server("ftp=c:3;socks=d:4"), None, "没有 http/https 项");
        assert_eq!(parse_proxy_server("https=;http="), None, "空值不算数");
    }

    #[test]
    fn parse_proxy_server_rejects_empty() {
        assert_eq!(parse_proxy_server(""), None);
        assert_eq!(parse_proxy_server("   "), None);
    }

    /// FFI 冒烟：`WinHttpGetIEProxyConfigForCurrentUser` 那套注册表读取不崩、
    /// 不吃到空指针、两次调用自洽。
    ///
    /// ⚠ 与 `theme::system_dark_is_callable_and_stable` 同款，**不能**断言具体值
    /// —— 值取决于跑测试这台机器的代理设置（CI 上多半是"没配代理"）。
    /// 它能抓住的是句柄成对释放之外的东西：字段偏移写错、`REG_SZ` 判定写反、
    /// `lpcbData` 当字符数用 —— 这些要么直接崩，要么返回垃圾。
    #[cfg(windows)]
    #[test]
    fn system_proxy_is_callable_and_stable() {
        let a = system_proxy();
        let b = system_proxy();
        assert_eq!(a.is_some(), b.is_some(), "同一时刻两次探测必须一致");
        // 真读到了就必须是个能建出来的代理（解析不出 scheme 的那条回落在内部兜住）
        if let Some(p) = a {
            // ⚠ `ureq::Proxy` **没有** Debug（源码里只 derive 了 Clone/Eq/Hash/PartialEq），
            // 所以这里断言的是它的 `uri()` —— 这也正是唯一值得断言的东西：
            // 真读到了就必然是一个带 host 的地址（`Proxy::new` 在无 authority 时
            // 直接 `Err(InvalidProxyUrl)`，那条路上本函数返回 `None`、不会走到这里）。
            assert!(
                p.uri().host().is_some_and(|h| !h.is_empty()),
                "解析出的代理地址没有 host: {}",
                p.uri()
            );
        }
    }
}


