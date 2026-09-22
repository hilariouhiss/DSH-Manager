//! 主题的模式、解析与系统态探测。
//!
//! ⚠ 平台事实（已在依赖源码中核实，出处见
//! `docs/superpowers/specs/2026-09-22-theme-system-design.md` §2）：
//! Slint 1.18 **不允许应用读系统主题** —— `SlintInternal.color-scheme` 对用户代码
//! 是编译错误（`i-slint-compiler-1.18.0/tests/syntax/lookup/global.slint:38` 断言了
//! 这一点），`SlintContext::color_scheme()` 又只在 `private_unstable_api` 里。
//! 所以"跟随系统"必须我们自己探测。

// ⚠ 此处曾有一行 `#[cfg(windows)] use std::os::windows::ffi::OsStrExt;`（供
// `registry_dark` 的 `encode_wide` 使用）。Ruling 17 删除 `registry_dark` 后它没有
// 消费者 —— 实测 `cargo build` 报 `unused import`（简报 Step 4 末尾"在顶部加这一行"
// 是删除前的残留），故一并删除。Task 6 的注册表**通知**需要宽字符串时，在那次提交里
// 按需加回。

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

/// 个人化设置键。Task 6 的变更监视用它 —— **只用于通知，不用于取值**
/// （取值一律走上面的 uxtheme 序号，理由见 `system_dark` 的文档）。
///
/// ⚠ 它的消费者在 Task 6，故需要一处过渡期抑制（Task 9 一并删除）。
#[allow(dead_code)]
#[cfg(windows)]
const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// 当前系统是否为暗色。
///
/// **本函数是 winit 判据 `should_use_dark_mode()` 的逐条镜像**
/// （`winit-0.30.13/src/platform_impl/windows/dark_mode.rs:126-127`）：
/// `should_apps_use_dark_mode() && !is_high_contrast()`。
///
/// ⚠ **三条失败路径一律返回 `false`（浅色），这是刻意的，不是保守。**
/// winit 的 `should_apps_use_dark_mode()` 用 `.unwrap_or(false)` 收尾（`:126` 同文件 `:153`），
/// `try_theme` 在 `!DARK_MODE_SUPPORTED` 时也直接落到 `Theme::Light`（`:61`/`:80`）。
/// 也就是说**取不到版本、取不到模块、取不到序号，winit 全判浅色**。
/// 我们若在这些情况下改读注册表，就会在"系统设置是暗色"的旧机器上让主体变暗、
/// 而标题栏仍是浅的 —— 正是同源约束要禁止的那种不一致。
///
/// ⚠ 因此本模块**不读注册表**。早先的 `registry_dark()` 回落已删除：
/// 它只在上述失败路径上被触发，而那些路径恰恰是 winit 判浅色的路径 ——
/// 回落不是安全网，是分叉源。（注册表在 Task 6 仍然要用，但只用于**变更通知**，不用于取值。）
// ⚠ 过渡期抑制：Task 6 的监视器是它的消费者，Task 9 连同文件顶部三处一起删除。
#[allow(dead_code)]
#[cfg(windows)]
pub fn system_dark() -> bool {
    use windows_sys::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETHIGHCONTRAST};

    /// 高对比度下 winit 判定为浅色（`dark_mode.rs:126` 的 `!is_high_contrast()`），
    /// 我们必须同样处理，否则高对比度用户会看到主体与标题栏不一致。
    fn high_contrast() -> bool {
        // HIGHCONTRASTW 在这个版本实现了 Default，用它而不是手写零值字段。
        let mut hc = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            ..Default::default()
        };
        // SPI_GETHIGHCONTRAST = 66；失败时按"非高对比度"处理（winit 也是同向的保守处理）
        let ok = unsafe {
            SystemParametersInfoW(SPI_GETHIGHCONTRAST, hc.cbSize, (&raw mut hc).cast(), 0)
        };
        ok != 0 && hc.dwFlags & HCF_HIGHCONTRASTON != 0
    }

    // 三条失败路径全判浅色 —— 逐条对齐 winit，理由见函数文档。
    // 顺序上先挡版本：不达标时 winit 连序号都不去解析，我们也不必白跑一次高对比度查询。
    if !dark_mode_supported() {
        return false;
    }
    if high_contrast() {
        return false;
    }
    // 取不到序号 → winit 的 `unwrap_or(false)` → 浅色
    uxtheme_dark().unwrap_or(false)
}

