//! 包管理器探测（FR-1 ~ FR-5）、通道判定（FR-7 / FR-8）与命令执行 helper。

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
}
