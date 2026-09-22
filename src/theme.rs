//! 主题的模式、解析与系统态探测。
//!
//! ⚠ 平台事实（已在依赖源码中核实，出处见
//! `docs/superpowers/specs/2026-09-22-theme-system-design.md` §2）：
//! Slint 1.18 **不允许应用读系统主题** —— `SlintInternal.color-scheme` 对用户代码
//! 是编译错误（`i-slint-compiler-1.18.0/tests/syntax/lookup/global.slint:38` 断言了
//! 这一点），`SlintContext::color_scheme()` 又只在 `private_unstable_api` 里。
//! 所以"跟随系统"必须我们自己探测。

// ⚠ 过渡期抑制，**Task 9 Step 1 必须删除这三行**（那里有强制的删除步骤）。
// 本模块的项分三批被消费：parse/as_str → Task 2，resolve → Task 6，
// index/from_index → Task 8。在最后一个消费者到位之前，`cargo build` 会对尚未
// 被消费的项报 dead_code，而 Global Constraints 要求构建输出干净。
// ⚠ 只标注这三个**已知待消费**的项，不用模块级 blanket —— 本文件是 Task 2/6/8
// 都要改的地方，模块级会连真实死代码一起盖住（见 Task 1 评审的 Important 项）。
// 仓库先例：docs/RULINGS.md Ruling 10（同一缺陷；那次用 crate 级，因为受影响项跨多个文件）。
#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
    use std::collections::BTreeMap;

    #[test]
    fn index_roundtrips_for_every_mode() {
        for m in [ThemeMode::Light, ThemeMode::Dark, ThemeMode::Auto] {
            assert_eq!(ThemeMode::from_index(m.index()), Some(m));
        }
        // ⚠ 往返**只钉到置换等价**：把 Dark 与 Auto 在 index 与 from_index 里
        // **同时**对调，上面那行照样通过。而 0/1/2 是跨文件契约 ——
        // ui/app.slint 的 ChipButton 直接写死 `theme-mode == 0/1/2`（Task 7），
        // 对调后界面高亮与真实模式会错位，且没有任何测试会发现。故钉死字面值。
        assert_eq!(
            [ThemeMode::Light.index(), ThemeMode::Dark.index(), ThemeMode::Auto.index()],
            [0, 1, 2]
        );
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
        // ⚠ 同理钉死字面值。字符串比下标更要紧：state.json 里已经落盘的值是
        // "light"/"dark"/"auto"，把 dark 与 auto 同时互换会**改变存量数据的含义** ——
        // 用户上次选的"深色"会变成"跟随系统"，而且往返测试依然全绿。
        assert_eq!(
            [ThemeMode::Light.as_str(), ThemeMode::Dark.as_str(), ThemeMode::Auto.as_str()],
            ["light", "dark", "auto"]
        );
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

    /// 从 `ui/app.slint` 的 Tokens 全局解析每个 brush 令牌的（暗值, 浅值）。
    ///
    /// ⚠ 只认 `out property <brush> 名字: dark ? #AAAAAA : #BBBBBB;` 这一形状，
    /// 且**形状不认识时报错而不是跳过** —— 跳过会让测试在令牌被改写后
    /// 悄悄失去覆盖，那正是这类回归测试最容易失效的方式。
    fn parse_palette() -> Result<Vec<(String, String, String)>, String> {
        let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/app.slint"))
            .map_err(|e| format!("读不到 ui/app.slint: {e}"))?;
        let start = src.find("global Tokens {").ok_or("找不到 Tokens 全局")?;
        let body = &src[start..];
        let end = body.find("\n}").ok_or("Tokens 全局没有结束大括号")?;
        let hex = |s: &str| -> Option<String> {
            let i = s.find('#')?;
            let rest: String = s[i + 1..].chars().take(8).collect();
            let n = rest.chars().take_while(|c| c.is_ascii_hexdigit()).count();
            (n == 6 || n == 8).then(|| format!("#{}", &rest[..n]))
        };
        let mut out = Vec::new();
        for line in body[..end].lines() {
            let Some(rest) = line.split("<brush>").nth(1) else { continue };
            let (name, val) = rest
                .split_once(':')
                .ok_or_else(|| format!("令牌行缺冒号: {line}"))?;
            let name = name.trim().to_string();
            if !name.is_empty() {
                // ⚠ 条件式是 `dark ? 暗 : 浅`：`?` 之前是**条件**，暗值在 `?` 之后。
                // 故先切 `?` 校验形状，再在其余部分切出两个分支。
                let (_, branches) = val
                    .split_once('?')
                    .ok_or_else(|| format!("令牌 {name} 没有明暗条件（期望 `dark ? 暗 : 浅`）: {line}"))?;
                let (dark_side, light_side) = branches
                    .split_once(':')
                    .ok_or_else(|| format!("令牌 {name} 的分支缺冒号（期望 `dark ? 暗 : 浅`）: {line}"))?;
                let (dark, light) = (
                    hex(dark_side).ok_or_else(|| format!("{name} 的暗色侧不是 hex: {line}"))?,
                    hex(light_side).ok_or_else(|| format!("{name} 的浅色侧不是 hex: {line}"))?,
                );
                out.push((name, dark, light));
            }
        }
        if out.is_empty() {
            return Err("一个令牌都没解析出来".into());
        }
        Ok(out)
    }

    fn palette() -> BTreeMap<String, (String, String)> {
        let mut m = BTreeMap::new();
        for (n, d, l) in parse_palette().expect("解析 ui/app.slint 令牌失败") {
            m.insert(n, (d, l));
        }
        m
    }

    fn lin(c: f64) -> f64 {
        if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }

    /// 返回 (r, g, b, alpha)，alpha 为 1.0 表示不透明。
    fn rgba(hex: &str) -> (u8, u8, u8, f64) {
        let h = hex.trim_start_matches('#');
        let ch = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).expect("hex 解析");
        let a = if h.len() == 8 { ch(6) as f64 / 255.0 } else { 1.0 };
        (ch(0), ch(2), ch(4), a)
    }

    fn lum(hex: &str) -> f64 {
        let (r, g, b, _) = rgba(hex);
        0.2126 * lin(r as f64 / 255.0) + 0.7152 * lin(g as f64 / 255.0) + 0.0722 * lin(b as f64 / 255.0)
    }

    fn contrast(a: &str, b: &str) -> f64 {
        let (la, lb) = (lum(a), lum(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// 把带 alpha 的 fg 合成到不透明 bg 上，返回不透明 hex。
    fn over(fg: &str, bg: &str) -> String {
        let (fr, fg_, fb, a) = rgba(fg);
        let (br, bg_, bb, _) = rgba(bg);
        let mix = |f: u8, b: u8| (f as f64 * a + b as f64 * (1.0 - a)).round() as u8;
        format!("#{:02X}{:02X}{:02X}", mix(fr, br), mix(fg_, bg_), mix(fb, bb))
    }

    /// 文字类令牌：两套主题都必须过门槛。背景取 **canvas**（各自主题里最不利的
    /// 那个面）。门槛见规格 §4.3：正文 4.5，小字标签 3.0。
    #[test]
    fn both_palettes_meet_text_contrast_bars() {
        let p = palette();
        let (c_dark, c_light) = p.get("canvas").expect("缺 canvas 令牌").clone();
        let bars: [(&str, f64); 9] = [
            ("ink", 4.5),
            ("ink-2", 4.5),
            ("ink-3", 4.5),
            ("ink-4", 3.0),
            ("accent", 4.5),
            ("accent-ink", 4.5),
            ("good", 4.5),
            ("warn", 4.5),
            ("danger", 4.5),
        ];
        for (name, bar) in bars {
            let (d, l) = p.get(name).unwrap_or_else(|| panic!("缺 {name} 令牌")).clone();
            let rd = contrast(&d, &c_dark);
            let rl = contrast(&l, &c_light);
            assert!(rd >= bar, "暗色 {name}={d} on {c_dark} 只有 {rd:.2}，需 >= {bar}");
            assert!(rl >= bar, "浅色 {name}={l} on {c_light} 只有 {rl:.2}，需 >= {bar}");
        }
    }

    /// α 档令牌：合成到各自 canvas 后必须仍然可辨。数值门槛取自规格 §4.3 表格。
    #[test]
    fn translucent_tiers_stay_visible_in_both_palettes() {
        let p = palette();
        let (c_dark, c_light) = p.get("canvas").expect("缺 canvas 令牌").clone();
        let bars: [(&str, f64); 11] = [
            ("shell", 1.005),
            ("hairline", 1.02),
            ("hairline-soft", 1.01),
            ("hairline-strong", 1.10),
            ("fill", 1.01),
            ("fill-hover", 1.02),
            ("fill-active", 1.05),
            ("sunk", 1.015),
            ("accent-fill", 1.02),
            ("accent-line", 1.10),
            ("glow-brand", 1.01),
        ];
        for (name, bar) in bars {
            let (d, l) = p.get(name).unwrap_or_else(|| panic!("缺 {name} 令牌")).clone();
            for (label, val, bg) in [("暗色", &d, &c_dark), ("浅色", &l, &c_light)] {
                let comp = over(val, bg);
                let r = contrast(&comp, bg);
                assert!(
                    r >= bar,
                    "{label} {name}={val} 合成到 {bg} 得 {comp}，只有 {r:.3}，需 >= {bar}"
                );
            }
        }
    }
}
