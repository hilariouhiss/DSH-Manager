//! 包管理器探测（FR-1 ~ FR-5）、通道判定（FR-7 / FR-8）与命令执行 helper。

use std::path::{Path, PathBuf};

use crate::model::*;

/// FR-7：由 semver 的 pre 字段推导发布通道。
pub fn channel_of(v: &Version) -> Channel {
    if v.pre.is_empty() {
        return Channel::Stable;
    }
    let s = v.pre.as_str();
    if s.starts_with("alpha") {
        Channel::Alpha
    } else if s.starts_with("rc") {
        Channel::Rc
    } else {
        Channel::Other
    }
}

/// FR-8 / GC-14：**只在指定通道内**求最新。
/// 绝不使用 npm 的 `latest` tag —— 它可能比已安装版本更旧。
pub fn latest_in<'a>(versions: &'a [Version], ch: Channel) -> Option<&'a Version> {
    versions.iter().filter(|v| channel_of(v) == ch).max()
}

/// 版本列表降序（最新在前）。FR-10 要求。
pub fn sorted_desc(mut versions: Vec<Version>) -> Vec<Version> {
    versions.sort_by(|a, b| b.cmp(a));
    versions
}

/// 在目录中查找 dsh 的可执行 shim。
pub fn shim_in(dir: &Path) -> Option<PathBuf> {
    SHIM_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|candidate| candidate.is_file())
}

