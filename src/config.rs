//! state.json 的 I/O 层。覆盖 SRS FR-30 / FR-31 / FR-32。
//! 只做 I/O，**不含"何时该写"的决策** —— 写入时机由调用方决定。

use std::path::{Path, PathBuf};

use crate::theme::ThemeMode;

/// 关闭主窗口时的行为。FR-23 修订版：**首次关闭时询问一次**，选中的结果
/// 记在这里，之后可在"设置"里随时改。
///
/// ⚠ 下标即 UI 契约：`ui/app.slint` 的 `close-behavior-index` 用 0/1/2 表示
/// 这三档，两边必须一起改。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum CloseBehavior {
    Hide,
    Quit,
    /// 每次关闭都问一遍。也是 state.json 里**没记过**时的缺省值 ——
    /// 于是"首次关闭"这条路径不需要任何额外标志：没值就是问。
    #[default]
    Ask,
}

impl CloseBehavior {
    pub fn index(self) -> i32 {
        match self {
            CloseBehavior::Hide => 0,
            CloseBehavior::Quit => 1,
            CloseBehavior::Ask => 2,
        }
    }

    pub fn from_index(i: i32) -> Option<Self> {
        match i {
            0 => Some(CloseBehavior::Hide),
            1 => Some(CloseBehavior::Quit),
            2 => Some(CloseBehavior::Ask),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            CloseBehavior::Hide => "hide",
            CloseBehavior::Quit => "quit",
            CloseBehavior::Ask => "ask",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "hide" => Some(CloseBehavior::Hide),
            "quit" => Some(CloseBehavior::Quit),
            "ask" => Some(CloseBehavior::Ask),
            _ => None,
        }
    }
}

#[derive(Default, Clone, Debug, PartialEq)]
pub struct StateFile {
    pub preferred_port: Option<u16>,
    pub running_port: Option<u16>,
    /// `None` = 从没问过（首次关闭要弹询问框）。见 `CloseBehavior`。
    pub close_behavior: Option<CloseBehavior>,
    /// `None` = 从没设过 = 跟随系统。见 `ThemeMode`。
    pub theme_mode: Option<ThemeMode>,
    /// 出网是否走 Windows 系统代理。`None` = 从没设过 = **走**（见 `use_system_proxy()`）。
    pub use_system_proxy: Option<bool>,
    /// FR-38：用户点过〔忽略此版本〕的**本程序**版本（如 `0.3.0`）。`None` = 没忽略过。
    ///
    /// ⚠ 存字符串而不是 `Version`：这一项只用于"是不是同一个版本"的相等比较，
    /// 解析失败（手动改坏、将来降级运行）当"没忽略过"就够 —— 为它引入一条
    /// 解析失败路径不划算。也**不**与 dsh 的版本混用（那是 registry 的事）。
    pub skipped_app_version: Option<String>,
    /// FR-8 v1.8 修订：要检测哪些通道的更新（`["alpha","rc","stable"]` 的子集）。
    ///
    /// ⚠ 存**裸字符串数组**而不是 `model::Channels`：`config.rs` 的边界是"纯 I/O、
    /// 不含业务逻辑"（SRS §9.1），名字 ↔ 通道的映射归 `model::Channels` 自己管。
    /// ⚠ `None`（没这个 key —— 旧版 state.json）= **全勾**；空数组 = 用户明确一个都不勾。
    /// 两者必须分得开：丢失这个区分，用户在设置里取消勾选之后重启会被悄悄改回全勾。
    pub update_channels: Option<Vec<String>>,
}

#[derive(Debug)]
pub enum Loaded {
    Ok(StateFile),
    Missing,
    Corrupt(String),
    NoLocation,
}