/// 本机是否达到"支持暗色模式"的 Windows 版本。
///
/// ⚠ **与 winit 逐字同源，这是同源约束的一部分，不是可选优化。**
/// winit 的判定是 `DARK_MODE_SUPPORTED`（`winit-0.30.13/src/platform_impl/windows/dark_mode.rs:46-53`），
/// 它先经 ntdll 的 `RtlGetVersion` 取版本，再要求
/// **`status >= 0` 且 `dwMajorVersion == 10` 且 `dwMinorVersion == 0`**，最后才比 `dwBuildNumber >= 17763`。
/// 本函数把那三个条件一起照搬 —— 只比构建号会让本函数在"主版本不是 10 的未来系统"上
/// 判成 `true` 而 winit 判 `false`，于是我们走 uxtheme、winit 走浅色，
/// **又回到主体与标题栏各说各话**，正是同源约束要禁止的那件事。
///
/// ⚠ **不能用 `GetVersionExW`**：它没有 manifest 时会**撒谎**（Win10+ 仍报 6.2 / build 9200），
/// 那会让本函数在现代系统上恒为 false，于是我们永远走注册表而 winit 走 uxtheme —— 同样是不同源。
/// winit 用 `RtlGetVersion` 正是为此。
///
/// 为什么必须挡：`uxtheme.dll` 的序号 132 只在 build 17763+ 才有定义。低于该版本时，
/// ① 若该序号上恰好是别的导出，`transmute` 出来的错误原型调用就是 **UB**；
/// ② winit 在那种机器上判**浅色**，我们若判成暗色就会不一致。
///
/// 取不到版本号、或版本不满足上述条件时返回 `false`（走注册表）：
/// 宁可在旧机器上退化成注册表读数，也不赌一个未知序号。
///
/// ⚠ **维护契约**：本函数是 winit 判定的镜像。若哪天 winit 放宽了它的条件
/// （例如支持主版本不再是 10 的系统），**这里必须同步放宽**，否则又会分叉。
#[cfg(windows)]
fn dark_mode_supported() -> bool {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOW;

    type RtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

    unsafe {
        // 与 `uxtheme_dark` 同一套取法：ntdll 常驻进程，不 FreeLibrary（同款理由）。
        let module = LoadLibraryA(c"ntdll.dll".as_ptr().cast());
        if module.is_null() {
            return false;
        }
        let Some(proc) = GetProcAddress(module, c"RtlGetVersion".as_ptr().cast()) else {
            return false;
        };
        let f: RtlGetVersion = std::mem::transmute(proc);
        let mut vi = OSVERSIONINFOW {
            dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOW>() as u32,
            ..Default::default()
        };
        let status = f(&mut vi);
        // NTSTATUS 的成功判据是 `>= 0`（不是 `== 0`）—— 与 winit 的写法保持一致
        status >= 0 && vi.dwMajorVersion == 10 && vi.dwMinorVersion == 0 && vi.dwBuildNumber >= 17763
    }
}