/// 路径归一化比较：Windows 大小写不敏感，且需容忍尾部分隔符。
pub fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// FR-3 第 1~2 步：按 PATH 顺序找出第一个含 dsh shim 的完整路径。
///
/// **顺序即语义** —— 先命中的就是用户敲 `dsh` 时真正执行的那个。
/// 自己扫 PATH 而不用 `where.exe`：无外部进程依赖，且可用假 `exists`
/// 闭包做单元测试（SRS §8.4 要求）。
pub fn find_dsh_on_path(
    path_dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    for dir in path_dirs {
        for name in SHIM_NAMES {
            let candidate = dir.join(name);
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// FR-3 第 2 步：由 dsh shim 所在目录判断 owner PM。
pub fn owner_of(shim: &Path, bins: &[PmInfo]) -> Option<Pm> {
    let parent = shim.parent()?;
    bins.iter()
        .find(|info| same_dir(&info.bin_dir, parent))
        .map(|info| info.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn channel_of_classifies_correctly() {
        assert_eq!(channel_of(&v("0.1.6-alpha.2")), Channel::Alpha);
        assert_eq!(channel_of(&v("0.1.5-rc.2")), Channel::Rc);
        assert_eq!(channel_of(&v("0.1.0")), Channel::Stable);
        assert_eq!(channel_of(&v("0.1.0-beta.1")), Channel::Other);
    }

    /// ★ GC-14 的核心回归测试。
    /// npm 的 latest tag 是 0.1.5-rc.2，比已装的 0.1.6-alpha.2 更旧。
    /// 在 alpha 通道内求"最新"必须得到 0.1.6-alpha.2，绝不能是 0.1.5-rc.2。
    #[test]
    fn latest_in_channel_never_downgrades() {
        let all = vec![v("0.1.5-rc.2"), v("0.1.6-alpha.1"), v("0.1.6-alpha.2")];
        assert_eq!(latest_in(&all, Channel::Alpha), Some(&v("0.1.6-alpha.2")));
        assert_eq!(latest_in(&all, Channel::Rc), Some(&v("0.1.5-rc.2")));
        assert_eq!(latest_in(&all, Channel::Stable), None);
    }

    #[test]
    fn latest_in_rc_ordering_is_semver_not_string() {
        // 字符串序会把 "0.1.10-rc.1" 排在 "0.1.5-rc.2" 之前；semver 不会
        let all = vec![v("0.1.5-rc.2"), v("0.1.10-rc.1")];
        assert_eq!(latest_in(&all, Channel::Rc), Some(&v("0.1.10-rc.1")));
    }

    #[test]
    fn prerelease_sorts_before_release_of_same_version() {
        // 0.1.6-alpha.2 < 0.1.6 —— 语义正确性
        let all = vec![v("0.1.6-alpha.2"), v("0.1.6")];
        assert_eq!(latest_in(&all, Channel::Stable), Some(&v("0.1.6")));
        assert_eq!(latest_in(&all, Channel::Alpha), Some(&v("0.1.6-alpha.2")));
    }

    #[test]
    fn sorted_desc_puts_newest_first() {
        let s = sorted_desc(vec![v("0.1.5-rc.2"), v("0.1.6-alpha.2"), v("0.1.0")]);
        assert_eq!(s[0], v("0.1.6-alpha.2"));
        assert_eq!(s[2], v("0.1.0"));
    }

    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// 造一个假文件系统：给定存在的路径集合。
    fn exists_fn(set: HashSet<PathBuf>) -> impl Fn(&Path) -> bool {
        move |p: &Path| set.contains(p)
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn find_dsh_takes_the_first_hit_in_path_order() {
        // ★ 关键语义：PATH 顺序决定谁生效。
        // 本机实测 PATH 中 pnpm\bin 排在 npm 之前，若两者都有 dsh，pnpm 的会赢。
        let dirs = vec![p("C:/first"), p("C:/second")];
        let set: HashSet<PathBuf> = [p("C:/first/dsh.cmd"), p("C:/second/dsh.cmd")]
            .into_iter()
            .collect();
        assert_eq!(
            find_dsh_on_path(&dirs, &exists_fn(set)),
            Some(p("C:/first/dsh.cmd"))
        );
    }

    #[test]
    fn find_dsh_recognizes_exe_shim_for_bun() {
        // SHIM_NAMES 必须同时含 .exe 与 .cmd：bun 在 Windows 生成 .exe shim
        let dirs = vec![p("C:/bun/bin")];
        let set: HashSet<PathBuf> = [p("C:/bun/bin/dsh.exe")].into_iter().collect();
        assert_eq!(
            find_dsh_on_path(&dirs, &exists_fn(set)),
            Some(p("C:/bun/bin/dsh.exe"))
        );
    }

    #[test]
    fn find_dsh_returns_none_when_absent() {
        let dirs = vec![p("C:/a"), p("C:/b")];
        assert_eq!(find_dsh_on_path(&dirs, &exists_fn(HashSet::new())), None);
    }

    #[test]
    fn owner_of_matches_by_parent_directory() {
        let bins = vec![
            PmInfo { kind: Pm::Npm, version: "1".into(), bin_dir: p("C:/Users/x/AppData/Roaming/npm") },
            PmInfo { kind: Pm::Pnpm, version: "1".into(), bin_dir: p("C:/Users/x/AppData/Local/pnpm/bin") },
        ];
        assert_eq!(
            owner_of(&p("C:/Users/x/AppData/Roaming/npm/dsh.cmd"), &bins),
            Some(Pm::Npm)
        );
        assert_eq!(
            owner_of(&p("C:/Users/x/AppData/Local/pnpm/bin/dsh.exe"), &bins),
            Some(Pm::Pnpm)
        );
    }

    #[test]
    fn owner_of_is_case_insensitive_on_windows() {
        // Windows 路径大小写不敏感，必须归一化比较
        let bins = vec![PmInfo {
            kind: Pm::Npm,
            version: "1".into(),
            bin_dir: p("C:/Users/X/AppData/Roaming/NPM"),
        }];
        assert_eq!(
            owner_of(&p("c:/users/x/appdata/roaming/npm/dsh.cmd"), &bins),
            Some(Pm::Npm)
        );
    }

    #[test]
    fn owner_of_tolerates_trailing_separator() {
        let bins = vec![PmInfo {
            kind: Pm::Npm,
            version: "1".into(),
            bin_dir: p("C:/npm/"),
        }];
        assert_eq!(owner_of(&p("C:/npm/dsh.cmd"), &bins), Some(Pm::Npm));
    }

    #[test]
    fn owner_of_returns_none_for_unknown_location() {
        let bins = vec![PmInfo { kind: Pm::Npm, version: "1".into(), bin_dir: p("C:/npm") }];
        assert_eq!(owner_of(&p("D:/elsewhere/dsh.cmd"), &bins), None);
    }
}