/// 读取的测试缝：`None` 表示路径不可用。
pub fn load_from(path: Option<&Path>) -> Loaded {
    let Some(p) = path else {
        return Loaded::NoLocation;
    };
    let text = match std::fs::read_to_string(p) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded::Missing,
        Err(e) => return Loaded::Corrupt(format!("读取失败: {e}")),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Loaded::Corrupt(format!("JSON 解析失败: {e}")),
    };
    let read_port = |key: &str| -> Option<u16> {
        v.get(key)?.as_u64().and_then(|n| u16::try_from(n).ok())
    };
    Loaded::Ok(StateFile {
        preferred_port: read_port("preferred_port"),
        running_port: read_port("running_port"),
        // ⚠ 认不出的值（手改过、将来降级运行）一律当"没记过"处理，
        // 也就是回到"每次询问"—— 比替用户猜一个行为安全。
        close_behavior: v
            .get("close_behavior")
            .and_then(|x| x.as_str())
            .and_then(CloseBehavior::parse),
        theme_mode: v
            .get("theme_mode")
            .and_then(|x| x.as_str())
            .and_then(ThemeMode::parse),
        // 同款：认不出的值（缺 key、手改成字符串、将来降级运行）一律当"没记过"，
        // 也就是回到缺省档（走系统代理），而不是替用户猜成直连。
        use_system_proxy: v.get("use_system_proxy").and_then(|x| x.as_bool()),
        // 同款：没有这个 key（旧版 state.json）或不是字符串 → "没忽略过"。
        skipped_app_version: v
            .get("skipped_app_version")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        // FR-8 v1.8：不是数组 → `None` = 没记过 = 全勾（比"一个都不勾"安全：
        // 用户会看到更新提示，而不是从此再也收不到）。数组里的非字符串项直接丢掉。
        update_channels: v.get("update_channels").and_then(|x| x.as_array()).map(|a| {
            a.iter()
                .filter_map(|s| s.as_str())
                .map(str::to_string)
                .collect()
        }),
    })
}

pub fn load() -> Loaded {
    load_from(state_path().as_deref())
}

/// 出网是否走系统代理。**没记过 / 文件缺失 / 文件损坏一律返回 `true`。**
///
/// ⚠ 缺省是"走"而不是"直连"，理由有两条：
/// - 与 `theme_mode` 的"没值就是缺省档"同一约定。而在装了没配代理的机器上
///   `ProxyEnable` 是 0，这一档自动退化成直连 —— 缺省选它的**代价是零**；
/// - 反过来（缺省直连）等于把 Ruling 89 那个"只有本程序出不去网、用户完全
///   无从判断原因"的坑，原样留给每一个没手动改过设置的人。
///
/// 消费者是 `dsh::agent()`，它跑在 **worker 线程**上，所以这里直接读文件而不是
/// 经 AppState 传值：设置面板点完，下一次请求就生效，不需要重启；也就不存在
/// 第二份真相需要同步（这正是"回调置 dirty → project() 推回 UI"那条路做不到的，
/// 它只覆盖 UI 线程）。
///
/// 每次出网读一次 state.json 是刻意的懒：本程序一个会话里最多几次请求
/// （目录一次、说明每版本一次且缓存一周），这点 I/O 远小于一次 TLS 握手。
pub fn use_system_proxy() -> bool {
    match load() {
        Loaded::Ok(s) => s.use_system_proxy.unwrap_or(true),
        _ => true,
    }
}

/// FR-38：这个版本是不是被用户点过〔忽略此版本〕。
///
/// ⚠ 单独成一个函数而不是让调用方自己 `load()`：判据里那个"读不到就算没忽略过"
/// 的分支（文件缺失/损坏/没这个 key）是**行为契约**的一部分 —— 它决定"更新还提不提示"，
/// 值得有一处明确的实现与一条测试。
pub fn app_update_skipped(version: &str) -> bool {
    matches!(load(), Loaded::Ok(s) if s.skipped_app_version.as_deref() == Some(version))
}

pub fn state_path() -> Option<PathBuf> {
    // GC-2：不引入 dirs crate，一个 APPDATA 路径不值一个依赖
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("dsh-manager").join("state.json"))
}

pub fn save(s: &StateFile) -> Result<(), String> {
    match state_path() {
        Some(p) => save_to(&p, s),
        None => Err("APPDATA 不可用".into()),
    }
}

/// 原子写入：写 `.tmp` → rename。
///
/// **必须**走这条路，不得直接截断目标文件。
/// 依据：FR-31 要应对的核心场景是管理器被强杀，而直接截断写入时进程若在
/// 写入中途终止，state.json 会变成半截 JSON。也就是说 —— 最需要持久化生效
/// 的场景，恰恰是朴素写入最容易毁掉数据的场景。文件一坏，下次启动回落缺省
/// 端口，孤儿就失联了。
///
/// fs::rename 的覆盖语义已核实：Rust 文档明确 "replacing the original file
/// if `to` already exists"，Windows 上通过 MoveFileExW 实现。
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("写临时文件失败: {e}")
    })?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("替换失败: {e}")
    })
}

