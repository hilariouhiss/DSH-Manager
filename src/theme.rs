//! 主题的模式、解析与系统态探测。
//!
//! ⚠ 平台事实（已在依赖源码中核实，出处见
//! `docs/superpowers/specs/2026-09-22-theme-system-design.md` §2）：
//! Slint 1.18 **不允许应用读系统主题** —— `SlintInternal.color-scheme` 对用户代码
//! 是编译错误（`i-slint-compiler-1.18.0/tests/syntax/lookup/global.slint:38` 断言了
//! 这一点），`SlintContext::color_scheme()` 又只在 `private_unstable_api` 里。
//! 所以"跟随系统"必须我们自己探测。

// ⚠ 过渡期抑制，**Task 9 必须删除本行**（那里有强制的删除步骤）。
// 本模块的项分三批被消费：parse/as_str → Task 2，resolve → Task 6，
// index/from_index → Task 8。在最后一个消费者到位之前，`cargo build` 会对尚未
// 被消费的项报 dead_code，而 Global Constraints 要求构建输出干净。
// 仓库先例：docs/RULINGS.md Ruling 10（同一缺陷，当时用的是 crate 级写法，
// 因为那时受影响项跨多个文件；本次只涉及本模块，故取更窄的模块级）。
// ⚠ 本行只压"还没被消费"，**不得**用它掩盖真实死代码 —— 后者一律删除。
#![allow(dead_code)]

/// 主题模式。
///
/// ⚠ 下标即 UI 契约：`ui/app.slint` 的 `theme-mode` 用 0/1/2 表示这三档，
/// 与 `CloseBehavior` / `close-behavior-index` 同一约定，改顺序要两边一起改。
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ThemeMode {
    Light,
    Dark,
    /// 跟随系统。也是 `state.json` 里**没记过**时的缺省 —— 于是"默认跟随系统"
    /// 这条需求不需要任何额外标志：没值就是跟随。
    #[default]
    Auto,
}

impl ThemeMode {
    pub fn index(self) -> i32 {
        match self {
            ThemeMode::Light => 0,
            ThemeMode::Dark => 1,
            ThemeMode::Auto => 2,
        }
    }

    pub fn from_index(i: i32) -> Option<Self> {
        match i {
            0 => Some(ThemeMode::Light),
            1 => Some(ThemeMode::Dark),
            2 => Some(ThemeMode::Auto),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
            ThemeMode::Auto => "auto",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "light" => Some(ThemeMode::Light),
            "dark" => Some(ThemeMode::Dark),
            "auto" => Some(ThemeMode::Auto),
            _ => None,
        }
    }
}

/// 把模式与系统态合成"现在该不该用暗色"。
///
/// **这是主题的唯一判据** —— UI 不参与判断，`system_dark` 甚至不进 .slint。
/// 强制模式下 `system_dark` 变化不会改变返回值，这是正确行为不是漏接线。
pub fn resolve(mode: ThemeMode, system_dark: bool) -> bool {
    match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::Auto => system_dark,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_roundtrips_for_every_mode() {
        for m in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Auto] {
            assert_eq!(ThemeMode::from_index(m.index()), Some(m));
        }
    }

    #[test]
    fn out_of_range_index_is_rejected_not_guessed() {
        // 越界不得猜一个模式 —— 与 CloseBehavior::from_index 同判据
        assert_eq!(ThemeMode::from_index(3), None);
        assert_eq!(ThemeMode::from_index(-1), None);
    }

    #[test]
    fn str_roundtrips_for_every_mode() {
        for m in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Auto] {
            assert_eq!(ThemeMode::parse(m.as_str()), Some(m));
        }
    }

    #[test]
    fn unrecognized_str_is_treated_as_never_set() {
        // 手改过或降级运行留下的值一律当"没记过"（→ 跟随系统），
        // 这比替用户猜一个固定主题安全。与 close_behavior 的既有处理同款。
        assert_eq!(ThemeMode::parse("MINT"), None);
        assert_eq!(ThemeMode::parse(""), None);
    }

    #[test]
    fn default_mode_follows_the_system() {
        assert_eq!(ThemeMode::default(), ThemeMode::Auto);
    }

    #[test]
    fn resolve_truth_table() {
        // 强制档忽略系统态；自动档完全等于系统态。六个格子逐个钉住。
        assert!(!resolve(ThemeMode::Light, true));
        assert!(!resolve(ThemeMode::Light, false));
        assert!(resolve(ThemeMode::Dark, true));
        assert!(resolve(ThemeMode::Dark, false));
        assert!(resolve(ThemeMode::Auto, true));
        assert!(!resolve(ThemeMode::Auto, false));
    }
}
