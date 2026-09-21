//! state.json 的 I/O 层。覆盖 SRS FR-30 / FR-31 / FR-32。
//! 只做 I/O，**不含"何时该写"的决策** —— 写入时机由调用方决定。

use std::path::{Path, PathBuf};

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
    })
}

pub fn load() -> Loaded {
    load_from(state_path().as_deref())
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

/// 原子写入的测试缝。
///
/// **必须**走"写 .tmp → rename"，不得直接截断目标文件。
/// 依据：FR-31 要应对的核心场景是管理器被强杀，而直接截断写入时进程若在
/// 写入中途终止，state.json 会变成半截 JSON。也就是说 —— 最需要持久化生效
/// 的场景，恰恰是朴素写入最容易毁掉数据的场景。文件一坏，下次启动回落缺省
/// 端口，孤儿就失联了。
///
/// fs::rename 的覆盖语义已核实：Rust 文档明确 "replacing the original file
/// if `to` already exists"，Windows 上通过 MoveFileExW 实现。
pub fn save_to(path: &Path, s: &StateFile) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let json = serde_json::json!({
        "preferred_port": s.preferred_port,
        "running_port": s.running_port,
        "close_behavior": s.close_behavior.map(CloseBehavior::as_str),
    });
    let text = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;

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
        let s = StateFile { preferred_port: Some(1), running_port: None, close_behavior: None };
        save_to(&p, &s).unwrap();
        let leftover = p.with_extension("json.tmp");
        assert!(!leftover.exists(), "原子写入不得残留 .tmp 文件");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_preserves_other_field() {
        let p = tmp("update.json");
        let s = StateFile { preferred_port: Some(3080), running_port: None, close_behavior: None };
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

        let baseline =
            StateFile { preferred_port: Some(3080), running_port: None, close_behavior: None };
        save_to(&p, &baseline).unwrap();

        // 占住 .tmp 路径，使"写临时文件"必然失败
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let different =
            StateFile { preferred_port: Some(9999), running_port: Some(1), close_behavior: None };
        assert!(save_to(&p, &different).is_err(), "写 .tmp 失败时 save_to 必须报错");

        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s, baseline, "写入失败时目标文件必须原封不动"),
            other => panic!("目标被破坏，期望 Ok(baseline)，得到 {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp_dir);
        let _ = std::fs::remove_file(&p);
    }
}