/// 原子写入的测试缝（state.json）。
pub fn save_to(path: &Path, s: &StateFile) -> Result<(), String> {
    let json = serde_json::json!({
        "preferred_port": s.preferred_port,
        "running_port": s.running_port,
        "close_behavior": s.close_behavior.map(CloseBehavior::as_str),
        "theme_mode": s.theme_mode.map(ThemeMode::as_str),
        "use_system_proxy": s.use_system_proxy,
        "skipped_app_version": s.skipped_app_version,
        "update_channels": s.update_channels,
    });
    let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    write_atomic(path, &text)
}

// ── 更新说明缓存（FR-27 修订）────────────────────────────────────────────────
//
// 需求：**同一个版本的更新说明只拉一次**，一周内不再联网；过期或没缓存过才去拉。
// 单独一个文件（notes-cache.json），不塞进 state.json：说明正文是 KB 级的，
// 而 state.json 每改一次端口就要整份重写 —— 混在一起等于每次改端口都顺带重写几 KB。

/// 缓存有效期：一周。过了就当没有，重新拉。
pub const NOTES_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// 缓存里最多留几个版本。说明正文是 KB 级，八个不同版本也就几十 KB；
/// 超出时丢最旧的 —— 这是个缓存，不是档案。
const NOTES_CACHE_MAX: usize = 8;

pub fn notes_cache_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("dsh-manager").join("notes-cache.json"))
}

/// Unix 秒。取不到系统时间（时钟早于 1970）时返回 0 —— 那会让所有条目都判为过期，
/// 即"重新拉一次"，这是安全的失败方向。
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 条目是否还在有效期内。⚠ 纯函数：TTL 的边界（差一秒、正好到点）只有把它
/// 从系统时钟里摘出来才测得住。
pub fn is_fresh(fetched_at: u64, now: u64) -> bool {
    // ⚠ 用 saturating_sub：时钟被往回调（或 fetched_at 来自"未来"）时，
    // now - fetched_at 会下溢 —— 在 debug 构建里那是 panic，不是"过期"。
    now.saturating_sub(fetched_at) < NOTES_TTL_SECS
}

/// 取某个版本的说明正文（仅当缓存命中且未过期）。
///
/// 任何异常（文件缺失、损坏、字段类型不对）都返回 None —— 调用方据此去联网拉，
/// 也就是"缓存坏了最多多拉一次"，绝不因为缓存本身出问题而报错。
pub fn load_notes(version: &str) -> Option<String> {
    load_notes_from(notes_cache_path()?.as_path(), version, now_unix())
}

pub fn load_notes_from(path: &Path, version: &str, now: u64) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let entry = v.get(version)?;
    let at = entry.get("at")?.as_u64()?;
    if !is_fresh(at, now) {
        return None;
    }
    entry.get("body")?.as_str().map(str::to_string)
}

/// 写入一条缓存，并顺手清掉过期条目、裁到上限。
pub fn store_notes(version: &str, body: &str) -> Result<(), String> {
    match notes_cache_path() {
        Some(p) => store_notes_to(&p, version, body, now_unix()),
        None => Err("APPDATA 不可用".into()),
    }
}

pub fn store_notes_to(path: &Path, version: &str, body: &str, now: u64) -> Result<(), String> {
    let existing = std::fs::read_to_string(path).ok();
    let mut map: serde_json::Map<String, serde_json::Value> = existing
        .as_deref()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    map.insert(version.to_string(), serde_json::json!({ "at": now, "body": body }));

    // 过期条目直接丢：留着也不会被采用，只是占地方。
    map.retain(|_, e| e.get("at").and_then(|a| a.as_u64()).is_some_and(|a| is_fresh(a, now)));
    // 仍超上限时丢最旧的几个（at 最小的）。
    while map.len() > NOTES_CACHE_MAX {
        let oldest = map
            .iter()
            .min_by_key(|(_, e)| e.get("at").and_then(|a| a.as_u64()).unwrap_or(0))
            .map(|(k, _)| k.clone());
        match oldest {
            Some(k) => {
                map.remove(&k);
            }
            None => break,
        }
    }

    let text = serde_json::to_string(&serde_json::Value::Object(map)).map_err(|e| e.to_string())?;
    write_atomic(path, &text)
}

