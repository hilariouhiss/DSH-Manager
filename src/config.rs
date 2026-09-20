//! state.json 的 I/O 层。覆盖 SRS FR-30 / FR-31 / FR-32。
//! 只做 I/O，**不含"何时该写"的决策** —— 写入时机由调用方决定。

use std::path::{Path, PathBuf};

#[derive(Default, Clone, Debug, PartialEq)]
pub struct StateFile {
    pub preferred_port: Option<u16>,
    pub running_port: Option<u16>,
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

pub fn update(f: impl FnOnce(&mut StateFile)) -> Result<(), String> {
    let mut s = match load() {
        Loaded::Ok(s) => s,
        // FR-32：损坏或缺失都不应让写入路径失败，从缺省值开始
        _ => StateFile::default(),
    };
    f(&mut s);
    save(&s)
}

/// 读—改—写的测试缝。
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
        let s = StateFile { preferred_port: Some(8080), running_port: Some(9090) };
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
        save_to(&p, &StateFile { preferred_port: Some(1), running_port: None }).unwrap();
        let leftover = p.with_extension("json.tmp");
        assert!(!leftover.exists(), "原子写入不得残留 .tmp 文件");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn update_preserves_other_field() {
        let p = tmp("update.json");
        save_to(&p, &StateFile { preferred_port: Some(3080), running_port: None }).unwrap();
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

        let baseline = StateFile { preferred_port: Some(3080), running_port: None };
        save_to(&p, &baseline).unwrap();

        // 占住 .tmp 路径，使"写临时文件"必然失败
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let different = StateFile { preferred_port: Some(9999), running_port: Some(1) };
        assert!(save_to(&p, &different).is_err(), "写 .tmp 失败时 save_to 必须报错");

        match load_from(Some(&p)) {
            Loaded::Ok(s) => assert_eq!(s, baseline, "写入失败时目标文件必须原封不动"),
            other => panic!("目标被破坏，期望 Ok(baseline)，得到 {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&tmp_dir);
        let _ = std::fs::remove_file(&p);
    }
}