/// `uxtheme.dll` 序号 132 导出。取不到（老系统）返回 `None`。
#[cfg(windows)]
fn uxtheme_dark() -> Option<bool> {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    // ⚠ 按序号取，不是按名字：该导出无名。`132 as *const u8` 是 winit 同款用法
    // （winit-0.30.13/src/platform_impl/windows/dark_mode.rs:141）。
    const ORDINAL: usize = 132;
    type ShouldAppsUseDarkMode = unsafe extern "system" fn() -> windows_sys::core::BOOL;

    unsafe {
        let module = LoadLibraryA(c"uxtheme.dll".as_ptr().cast());
        if module.is_null() {
            return None;
        }
        let proc = GetProcAddress(module, ORDINAL as *const u8)?;
        // ⚠ 故意不 FreeLibrary：该模块是进程级单例，且进程存活期内我们会反复调用。
        // 释放它会在下次调用时重新加载 —— 那是纯粹的浪费，不是严谨。
        let f: ShouldAppsUseDarkMode = std::mem::transmute(proc);
        Some(f() != 0)
    }
}

/// 非 Windows：不作探测，一律浅色（GC-1 声明只在 Windows 上验证）。
// ⚠ 过渡期抑制：与 Windows 侧同因 —— Task 6 的监视器是它的消费者，Task 9 一并删除。
#[allow(dead_code)]
#[cfg(not(windows))]
pub fn system_dark() -> bool {
    false
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

    /// `Tokens` 全局里应当出现的 brush 令牌数量下限。
    ///
    /// ⚠ 下限守的是"**某个令牌行被整行删掉**"这一情形：`parse_palette` 只认带 `<brush>` 的行
    /// （块内合法地存在 `<length>` / `<duration>` / `<float>` 行，不能因为不是 brush 就报错），
    /// 于是被删掉的行不留任何痕迹 —— 40 变 39，那个令牌就静默失去对比度覆盖。
    /// 取 `>=` 而非 `==`：以后新增令牌不会误报，而丢令牌一定会被抓到。
    ///
    /// ⚠ 但**不要**把"某行的 `<brush>` 标记被写坏"算作本断言的功劳：那种写法（如 `<brushX>`）
    /// 会让 Slint 编译器先报 `Unknown type`，根本走不到测试。所以本下限是**纵深防御**，
    /// 不是唯一一道防线（Task 3+4 实测确认：改坏标记时 build.rs 直接 panic）。
    const MIN_BRUSH_TOKENS: usize = 40;

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
                // ⚠ 两段式切分，顺序不能反：值的形状是 `dark ? #暗 : #浅`，
                // 所以 `?` 之前那一段是**条件本身**（` dark `），里面没有 `#`。
                // 先按 `?` 验证形状，再在 `?` 之后那一段里按 `:` 分出明暗两值。
                // （先按 `:` 再按 `?` 会去 ` dark ` 里找 hex，永远找不到 —— 那是本计划初稿的 bug。）
                let (_, arms) = val
                    .split_once('?')
                    .ok_or_else(|| format!("令牌 {name} 没有明暗条件（期望 `dark ? 暗 : 浅`）: {line}"))?;
                let (dark_side, light_side) = arms
                    .split_once(':')
                    .ok_or_else(|| format!("令牌 {name} 的明暗两值之间缺冒号: {line}"))?;
                let (dark, light) = (
                    hex(dark_side).ok_or_else(|| format!("{name} 的暗色侧不是 hex: {line}"))?,
                    hex(light_side).ok_or_else(|| format!("{name} 的浅色侧不是 hex: {line}"))?,
                );
                out.push((name, dark, light));
            }
        }
        if out.len() < MIN_BRUSH_TOKENS {
            return Err(format!(
                "只解析出 {} 个 brush 令牌，少于下限 {MIN_BRUSH_TOKENS} —— \
                 多半是某行的整行被删掉或被跳过，那个令牌会静默失去覆盖",
                out.len()
            ));
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

    /// ⚠ 这条**不能**断言具体值 —— 值取决于跑测试时系统的主题设置。
    /// 它能抓住的是：FFI 不崩、不吃到空指针、两次调用自洽。
    /// 序号写错或 `GetProcAddress` 结果被误用时，这里会直接段错误而不是返回。
    #[test]
    fn system_dark_is_callable_and_stable() {
        let a = system_dark();
        let b = system_dark();
        assert_eq!(a, b, "同一时刻两次探测必须一致");
    }
}