/// 串行化 `update` 的"读—改—写"。进程内全局锁，够用 —— 只有本程序写这个文件。
static UPDATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn update(f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
    // ⚠ 三个线程都会调 update（UI 线程、worker、启动时的孤儿探测线程），而它是
    // 「读—改—写」：先 `load()` 出快照，改完再整体 `save()`。不加锁时交错的两个
    // 写入方会各自基于自己读到的**旧快照**写回，后写的一方于是抹掉前一方刚写入的
    // 字段 —— 例如别的线程刚写好的 `running_port`。那正是 FR-31 要防的静默失联：
    // 运行态端口丢了，下次启动就再也认不出孤儿进程。
    //
    // 中毒不能把程序带下去：这把锁保护的是文件 I/O，而不是某个必须成立的不变量，
    // 前一个持锁者 panic 之后文件本身仍然是完整的（save 走临时文件 + rename）。
    // 所以中毒时取回内部值继续用，而不是 unwrap。
    let _guard = UPDATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut s = match load() {
        Loaded::Ok(s) => s,
        // FR-32：损坏或缺失都不应让写入路径失败，从缺省值开始
        _ => StateFile::default(),
    };
    f(&mut s);
    save(&s)
}

/// 读—改—写的测试缝。
///
/// ⚠ `#[cfg(test)]` 是 Task 20 加的：本函数**确实只有测试调用**（生产路径一律走
/// `update`，它自己带 `UPDATE_LOCK`；task-18-report 已把"本函数故意不加锁、只作
/// 测试缝"记录在案）。没有这个门，`cargo build` 会报 dead_code —— 而按本任务的
/// 要求，真实死代码只能删除或接线，**不得**恢复 `allow`。
#[cfg(test)]
pub fn update_at(path: &Path, f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
    let mut s = match load_from(Some(path)) {
        Loaded::Ok(s) => s,
        _ => StateFile::default(),
    };
    f(&mut s);
    save_to(path, &s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeMode;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dsh-mgr-test-{name}-{}", std::process::id()))
    }

    #[test]
    fn missing_file_yields_missing_not_error() {
        let p = tmp("missing.json");
        let _ = std::fs::remove_file(&p);
        assert!(matches!(load_from(Some(&p)), Loaded::Missing));
    }

    #[test]
    fn corrupt_file_yields_corrupt_with_reason() {
        let p = tmp("corrupt.json");
        std::fs::write(&p, "{ this is not json").unwrap();
        match load_from(Some(&p)) {
            Loaded::Corrupt(msg) => assert!(!msg.is_empty(), "应带上原因"),
            other => panic!("期望 Corrupt，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn parses_both_fields() {
        let p = tmp("both.json");
        std::fs::write(&p, r#"{"preferred_port":8080,"running_port":9090}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.preferred_port, Some(8080));
                assert_eq!(s.running_port, Some(9090));
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn absent_fields_are_none_not_default() {
        // FR-31：running_port 为 None 本身就是有意义的信息（没有本程序启动的实例）
        let p = tmp("empty-obj.json");
        std::fs::write(&p, "{}").unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.preferred_port, None);
                assert_eq!(s.running_port, None);
                // ⚠ 这一条是"首次关闭要询问"的立身之本：没记过 ≠ 默认隐藏。
                assert_eq!(s.close_behavior, None, "没记过关闭行为时必须回到“每次询问”");
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// 判别性测试：认不出的 close_behavior 必须当"没记过"处理。
    ///
    /// 它钉住的是"**绝不替用户猜**"：手改过、或将来降级运行读到新版本写的值时，
    /// 未知值只能回到"每次询问"，不能猜成隐藏或退出。下面把三个合法值也各读一遍 ——
    /// 否则"未知值 → None"这条断言在"解析整个坏掉、永远返回 None"的实现下会假通过。
    #[test]
    fn unknown_close_behavior_falls_back_to_ask() {
        let p = tmp("unknown-behavior.json");
        std::fs::write(&p, r#"{"close_behavior":"nuke-the-site"}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.close_behavior, None),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        for (text, want) in [
            ("hide", CloseBehavior::Hide),
            ("quit", CloseBehavior::Quit),
            ("ask", CloseBehavior::Ask),
        ] {
            std::fs::write(&p, format!(r#"{{"close_behavior":"{text}"}}"#)).unwrap();
            match load_from(Some(&p)) {
                Loaded::Ok(s) => assert_eq!(s.close_behavior, Some(want), "{text} 应被读回"),
                other => panic!("期望 Ok，得到 {other:?}"),
            }
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn close_behavior_survives_update() {
        // 设置面板改的就是这一项，而 update 是"读—改—写"：
        // 改关闭行为绝不能顺手抹掉运行态端口（反之亦然）。
        let p = tmp("behavior-update.json");
        save_to(
            &p,
            &StateFile {
                preferred_port: Some(3080),
                running_port: Some(8080),
                close_behavior: None,
                theme_mode: None,
                use_system_proxy: None,
                skipped_app_version: None,
                update_channels: None,
            },
        )
        .unwrap();
        update_at(&p, |s| s.close_behavior = Some(CloseBehavior::Hide)).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.close_behavior, Some(CloseBehavior::Hide));
                assert_eq!(s.running_port, Some(8080), "改关闭行为不得丢失运行态端口");
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn no_location_when_path_unavailable() {
        assert!(matches!(load_from(None), Loaded::NoLocation));
    }

    #[test]
    fn save_then_load_roundtrip() {
        let p = tmp("roundtrip.json");
        let s = StateFile {
            preferred_port: Some(8080),
            running_port: Some(9090),
            close_behavior: Some(CloseBehavior::Quit),
            theme_mode: None,
            use_system_proxy: None,
            skipped_app_version: None,
            update_channels: None,
        };
        save_to(&p, &s).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(got) => assert_eq!(got, s),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn save_creates_parent_directory() {
        let dir = tmp("mkdir");
        let p = dir.join("nested").join("state.json");
        let _ = std::fs::remove_dir_all(&dir);
        save_to(&p, &StateFile::default()).unwrap();
        assert!(p.is_file(), "应自动创建父目录");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_leaves_no_tmp_file_behind() {
        let p = tmp("notmp.json");
        let s = StateFile {
            preferred_port: Some(1),
            running_port: None,
            close_behavior: None,
            theme_mode: None,
            use_system_proxy: None,
            skipped_app_version: None,
            update_channels: None,
        };
        save_to(&p, &s).unwrap();
        let leftover = p.with_extension("json.tmp");
        assert!(!leftover.exists(), "原子写入不得残留 .tmp 文件");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_preserves_other_field() {
        let p = tmp("update.json");
        let s = StateFile {
            preferred_port: Some(3080),
            running_port: None,
            close_behavior: None,
            theme_mode: None,
            use_system_proxy: None,
            skipped_app_version: None,
            update_channels: None,
        };
        save_to(&p, &s).unwrap();
        update_at(&p, |s| s.running_port = Some(8080)).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.preferred_port, Some(3080), "改 running 不得丢失 preferred");
                assert_eq!(s.running_port, Some(8080));
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_on_corrupt_file_starts_from_default() {
        // FR-32：损坏文件不应让写入路径也失败
        let p = tmp("update-corrupt.json");
        std::fs::write(&p, "garbage").unwrap();
        update_at(&p, |s| s.preferred_port = Some(3080)).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.preferred_port, Some(3080)),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// 判别性测试：唯一能区分"原子写入"与"朴素截断写入"的测试。
    ///
    /// 手法：把 .tmp 的路径占成目录，逼 `save_to` 在写临时文件这一步失败，
    /// 然后断言**目标文件分毫未动**。
    ///
    /// 它捕获的变异：把 `save_to` 里"写 .tmp → rename"整段换成
    /// `std::fs::write(path, text)`（朴素截断写入）。
    ///   - 原子实现：写 .tmp 失败 → 目标从未被碰过 → 仍是 baseline → 通过
    ///   - 朴素实现：直接写进目标 → 返回 Ok 且目标变成 different → 失败
    ///
    /// 这正是 FR-31 的立身之本：写入失败绝不能毁掉已有的 state.json，
    /// 否则下次启动回落缺省端口，孤儿即失联。不要弱化本测试。
    #[test]
    fn failed_write_leaves_target_intact() {
        let p = tmp("atomic-swap.json");
        let tmp_dir = p.with_extension("json.tmp");
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_dir_all(&tmp_dir);

        let baseline = StateFile {
            preferred_port: Some(3080),
            running_port: None,
            close_behavior: None,
            theme_mode: None,
            use_system_proxy: None,
            skipped_app_version: None,
            update_channels: None,
        };
        save_to(&p, &baseline).unwrap();

        // 占住 .tmp 路径，使"写临时文件"必然失败
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let different = StateFile {
            preferred_port: Some(9999),
            running_port: Some(1),
            close_behavior: None,
            theme_mode: None,
            use_system_proxy: None,
            skipped_app_version: None,
            update_channels: None,
        };
        assert!(save_to(&p, &different).is_err(), "写 .tmp 失败时 save_to 必须报错");

        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s, baseline, "写入失败时目标文件必须原封不动"),
            other => panic!("目标被破坏，期望 Ok(baseline)，得到 {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp_dir);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn theme_mode_roundtrips_through_json() {
        let p = tmp("theme.json");
        let s = StateFile {
            preferred_port: None,
            running_port: None,
            close_behavior: None,
            theme_mode: Some(ThemeMode::Dark),
            use_system_proxy: None,
            skipped_app_version: None,
            update_channels: None,
        };
        save_to(&p, &s).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(got) => assert_eq!(got.theme_mode, Some(ThemeMode::Dark)),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// ⚠ 兼容性：旧版 state.json 只有三个字段。缺 key 必须读出 `None`
    /// （= 跟随系统），**不得**当成损坏、也不得回落成某个固定主题。
    #[test]
    fn legacy_state_without_theme_mode_reads_as_none() {
        let p = tmp("legacy.json");
        std::fs::write(
            &p,
            r#"{"preferred_port": 3080, "running_port": null, "close_behavior": "quit"}"#,
        )
        .unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => {
                assert_eq!(s.theme_mode, None, "缺 key 应为 None（跟随系统）");
                assert_eq!(s.preferred_port, Some(3080), "旧字段不得受影响");
                assert_eq!(s.close_behavior, Some(CloseBehavior::Quit));
            }
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn unrecognized_theme_mode_reads_as_none() {
        let p = tmp("badtheme.json");
        std::fs::write(&p, r#"{"theme_mode": "MINT"}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.theme_mode, None),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// 保存必须落 `theme_mode` 这个 key —— 否则"选了深色重启还是跟随系统"，
    /// 而所有内存测试都看不出来。
    #[test]
    fn save_writes_theme_mode_key() {
        let p = tmp("writetheme.json");
        let s = StateFile {
            preferred_port: None,
            running_port: None,
            close_behavior: None,
            theme_mode: Some(ThemeMode::Light),
            use_system_proxy: None,
            skipped_app_version: None,
            update_channels: None,
        };
        save_to(&p, &s).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v.get("theme_mode").and_then(|x| x.as_str()), Some("light"));
        let _ = std::fs::remove_file(&p);
    }

    /// `use_system_proxy` 的读取契约。
    ///
    /// ⚠ 缺 key 必须是 `None`（= 没记过）而**不是** `Some(false)`：`None` 才代表
    /// "用户可以什么都没选过"，`use_system_proxy()` 据此回落到缺省档（走系统代理）。
    /// 若这里读成 `Some(false)`，"没记过"和"用户明确选了直连"就再也分不开 ——
    /// 而两者的用户意图正好相反。
    #[test]
    fn use_system_proxy_absent_is_none_and_explicit_values_roundtrip() {
        let p = tmp("proxy.json");
        std::fs::write(&p, r#"{"preferred_port":3080}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(
                s.use_system_proxy, None,
                "缺 key 必须是 None（没记过），不能替用户猜成直连"
            ),
            other => panic!("期望 Ok，得到 {other:?}"),
        }

        for v in [true, false] {
            save_to(&p, &StateFile { use_system_proxy: Some(v), ..Default::default() }).unwrap();
            match load_from(Some(&p)) {
                Loaded::Ok(s) => assert_eq!(s.use_system_proxy, Some(v), "{v} 应被原样读回"),
                other => panic!("期望 Ok，得到 {other:?}"),
            }
        }

        // ⚠ 落盘的必须是**真布尔**而不是字符串。写成 `"false"` 时 `as_bool()` 读回
        // `None` → 又回落到"走代理" —— 用户关掉开关、重启后又自己打开了，
        // 而上面那圈往返测试在写入侧也用字符串时会**照样全绿**。
        save_to(&p, &StateFile { use_system_proxy: Some(false), ..Default::default() }).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("\"use_system_proxy\": false"), "落盘形状不对: {text}");
        let _ = std::fs::remove_file(&p);
    }

    /// FR-38：被忽略的那个本程序版本必须活过一次重启。
    ///
    /// ⚠ 这条的**用户可见后果**才是它的价值：读不回来就等于"忽略"没生效，
    /// 于是每次开机都弹同一个更新框 —— 而"忽略此版本"存在的全部理由就是不弹。
    #[test]
    fn skipped_app_version_roundtrips_and_absent_is_none() {
        let p = tmp("skipped-app.json");

        // 旧版 state.json：没有这个 key，必须读成"没忽略过"，且**不得**判为损坏
        std::fs::write(&p, r#"{"preferred_port":3080}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.skipped_app_version, None),
            other => panic!("缺 key 不是损坏。期望 Ok，得到 {other:?}"),
        }

        save_to(
            &p,
            &StateFile { skipped_app_version: Some("0.3.0".into()), ..Default::default() },
        )
        .unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.skipped_app_version.as_deref(), Some("0.3.0")),
            other => panic!("期望 Ok，得到 {other:?}"),
        }

        // 手改成数字之类的非字符串 → 同样当"没忽略过"，不 panic、不判损坏
        std::fs::write(&p, r#"{"skipped_app_version":300}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.skipped_app_version, None, "非字符串一律当没忽略过"),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// FR-8 v1.8：通道勾选的落盘往返 —— 并且**空数组 ≠ 没记过**。
    ///
    /// ⚠ 这条测试钉的是"用户取消勾选之后重启会不会被改回全勾"：
    /// `None`（旧 state.json / 手改成非数组）读成"没记过"→ 由调用方当全勾；
    /// `[]` 读成"一个都不勾"→ 必须原样保留。
    #[test]
    fn update_channels_roundtrip_and_absent_is_none() {
        let p = tmp("channels.json");

        // 旧版 state.json：没有这个 key —— 读成 None（= 没记过 = 全勾），且不得判为损坏
        std::fs::write(&p, r#"{"preferred_port":3080}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.update_channels, None),
            other => panic!("缺 key 不是损坏。期望 Ok，得到 {other:?}"),
        }

        // 明确一个都不勾：必须读回空数组，不能变成 None
        save_to(&p, &StateFile { update_channels: Some(vec![]), ..Default::default() }).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s.update_channels, Some(vec![]), "空数组 ≠ 没记过"),
            other => panic!("期望 Ok，得到 {other:?}"),
        }

        save_to(
            &p,
            &StateFile {
                update_channels: Some(vec!["alpha".into(), "stable".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(
                s.update_channels,
                Some(vec!["alpha".to_string(), "stable".to_string()])
            ),
            other => panic!("期望 Ok，得到 {other:?}"),
        }

        // 手改成字符串 / 数字 → 当"没记过"（= 全勾），不 panic、不判损坏
        for bad in [r#"{"update_channels":"alpha"}"#, r#"{"update_channels":3}"#] {
            std::fs::write(&p, bad).unwrap();
            match load_from(Some(&p)) {
                Loaded::Ok(s) => assert_eq!(s.update_channels, None, "{bad} 应读成没记过"),
                other => panic!("期望 Ok，得到 {other:?}"),
            }
        }

        // 数组里混进非字符串项：丢掉那一项，保留认得出的
        std::fs::write(&p, r#"{"update_channels":["alpha",7,null,"rc"]}"#).unwrap();
        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(
                s.update_channels,
                Some(vec!["alpha".to_string(), "rc".to_string()])
            ),
            other => panic!("期望 Ok，得到 {other:?}"),
        }
        let _ = std::fs::remove_file(&p);
    }

    /// 认不出的类型（手改过、将来降级运行）当"没记过" ——
    /// 与 `close_behavior` / `theme_mode` 同判据，不替用户做决定。
    #[test]
    fn non_bool_use_system_proxy_reads_as_none() {
        let p = tmp("badproxy.json");
        for bad in [r#"{"use_system_proxy": "yes"}"#, r#"{"use_system_proxy": 1}"#] {
            std::fs::write(&p, bad).unwrap();
            match load_from(Some(&p)) {
                Loaded::Ok(s) => assert_eq!(s.use_system_proxy, None, "{bad} 应读成 None"),
                other => panic!("期望 Ok，得到 {other:?}"),
            }
        }
        let _ = std::fs::remove_file(&p);
    }
}

#[cfg(test)]
mod notes_cache_tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dsh-mgr-notes-{name}-{}", std::process::id()))
    }

    /// 判别性测试：TTL 的**边界**。它捕获的变异是把 `<` 写成 `<=`（或反过来）——
    /// 那种偏差在真实使用里要等一整周才看得出来。
    #[test]
    fn ttl_boundary_is_exclusive() {
        let t = 1_000_000;
        assert!(is_fresh(t, t), "刚写入的就是新鲜的");
        assert!(is_fresh(t, t + NOTES_TTL_SECS - 1), "差一秒还没过期");
        assert!(!is_fresh(t, t + NOTES_TTL_SECS), "正好到点就算过期");
        assert!(!is_fresh(t, t + NOTES_TTL_SECS + 1));
    }

    /// 时钟被往回调 / 条目来自"未来"时不能 panic（debug 构建里减法下溢就是 panic）。
    #[test]
    fn clock_going_backwards_is_not_a_panic() {
        assert!(is_fresh(2_000_000, 1_000_000));
        assert!(is_fresh(0, 0));
    }

    #[test]
    fn store_then_load_roundtrip() {
        let p = tmp("roundtrip.json");
        let _ = std::fs::remove_file(&p);
        store_notes_to(&p, "1.2.3", "正文 A", 500).unwrap();
        assert_eq!(load_notes_from(&p, "1.2.3", 500).as_deref(), Some("正文 A"));
        let _ = std::fs::remove_file(&p);
    }

    /// 需求的原话是"拉取一次后缓存，直到一周后失效"：过期后必须**读不到**，
    /// 这样调用方才会重新去拉。
    #[test]
    fn expired_entry_is_not_served() {
        let p = tmp("expired.json");
        let _ = std::fs::remove_file(&p);
        store_notes_to(&p, "1.2.3", "正文 A", 500).unwrap();
        assert!(load_notes_from(&p, "1.2.3", 500 + NOTES_TTL_SECS).is_none());
        // 而且写入新条目时会被顺手清掉，文件不会无限长大
        store_notes_to(&p, "9.9.9", "正文 B", 500 + NOTES_TTL_SECS).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(!text.contains("1.2.3"), "过期条目应在写入时被清掉: {text}");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn unknown_version_and_broken_file_yield_none() {
        let p = tmp("broken.json");
        let _ = std::fs::remove_file(&p);
        assert!(load_notes_from(&p, "1.2.3", 0).is_none(), "文件不存在 → None");
        std::fs::write(&p, "{ 这不是 json").unwrap();
        assert!(load_notes_from(&p, "1.2.3", 0).is_none(), "文件损坏 → None（最多多拉一次）");
        std::fs::write(&p, r#"{"1.2.3":{"at":"昨天","body":42}}"#).unwrap();
        assert!(load_notes_from(&p, "1.2.3", 0).is_none(), "字段类型不对 → None");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn keeps_other_versions_and_caps_size() {
        let p = tmp("cap.json");
        let _ = std::fs::remove_file(&p);
        for i in 0..(NOTES_CACHE_MAX + 3) {
            store_notes_to(&p, &format!("1.0.{i}"), &format!("正文 {i}"), 100 + i as u64).unwrap();
        }
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        let map = v.as_object().unwrap();
        assert_eq!(map.len(), NOTES_CACHE_MAX, "应裁到上限");
        // 留下的必须是最新的几个：最新的那条一定在
        assert!(map.contains_key(&format!("1.0.{}", NOTES_CACHE_MAX + 2)));
        assert!(!map.contains_key("1.0.0"), "最旧的应被丢掉");
        let _ = std::fs::remove_file(&p);
    }
}
