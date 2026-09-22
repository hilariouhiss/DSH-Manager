# 明暗双主题与三种切换模式 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让应用有明、暗两套完整主题，用户可在「浅色 / 深色 / 跟随系统」间切换，入口在设置面板，默认跟随系统，且运行中实时跟随 Windows 主题变化。

**Architecture:** Rust 是主题的唯一真相源 —— `src/theme.rs` 探测系统态并合成 `resolved_dark`，经 `ui.global::<Tokens>().set_dark()` 写入 Slint 全局；`Tokens` 每个令牌写成 `dark ? 暗值 : 浅值`。UI 只上报意图（`theme-mode-changed(int)`）与显示选中态（`theme-mode`），`system_dark` 不进 .slint。Fluent 控件（ScrollView 滚动条 / AboutSlint）由 `Palette.color-scheme` 单独同步。

**Tech Stack:** Rust 2024 / Slint 1.18（`slint-build` 编译 `ui/app.slint`）/ serde_json（手写 JSON，无 derive）/ windows-sys 0.61（钉在已在 lock 中的版本）/ 既有 `mpsc` + 80ms Timer 排空机制。

**Spec:** `docs/superpowers/specs/2026-09-22-theme-system-design.md`（本计划从该规格论证而来，执行者两份都要读；规格 §2 的平台事实表是本设计的支点，§4.3 是浅色令牌的权威值表）

## Global Constraints

- **两份契约不可破坏**（`ui/app.slint` 头部注释）：所有 `in property` / `in-out property` 的名字与类型、所有 callback 的名字与签名，改任一都要同步 `src/main.rs`。
- **`cargo build` 必须 0 警告**，`cargo test` 必须全绿。本仓库不接受 `allow` 掩盖死代码。
- **下标即契约**：`ThemeMode` 的 0/1/2 与 `ui/app.slint` 的 `theme-mode` 一一对应（同 `CloseBehavior` 与 `close-behavior-index` 的既有约定）。
- **不加会新增编译单元的依赖。** `windows-sys` 固定 `"0.61"`（0.61.2 已在 `Cargo.lock` 中，被 12 处依赖使用）。
- **暗色侧现有色值一律不改** —— 本次是加浅色，不是重做暗色。上次提交 `9c5a478` 的验证结论必须继续有效。
- **端口纪律**：`3080` 是本会话的 live `dsh web`，任何验证都不得触碰；需要真实监听的场景用 `3099`。截图运行应用时**不得点击任何控件**。
- **不得 `git add target/`**（`.gitignore` 已忽略，验证脚本放那里）。

## File Structure

| 文件 | 责任 | 动作 |
|---|---|---|
| `src/theme.rs` | 主题模式、解析、系统态探测、变更监视。**唯一**知道"如何得到系统主题"的地方 | 新建 |
| `src/config.rs` | `state.json` I/O。新增 `theme_mode` 字段的读写与兼容 | 修改 |
| `src/model.rs` | `UiMsg` 消息枚举。新增系统主题变更变体 | 修改 |
| `src/main.rs` | `AppState`、`project()`、`drain()`、`wire_callbacks()`、`main()` 接线 | 修改 |
| `ui/app.slint` | `Tokens` 双主题令牌、设置面板「主题」组、两份契约 | 修改 |
| `Cargo.toml` | 加 `windows-sys` 目标依赖 | 修改 |
| `docs/ARCHITECTURE.md` / `docs/RULINGS.md` / `docs/VERIFICATION.md` | 平台事实留档与验证记录 | 修改 |

**不新建** `src/theme_win.rs`：探测与解析同属"主题"这一个职责，且解析部分需要被探测部分复用；拆开只会制造两处互相引用的文件。

---

### Task 1: `ThemeMode` 与解析（纯逻辑）

**Files:**
- Create: `src/theme.rs`
- Modify: `src/main.rs:6-10`（`mod` 声明处，加一行 `mod theme;`）

**Interfaces:**
- Consumes: 无
- Produces:
  - `theme::ThemeMode`（`Copy + Clone + PartialEq + Eq + Debug + Default`，默认 `Auto`）
  - `ThemeMode::index(self) -> i32`、`ThemeMode::from_index(i: i32) -> Option<ThemeMode>`
  - `ThemeMode::as_str(self) -> &'static str`、`ThemeMode::parse(s: &str) -> Option<ThemeMode>`
  - `theme::resolve(mode: ThemeMode, system_dark: bool) -> bool`

- [ ] **Step 1: 写失败的测试**

创建 `src/theme.rs`，**只写文档注释与测试模块**（实现留空，先让它编译失败）：

```rust
//! 主题的模式、解析与系统态探测。
//!
//! ⚠ 平台事实（已在依赖源码中核实，出处见
//! `docs/superpowers/specs/2026-09-22-theme-system-design.md` §2）：
//! Slint 1.18 **不允许应用读系统主题** —— `SlintInternal.color-scheme` 对用户代码
//! 是编译错误（`i-slint-compiler-1.18.0/tests/syntax/lookup/global.slint:38` 断言了
//! 这一点），`SlintContext::color_scheme()` 又只在 `private_unstable_api` 里。
//! 所以"跟随系统"必须我们自己探测。

#[cfg(test)]
mod tests {
    use super::*;

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
}
```

同时在 `src/main.rs` 的模块声明处（`mod pm;` 之后）加：

```rust
mod theme;
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test theme:: 2>&1 | tail -20`
Expected: 编译错误 —— `cannot find type ThemeMode in this scope`、`cannot find function resolve`。

- [ ] **Step 3: 写最小实现**

在 `src/theme.rs` 的文档注释之后、`#[cfg(test)]` 之前插入：

```rust
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test theme:: 2>&1 | tail -12`
Expected: `test result: ok. 6 passed`

- [ ] **Step 5: 加过渡期的 `dead_code` 抑制（带移除契约）**

⚠ **这一步是必需的，且理由已实测**：二进制 crate 里未被消费的 `pub` 项会触发 `dead_code`（本步会看到 3 条：
`ThemeMode`、它的 4 个关联项、`resolve`）。消费者要等到 Task 2（`parse`/`as_str`）、Task 6（`resolve`）、
Task 8（`index`/`from_index`）才到位，而 Global Constraints 要求 `cargo build` 0 警告 —— 两条在过渡期不可兼得。

仓库先例：`docs/RULINGS.md` **Ruling 10** 处理过同一个缺陷（当时是 `model.rs` 的类型要到 Task 17 才被消费），
措施是"一行带移除契约的 `#![allow(dead_code)]` + 最终任务里强制删除"。

本计划的形态与那次不同：受影响项**只在一个模块里**（`src/theme.rs`），而 Ruling 10 选 crate 级正是因为
当时"分布在 model.rs / pm.rs / dsh.rs 多个文件"。

Task 1 评审进一步指出模块级仍然偏宽（**Ruling 7**）：它会连同 `#[cfg(test)] mod tests` 以及将来任何
真被写死的 helper 一起盖住，而 `src/theme.rs` 恰恰是 Task 2 / 6 / 8 都要改的那个文件 ——
"允许掩盖真死代码"的窗口会覆盖整个特性期。故改为**逐项**标注，共三处
（`impl ThemeMode` 一处即覆盖其全部关联项）：

- 在 `pub enum ThemeMode` 之前、`impl ThemeMode` 之前、`pub fn resolve` 之前，各加一行 `#[allow(dead_code)]`。
- 注释块只在**第一处**写全，另两处各只写这一行，避免三份重复注释。

```rust
// ⚠ 过渡期抑制，**Task 9 Step 1 必须删除这三行**（那里有强制的删除步骤）。
// 本模块的项分三批被消费：parse/as_str → Task 2，resolve → Task 6，
// index/from_index → Task 8。在最后一个消费者到位之前，`cargo build` 会对尚未
// 被消费的项报 dead_code，而 Global Constraints 要求构建输出干净。
// ⚠ 只标注这三个**已知待消费**的项，不用模块级 blanket —— 本文件是 Task 2/6/8
// 都要改的地方，模块级会连真实死代码一起盖住（见 Task 1 评审的 Important 项）。
// 仓库先例：docs/RULINGS.md Ruling 10（同一缺陷；那次用 crate 级，因为受影响项跨多个文件）。
#[allow(dead_code)]
```

Run: `cargo build 2>&1 | grep -c "^warning"`
Expected: `0`

再 Run: `cargo test 2>&1 | tail -4`
Expected: `test result: ok. 101 passed`（95 + 6 新增）

- [ ] **Step 6: 提交**

```bash
git add src/theme.rs src/main.rs
git commit -m "feat(theme): ThemeMode 三档与 resolve 解析（纯逻辑 + 单测）"
```

---

### Task 2: `state.json` 持久化 `theme_mode`

**Files:**
- Modify: `src/config.rs:11`（`use` 区）、`:57-63`（`StateFile`）、`:90-99`（解析）、`:146-150`（保存）、以及 6 处测试字面构造：`:377`、`:403`、`:429`、`:439`、`:485`、`:491`
- Test: `src/config.rs` 的 `mod tests`

**Interfaces:**
- Consumes: `theme::ThemeMode`、`ThemeMode::as_str`、`ThemeMode::parse`（Task 1）
- Produces: `config::StateFile { …, pub theme_mode: Option<ThemeMode> }`

- [ ] **Step 1: 写失败的测试**

在 `src/config.rs` 的 `mod tests` 内追加（`use super::*;` 已存在；再补 `use crate::theme::ThemeMode;`）：

```rust
    #[test]
    fn theme_mode_roundtrips_through_json() {
        let p = tmp("theme.json");
        let s = StateFile {
            preferred_port: None,
            running_port: None,
            close_behavior: None,
            theme_mode: Some(ThemeMode::Dark),
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
        };
        save_to(&p, &s).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v.get("theme_mode").and_then(|x| x.as_str()), Some("light"));
        let _ = std::fs::remove_file(&p);
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test config::tests::theme 2>&1 | tail -20`
Expected: 编译错误 —— `struct StateFile has no field named theme_mode`，同时 6 处既有字面构造报 `missing field theme_mode`。

- [ ] **Step 3: 写最小实现**

`src/config.rs` 顶部 `use` 区加：

```rust
use crate::theme::ThemeMode;
```

`StateFile` 加字段：

```rust
#[derive(Default, Clone, Debug, PartialEq)]
pub struct StateFile {
    pub preferred_port: Option<u16>,
    pub running_port: Option<u16>,
    /// `None` = 从没问过（首次关闭要弹询问框）。见 `CloseBehavior`。
    pub close_behavior: Option<CloseBehavior>,
    /// `None` = 从没设过 = 跟随系统。见 `ThemeMode`。
    pub theme_mode: Option<ThemeMode>,
}
```

`load_from` 的 `StateFile { .. }`（约 `:90`）加：

```rust
        theme_mode: v
            .get("theme_mode")
            .and_then(|x| x.as_str())
            .and_then(ThemeMode::parse),
```

`save_to`（约 `:146`）加：

```rust
        "theme_mode": s.theme_mode.map(ThemeMode::as_str),
```

把 6 处测试字面构造各补一行 `theme_mode: None,`（编译器会逐个报 `missing field`，不要用 `..Default::default()` —— 那样新增字段就再也无法被编译器提醒了）。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test config:: 2>&1 | tail -12`
Expected: 全部 ok，包含新增 4 项。

- [ ] **Step 5: 提交**

```bash
git add src/config.rs
git commit -m "feat(theme): state.json 持久化 theme_mode，含旧文件兼容"
```

---

### Task 3: 对比度回归测试（先红）

**Files:**
- Modify: `src/theme.rs`（在 `mod tests` 内追加）

**Interfaces:**
- Consumes: Task 1 的 `src/theme.rs`
- Produces: `mod tests` 内的辅助函数 `parse_palette() -> Result<Vec<(String, String, String)>, String>`、`palette() -> BTreeMap<String, (String, String)>`、`contrast(&str, &str) -> f64`、`over(&str, &str) -> String`（Task 4 靠这两个测试验收）

> **为什么先写这个**：Task 4 要改 45 行令牌，全靠肉眼会漏。这两个测试**解析 `ui/app.slint` 的实际文本**（不是校验意图），漏改或写错色值会让它红。

- [ ] **Step 1: 写失败的测试**

在 `src/theme.rs` 的 `mod tests` 内追加（顶部补 `use std::collections::BTreeMap;`）：

```rust
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
                 多半是某行的 `<brush>` 标记被改了，那些令牌会静默失去覆盖",
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test theme::tests 2>&1 | tail -20`
Expected: 两个新测试 FAIL —— 报 `令牌 xxx 没有明暗条件（期望 `dark ? 暗 : 浅`）`。这正是预期的红：`ui/app.slint` 现在还是单套暗色常量。

- [ ] **Step 3: 提交（红测试先入库）**

```bash
git add src/theme.rs
git commit -m "test(theme): 双主题对比度回归测试（解析 app.slint 实际值，先红）"
```

---

### Task 4: `ui/app.slint` 双主题令牌

**Files:**
- Modify: `ui/app.slint:46-111`（`Tokens` 全局）、`:136`（`Orb` 默认 tint）、`:241`（图标井）、`:268`（CTA 悬停）、`:471`（Flyout 阴影）、`:1190`（顶部光）

**Interfaces:**
- Consumes: `theme::resolve` 的布尔结果（Task 8 写入）
- Produces: `export global Tokens`（Rust 侧类型 `Tokens`，含 `set_dark(bool)` / `get_dark() -> bool`）、新增令牌 `well` / `well-hover` / `cta-hover-top` / `cta-hover-bottom` / `top-light` / `shadow`

**权威色值表是规格 §4.3**（浅色侧每一格都已验算过 WCAG，不要自行发挥）。暗色侧一律保持现值。

- [ ] **Step 1: 打开令牌块并与规格对表**

读 `ui/app.slint:46-111` 与规格 §4.3 表格逐行对照。**先看清再动手** —— 45 个令牌里任何一个漏改，Task 3 的测试都会红。

- [ ] **Step 2: 改 `Tokens` 为可切换的双主题全局**

把 `global Tokens {` 改为 `export global Tokens {`，并在其第一行插入输入属性；每个 `out property <brush>` 改为 `dark ? 暗值 : 浅值`。**暗色值原样保留**，浅色值取自规格 §4.3。整块替换为：

```slint
// ── 设计令牌 ────────────────────────────────────────────────────────────────
// 一处定义，全局引用。
//
// ⚠ 色值锚定 `assets/logo.png`（本机采样，非肉眼估）：
//     头发主蓝 #59659B（占 9.2%，签名色）／发丝高光 #7AA3CD→#95B6E9
//     裙装藏青 #30304E~#3B3C62／围裙暖白 #FDF4F5
// 所以下面三件事是一体的，改一件要一起想：
//   ① 中性色阶是**藏青**不是纯灰（取裙装的色相，压到近黑／提到近白）
//   ② 玻璃层的结构色随主题**反向**（暗色是白 α，浅色必须翻成深 α）
//   ③ 强调色是**紫蓝**不是薄荷 —— logo 里一个绿色像素都没有
//
// ⚠ 双主题：`dark` 由 Rust 唯一写入（`theme::resolve` 的结果），UI 不参与判断，
// 系统态（system_dark）甚至不进本文件。每格写成 `dark ? 暗 : 浅` 是 Slint 自己的
// 形状 —— 见 i-slint-compiler-1.18.0/widgets/fluent/styling.slint:36。
// 浅色侧每个值都在规格 §4.3 验算过 WCAG，改动要走 test(theme) 的对比度回归测试。
//
// 浅色侧要点：accent 用 logo **真实**主蓝 #59659B（暗色侧是提亮版 #9AA6E8）——
// 浅底上必须压深才够 4.87，压深后正好落回 logo 原色。warn/danger 同理必须压深
// （#E3B341 / #F07178 在白底上只有 1.9 / 2.6，不可用）。
export global Tokens {
    /// 当前是否为暗色。Rust 写，其余令牌读。
    in-out property <bool> dark: true;

    out property <brush> canvas:        dark ? #08080F : #EDEFF7;  // 最深/最浅一层
    out property <brush> core-top:      dark ? #12121C : #FFFFFF;  // 卡片内胎（渐变上端）
    out property <brush> core-bottom:   dark ? #0D0D15 : #F6F7FB;  // 卡片内胎（渐变下端）
    out property <brush> shell:         dark ? #EEF0FF06 : #2A2E4A03; // 外碗底
    out property <brush> hairline:      dark ? #EEF0FF12 : #2A2E4A09; // 1px 结构线
    out property <brush> hairline-soft: dark ? #EEF0FF0A : #2A2E4A05;
    out property <brush> hairline-strong: dark ? #EEF0FF33 : #2A2E4A22; // 输入类控件的描边
    out property <brush> inner-light:   dark ? #EEF0FF0E : #FFFFFF99; // 内胎顶缘高光

    out property <brush> ink:           dark ? #EFEFF7 : #191B2A;  // 主文字
    out property <brush> ink-2:         dark ? #AFB0C6 : #4C4F66;  // 正文
    out property <brush> ink-3:         dark ? #7C7D95 : #666980;  // 次级
    out property <brush> ink-4:         dark ? #63647C : #82859B;  // 标签 / 眉标

    out property <brush> accent:        dark ? #9AA6E8 : #59659B;  // 唯一强调色
    out property <brush> accent-ink:    dark ? #BCC5F7 : #3A4270;  // 强调色上的文字
    out property <brush> good:          dark ? #7FB4E4 : #2E6C9E;  // 信息 / 成功
    out property <brush> warn:          dark ? #E3B341 : #8A6200;  // ⚠ 不取自 logo：金蝶结只占 67px(0.05%)

    out property <brush> cta-top:       dark ? #F8F6FA : #59659B;  // 主 CTA（浅色为实心强调色）
    out property <brush> cta-bottom:    dark ? #DCDAE6 : #59659B;
    out property <brush> cta-ink:       dark ? #0B0B14 : #FFFFFF;
    // ⚠ 悬停方向**相反**：暗色白胶囊变亮，浅色实心胶囊必须变深 ——
    // 变浅会让白字掉到 4.36，不达标。
    out property <brush> cta-hover-top:    dark ? #FFFFFF : #4C5788;
    out property <brush> cta-hover-bottom: dark ? #E6E7F2 : #4C5788;
    // 图标井：暗色是白胶囊里的黑井；浅色 CTA 是实心强调色，井翻成白井。
    out property <brush> well:          dark ? #00000024 : #FFFFFF2E;
    out property <brush> well-hover:    dark ? #0000001F : #FFFFFF38;

    out property <brush> glow-brand:    dark ? #9AA6E81F : #59659B14; // 强调色的氛围光
    out property <brush> glow-blue:     dark ? #7FB4E417 : #7FB4E41A;
    out property <brush> top-light:     dark ? #EEF0FF0B : #FFFFFFB3; // 顶部一线竖直渐变
    out property <brush> shadow:        dark ? #00000080 : #2A2E4A33; // 浮层投影
    out property <length> r-lg: 22px;
    out property <length> r-md: 16px;
    out property <length> r-sm: 11px;
    out property <length> font-hero: 27px;

    // ── 控制面：所有可点元素共用同一套填充，不允许各自写白百分比 ─────────────
    // 收敛前散着 #FFFFFF08/0D/12/14/17/1F 六个值，四个按钮族各挑一个，
    // 于是"同一档悬停"在四个地方是四种亮度。
    out property <brush> fill:        dark ? #EEF0FF0D : #2A2E4A05; // 静止
    out property <brush> fill-hover:  dark ? #EEF0FF17 : #2A2E4A09; // 悬停
    out property <brush> fill-active: dark ? #EEF0FF1F : #2A2E4A0D; // 按压 / 已展开

    // ── 槽与浮层 ──────────────────────────────────────────────────────────
    out property <brush> sunk:        dark ? #00000059 : #2A2E4A06; // 下凹槽底（输入框 / 下拉框）
    out property <brush> solid:       dark ? #15151F : #FFFFFF;   // 浮层底：不透明，才压得住下层文字
    out property <brush> overlay:     dark ? #000000B8 : #1A1C2E66; // 模态遮罩

    // ── 强调色的四档（静止填充 / 悬停填充 / 描边 / 徽标底）──────────────────
    out property <brush> accent-fill:       dark ? #9AA6E81A : #59659B14;
    out property <brush> accent-fill-hover: dark ? #9AA6E82E : #59659B24;
    out property <brush> accent-line:       dark ? #9AA6E847 : #59659B3D;
    out property <brush> accent-soft:       dark ? #9AA6E812 : #59659B0F;
    out property <brush> warn-soft:         dark ? #E3B34112 : #8A620012;

    out property <brush> danger:      dark ? #F07178 : #9C3F41;   // 错误（日志行）
    out property <brush> lamp-off:    dark ? #494C60 : #B0B3C2;   // 熄灭的状态灯

    // ── 动效与状态（两套主题共用，不参与主题）──────────────────────────────
    // 三档时长：读数变色 400ms / 配色切换 200ms / 按压回弹 150ms。
    // 收敛前是 160/200/260/300/400 五档，差别全在"当时顺手写了多少"。
    out property <duration> motion-slow:  400ms;
    out property <duration> motion-hover: 200ms;
    out property <duration> motion-tap:   150ms;
    /// 状态栏高度。⚠ 两处必须一致：主布局靠 `padding-bottom: Tokens.status-h`
    /// 给贴底的状态栏让位，状态栏自己用它定高 —— 不一致就会出现"内容压在栏上"
    /// 或者"栏上方一条谁也解释不清的空白"。
    out property <length> status-h: 34px;
    /// 不可用元素整体压暗的统一档位（收敛前 0.30 / 0.34 两个值）。
    /// 用 opacity 表达，故两套主题同样成立，不需要按主题分叉。
    out property <float> disabled: 0.35;
}
```

- [ ] **Step 3: 吸收 5 处散落字面量**

1. `:136` `Orb` 的默认 tint 改为引用**氛围光**令牌（⚠ 不是 `Tokens.accent` —— 那是**不透明**色，拿它当 tint 会让缺省光晕变成一块实心色斑；两个调用点传的都是 `glow-*`，缺省必须同档）：

```slint
    in property <brush> tint: Tokens.glow-brand;
```

2. `:241` 图标井改用新令牌：

```slint
                background: emphasis
                    ? (touch.has-hover ? Tokens.well-hover : Tokens.well)
                    : (touch.has-hover ? Tokens.fill-active : Tokens.fill-hover);
```

3. `:268` CTA 悬停渐变改用新令牌：

```slint
                ? @linear-gradient(180deg, Tokens.cta-hover-top 0%, Tokens.cta-hover-bottom 100%)
```

4. `:471` Flyout 阴影：

```slint
    drop-shadow-color: Tokens.shadow;
```

5. `:1190` 顶部光：

```slint
        background: @linear-gradient(180deg, Tokens.top-light 0%, transparent 100%);
```

- [ ] **Step 4: 运行对比度回归测试确认转绿**

Run: `cargo test theme::tests 2>&1 | tail -12`
Expected: `both_palettes_meet_text_contrast_bars` 与 `translucent_tiers_stay_visible_in_both_palettes` 都 PASS。

- [ ] **Step 5: 确认编译与令牌块外再无颜色字面量**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished`，0 警告。

Run: `awk '/global Tokens \{/{inblk=1} /^\}/{inblk=0} !inblk' ui/app.slint | grep -n '#[0-9A-Fa-f]\{6,8\}'`
Expected: 只剩注释行（`//` 开头）。**任何非注释行都是漏抽的字面量**，必须补成令牌。
（当前应为 4 个命中，全部是文档注释里引用的 logo 锚点色。）

⚠ 这个 awk 用**块相对**切法，而不是早先写的行号阈值 `awk -F: '$1>115'` —— 那个阈值在双主题改写后
**已经过期**：令牌块从 `:43` 撑到 `:137`，`>115` 会把块内 7 行令牌定义当成"块外字面量"报出来。

⚠⚠ 锚点**不能带 `^`**：声明行是 `export global Tokens {`（Task 4 把它从 `global` 改成了 `export global`），
所以 `/^global Tokens \{/` 永远不匹配 —— `inblk` 恒为 0、`!inblk` 恒为真、整个文件穿透，
实测会返回 **45** 个假阳性。必须是 `/global Tokens \{/`（或不带锚点）。
这条是 Task 3+4 实测抓到的：我第一次修这个命令时正是把 `^` 加了进去，等于用一个 bug 换掉了另一个。

- [ ] **Step 6: 确认 `dark` 开关真的驱动了颜色（临时验证，随后还原）**

Run: `cargo test theme::tests::both_palettes_meet_text_contrast_bars 2>&1 | tail -4`（应 PASS）
然后**手动**把 `canvas` 的浅色值 `#EDEFF7` 临时改成 `#000000`，重跑上面这条命令。

Expected: **FAIL**，报"浅色 canvas 相关对比度不足"。这证明测试真的在读文件、真的会因错值而红（而不是恒真）。改回 `#EDEFF7`，重跑确认 PASS。

⚠ 同时确认**浅色 `ink` 的报错数字**：`#191B2A` 落在 `#000000` 上对比度应约为 **1.23**（Task 3+4 实测值）。
若报出的数字与此相差甚远，说明测试读的不是 `ui/app.slint` 的真实值 —— 那就是假绿，必须查清再提交。

- [ ] **Step 7: 提交**

```bash
git add ui/app.slint
git commit -m "feat(ui): Tokens 双主题（浅色版逐令牌验算）+ 吸收 5 处散落字面量"
```

---

### Task 5: Windows 系统主题探测

**Files:**
- Modify: `Cargo.toml`（加目标依赖）
- Modify: `src/theme.rs`（加探测实现与单测）

**Interfaces:**
- Consumes: 无（只依赖 Windows API）
- Produces: `theme::system_dark() -> bool`

> **与 winit 同源是硬要求**：winit 用 `uxtheme.dll` 序号 132 的 `ShouldAppsUseDarkMode()` 给系统标题栏定色（`winit-0.30.13/src/platform_impl/windows/dark_mode.rs:130`）。我们若改用注册表读，某些机器上会出现"应用主体变暗而标题栏不变"。故主路径同源，注册表只作回落。

- [ ] **Step 1: 写失败的测试**

`src/theme.rs` 的 `mod tests` 内追加：

```rust
    /// ⚠ 这条**不能**断言具体值 —— 值取决于跑测试时系统的主题设置。
    /// 它能抓住的是：FFI 不崩、不吃到空指针、两次调用自洽。
    /// 序号写错或 `GetProcAddress` 结果被误用时，这里会直接段错误而不是返回。
    #[test]
    fn system_dark_is_callable_and_stable() {
        let a = system_dark();
        let b = system_dark();
        assert_eq!(a, b, "同一时刻两次探测必须一致");
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test theme::tests::system_dark 2>&1 | tail -12`
Expected: 编译错误 —— `cannot find function system_dark`。

- [ ] **Step 3: 加依赖**

`Cargo.toml` 末尾追加（⚠ 用 `"0.61"` 而非 `"0.61.2"`：0.61.2 已在 `Cargo.lock` 中，该区间解析到它，**不新增编译单元**）：

```toml
# 系统主题探测与变更监视。⚠ 钉在 0.61 —— 该版本已在 lock 里（winit 等 12 处
# 依赖使用它），故不新增 crate。手写等价 FFI 要跨 3 个 DLL 且自己管 HKEY/HANDLE
# 成对释放，每处都是测试抓不到的泄漏机会。
#
# ⚠ `Win32_Security` 是 `CreateEventW` 的**必需** feature，不是可选：
# windows-sys 把签名里带 `SECURITY_ATTRIBUTES` 的函数整体 cfg 在它后面
# （已核实 windows-sys-0.61.2/src/Windows/Win32/System/Threading/mod.rs）。
# 少了它 `CreateEventW` 根本不存在，而事件对象是"无竞态武装通知"的前提。
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = [
    "Win32_Foundation",
    "Win32_Security",
    "Win32_System_LibraryLoader",
    "Win32_System_Registry",
    "Win32_System_SystemInformation",
    "Win32_System_Threading",
    "Win32_UI_Accessibility",
    "Win32_UI_WindowsAndMessaging",
] }
```

⚠ `Win32_System_SystemInformation` 是给 `dark_mode_supported` 里的 `OSVERSIONINFOW` 用的。
⚠ `RtlGetVersion` 本身在 windows-sys 里属 `Win32::Wdk::System::SystemServices`（要再拉一整棵 `Win32_Wdk`）；
本计划**故意不用它**，改为像取 uxtheme 那样从 ntdll 用 `GetProcAddress` 按名取 —— 少一个 feature 树，
且与本文件既有的取法一致。同一个 crate，故仍不新增编译单元。

- [ ] **Step 3b: 编译确认依赖本身没问题**

Run: `cargo build 2>&1 | tail -6`
Expected: **`Finished`，0 警告** —— 注意这里**不会**报 `cannot find function system_dark`，
因为 `cargo build` 不编译 `#[cfg(test)]`，而引用它的那个测试在测试模块里。
（初稿把这里写成"应报 Step 2 那条 `cannot find function`"是错的，Task 5 实测指出：
`cargo build` 根本看不到那个引用。RED 只能从 `cargo test` 得到。）
本步只用来看**依赖本身**是否就位：**不得**出现 `unresolved import` 或 feature 相关错误。
若报 feature 名不存在，说明 `"0.61"` 解析到了别的补丁版本，改回精确 `"0.61.2"`。

再 Run: `git diff Cargo.lock | grep -E "^\+" | grep -v "^+++"`
Expected: 只多出一条 **依赖边**（`+ "windows-sys 0.61.2",`，挂在 dsh-manager 名下），
**不得**出现新增的 crate 版本条目 —— 那才叫"新增编译单元"。

- [ ] **Step 4: 写实现**

在 `src/theme.rs` 的 `resolve` 之后插入：

```rust
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
#[cfg(not(windows))]
pub fn system_dark() -> bool {
    false
}
```

⚠ **不要**再加 `#[cfg(windows)] use std::os::windows::ffi::OsStrExt;`。本计划初稿要求加它，是因为
`registry_dark()` 里有两个 `encode_wide()` 调用 —— 而 `registry_dark()` 已按 Ruling 17 删除，
这行 import 于是没有使用者，加进去会**实测**产生 `warning: unused import`（1 条），直接违反 0 警告规则。
Task 5 实测后删除该行并在原位留了说明。若 Task 6 的通知路径需要宽字符串，**届时**再按需引入。

⚠ 三处需要核实后可能要微调（`windows-sys` 0.61 的签名细节）：`c"uxtheme.dll"` 需要 Rust 2021+ 的 C 字符串字面量（本仓库 edition 2024，可用）；`(&raw mut x).cast()` 需要 Rust 1.82+。若 `HIGHCONTRASTW` 的字段名不符，查 `windows-sys-0.61.2/src/Windows/Win32/UI/Accessibility/mod.rs` 后按其定义写。

⚠ **过渡期的 `dead_code` 抑制**（Task 5 实测，不是推断）：`system_dark` 与 `PERSONALIZE_KEY` 的消费者都在
Task 6，所以本步落地后 `cargo build` 会报若干条 dead_code（`system_dark` / `uxtheme_dark` /
`dark_mode_supported` / `PERSONALIZE_KEY`）。本步加**两处**逐项 `#[allow(dead_code)]`：
一处挂在 `system_dark` 上（rustc 把带 allow 的项当存活根，其私有被调者随之存活，故 `uxtheme_dark`
与 `dark_mode_supported` 一并被覆盖），一处挂在 `PERSONALIZE_KEY` 上。
**不要**加模块级或 crate 级 blanket。`registry_dark` 已按 Ruling 17 删除，故不在列表里。

同时 **`#[cfg(not(windows))]` 的那个桩也要各加一处**：它没有任何消费者，非 Windows 构建同样会因
0 警告规则而红，而保留该桩的全部意义就是让别处也能编译。这也是一处 allow，不是 blanket。

⚠ 文件里既有的 `#[allow(dead_code)]` 行号会随每次改动漂移（Task 5 落地后实测为 **6 处**：
`ThemeMode` / `impl ThemeMode` / `resolve` / `PERSONALIZE_KEY` / `system_dark` / 非 Windows 桩）。
引用它们时**一律用 grep，不要用行号**。

⚠ **顺序约束**：Task 9 删抑制必须发生在 **Task 6 与 Task 8 之后** —— Task 1 那三处抑制的消费者正是
Task 2 / 6 / 8，提前删会让那些符号重新变成 dead_code 而报错。Task 9 是最后一个任务，天然满足；
若将来调整任务顺序，这条要一起看。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test theme::tests::system_dark 2>&1 | tail -8`
Expected: PASS（`ok. 1 passed`）。若段错误 → 序号或 `transmute` 写错。

- [ ] **Step 6: 人工核对与系统当前设置是否一致**

Run: `cargo test theme::tests::system_dark -- --nocapture 2>&1 | tail -6`
再在 PowerShell 里读系统设置对照：

```powershell
Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' | Select-Object AppsUseLightTheme
```

Expected: `AppsUseLightTheme = 1` 时测试内部的 `system_dark()` 应为 `false`；`= 0` 时为 `true`。若不符，说明主路径（uxtheme）与注册表不一致 —— 记录到 `docs/VERIFICATION.md`，因为这正是不该发生的情况。

- [ ] **Step 7: 提交**

```bash
git add Cargo.toml Cargo.lock src/theme.rs
git commit -m "feat(theme): Windows 系统主题探测（与 winit 同源 + 注册表回落）"
```

---

### Task 6: 变更监视线程、消息与状态接入

**Files:**
- Modify: `src/model.rs:279-293`（`UiMsg`）
- Modify: `src/theme.rs`（加 `spawn_watcher`）
- Modify: `src/main.rs`：`AppState`（约 `:60-95`）、`AppState::new`（约 `:103`）、`drain()`（`:360` 起）、`main()` 的 `AppState::new(...)` 调用处

**Interfaces:**
- Consumes: `theme::system_dark()`（Task 5）、`theme::resolve`（Task 1）
- Produces:
  - `model::UiMsg::SystemThemeChanged(bool)`
  - `theme::spawn_watcher(tx: std::sync::mpsc::Sender<model::UiMsg>)`
  - `AppState { … theme_mode: ThemeMode, system_dark: bool }`
  - `AppState::new(preferred_port: u16, close_behavior: CloseBehavior, theme_mode: ThemeMode) -> Self`

> **本任务刻意把"消息定义 + 它的处理 + 状态字段"放在一起提交。** 只加变体而不加 `drain()` 分支会让构建红着跨过后续任务，逼人用 `_ => {}` 兜底 —— 那正好把编译器替我们找漏的能力扔掉。这里一次做完，构建全程绿。

- [ ] **Step 1: 加消息变体**

`src/model.rs` 的 `UiMsg` 内追加（放在 `Failed` 之前）：

```rust
    /// 系统主题变了。⚠ 只带"现在是不是暗色"，不带模式 ——
    /// 模式归 Rust 的 AppState 管，与系统态在这里是正交的两件事。
    SystemThemeChanged(bool),
```

- [ ] **Step 2: `AppState` 加两个字段并接上 `drain()`**

`AppState` 结构体加：

```rust
    theme_mode: ThemeMode,
    /// 系统当前是否为暗色。**刻意只存在 Rust 侧** —— UI 拿不到也不需要，
    /// 它只需要知道 `resolve()` 之后的结果（那个进了 Tokens.dark）。
    system_dark: bool,
```

`AppState::new` 签名改为 `fn new(preferred_port: u16, close_behavior: CloseBehavior, theme_mode: ThemeMode) -> Self`，初始化列表加 `theme_mode,` 与 `system_dark: theme::system_dark(),`；`main()` 里的调用处补第三个实参（本步先传 `ThemeMode::default()`，Task 8 再改成读持久化值）。

`drain()` 的 `match msg` 内加。

⚠⚠ **不要在这里再 borrow 一次。** `drain()` 在 `match` **之外**已经持有
`let mut s = state.borrow_mut();`（`src/main.rs:371`，作用域覆盖整个 `match` —— 该函数自己在
`src/main.rs:540` 的注释里写明"`s` 已在本轮结束处释放"）。所以本臂里任何 `state.borrow()` /
`state.borrow_mut()` 都是**第二次**借用，而 `RefCell` 的借用是**运行期**检查的：
编译期一声不吭，第一次切系统主题就在 80ms timer 回调里 panic。

本计划初稿正是这么写的（一个内层 `state.borrow()` 加一个内层 `borrow_mut()`），
Task 6 用一次性探针实测到 `panicked at src/main.rs:489:43: RefCell already mutably borrowed`。
**正确写法是直接用在手的 `s`，先取值再改**：

```rust
            // 系统主题变了。⚠ 只有「跟随系统」档会因此改变外观：强制档下
            // resolved 不变，就不该置 dirty 触发一次无谓重绘。
            //
            // ⚠ **不在这里再 `state.borrow()`** —— 本臂之上 `drain` 已经持有
            // `s`（`state.borrow_mut()`，作用域覆盖整个 `match`）。RefCell 的
            // 借用是**运行期**检查的：多借一次编译期毫无提示，第一次切系统主题就
            // 直接 panic（实测 "RefCell already mutably borrowed"）。
            // 所以旧值一律从**已在手的** `s` 上取，先取完再改 —— 次序不变。
            UiMsg::SystemThemeChanged(dark) => {
                let was = theme::resolve(s.theme_mode, s.system_dark);
                let now = theme::resolve(s.theme_mode, dark);
                s.system_dark = dark;
                if was != now {
                    s.dirty = true;
                }
                changed |= was != now;
            }
```

⚠ 这类"看着对、编译过、跑起来才炸"的缺陷本计划已出现过一次同类（D3 的解析器形状），
共同点是**代码片段要与既有代码的上下文交互**：内层 borrow 的合法性取决于外层是否已持有借用。
改这类片段时必须先把**所在函数的既有作用域**读一遍。

- [ ] **Step 3: 编译确认变体已被穷尽处理**

Run: `cargo build 2>&1 | tail -6`
Expected: `Finished`，**0 警告**。若仍有 `non-exhaustive patterns`，说明 `drain()` 之外还有别的 `match` 在匹配 `UiMsg` —— 按编译器的指引逐个补上，不要写 `_ => {}`。

- [ ] **Step 4: 写监视线程**

`src/theme.rs` 追加。**无竞态顺序是硬要求**，理由见规格 §4.4：

```rust
/// 启动系统主题监视线程。任何变更都会经 `tx` 送回 UI 线程。
///
/// ⚠ 顺序**必须**是"先武装通知、再读值"：反过来（读 → 武装）会留下一个
/// 微秒级窗口，落在窗口里的变更**永远**不会被发现 —— 因为此后不再有变更
/// 来唤醒它。后果不是慢一拍，而是永久不一致：winit 的标题栏早已变色，
/// 应用主体却停在旧主题，直到用户下次再切主题才自愈。
#[cfg(windows)]
pub fn spawn_watcher(tx: std::sync::mpsc::Sender<crate::model::UiMsg>) {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY_CURRENT_USER, HKEY, KEY_NOTIFY,
        KEY_READ, REG_NOTIFY_CHANGE_LAST_SET,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, ResetEvent, WaitForSingleObject, INFINITE,
    };

    let sub: Vec<u16> = std::ffi::OsStr::new(PERSONALIZE_KEY)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    std::thread::spawn(move || {
        // 事件对象建一次、每轮复用；键句柄每轮开关（生命周期短且成对，避免长期持有）
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return;
        }
        loop {
            unsafe {
                let mut hkey: HKEY = std::ptr::null_mut();
                if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ | KEY_NOTIFY, &mut hkey)
                    != 0
                {
                    // 键打不开（极罕见）：退避后重试，不要退化成忙等
                    CloseHandle(event);
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    return;
                }
                // ① 先武装（异步：立即返回，变更时置位 event）
                let armed = RegNotifyChangeKeyValue(
                    hkey,
                    1, // bWatchSubtree
                    REG_NOTIFY_CHANGE_LAST_SET,
                    event,
                    1, // fAsynchronous = TRUE
                );
                // ② 再读值 —— 此刻之后发生的任何变更都会置位 event，不会丢
                let dark = system_dark();
                RegCloseKey(hkey);
                if armed != 0 {
                    // 武装失败：无法可靠监视，退化为定时重读（仍然不丢，只是有延迟）
                    if tx.send(crate::model::UiMsg::SystemThemeChanged(system_dark())).is_err() {
                        CloseHandle(event);
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    continue;
                }
                if tx.send(crate::model::UiMsg::SystemThemeChanged(dark)).is_err() {
                    // UI 线程已退出
                    CloseHandle(event);
                    return;
                }
                // ③ 等下一次变更
                let w = WaitForSingleObject(event, INFINITE);
                ResetEvent(event);
                if w != WAIT_OBJECT_0 {
                    CloseHandle(event);
                    return;
                }
            }
        }
    });
}

/// 非 Windows：没有系统主题可跟随，空实现。
#[cfg(not(windows))]
pub fn spawn_watcher(_tx: std::sync::mpsc::Sender<crate::model::UiMsg>) {}
```

⚠ 若 `KEY_READ | KEY_NOTIFY` 的类型不匹配，按 `windows-sys-0.61.2` 里 `REG_SAM_FLAGS` 的定义转换。`WAIT_OBJECT_0` 与 `u32::MAX`（INFINITE）类型以该版本定义为准。

- [ ] **Step 5: 加一句单测钉住"空实现也不炸"**

```rust
    #[test]
    fn watcher_sender_survives_a_dropped_receiver() {
        // 线程必须能容忍接收端先消失（UI 线程退出）而不 panic ——
        // 它是在 `send` 失败时 return，不是 unwrap。
        let (tx, rx) = std::sync::mpsc::channel::<crate::model::UiMsg>();
        drop(rx);
        spawn_watcher(tx);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
```

- [ ] **Step 6: 编译、测试、提交**

Run: `cargo build 2>&1 | tail -5` → Expected: `Finished`，0 警告。
Run: `cargo test 2>&1 | tail -6` → Expected: 全绿。

```bash
git add src/model.rs src/theme.rs src/main.rs
git commit -m "feat(theme): 系统主题变更消息、无竞态监视线程与状态接入"
```

---

### Task 7: 设置面板「主题」组与 UI 契约

**Files:**
- Modify: `ui/app.slint:1150`（属性区）、`:1163`（callback 区）、`:1927` 之后（设置面板「关闭行为」组之后）

**Interfaces:**
- Consumes: Task 4 的 `Tokens.*`（含新的条件表达式）、既有 `ChipButton` / `Eyebrow` / `Divider` 组件
- Produces: `MainWindow` 的 `in-out property <int> theme-mode` 与 `callback theme-mode-changed(int)`（Rust 侧名字分别为 `set_theme_mode` / `get_theme_mode` / `on_theme_mode_changed`）

- [ ] **Step 1: 加属性与 callback**

`ui/app.slint:1150` 附近（`close-behavior-index` 之后）加：

```slint
    /// 主题模式：0 = 浅色 / 1 = 深色 / 2 = 跟随系统。
    /// ⚠ 与 src/theme.rs 的 `ThemeMode` 下标一一对应，改顺序要两边一起改。
    /// 默认 2：与 `state.json` 里没记过时的缺省（跟随系统）一致。
    in-out property <int> theme-mode: 2;
```

`ui/app.slint:1163` 附近（`callback settings-changed(int);` 之后）加：

```slint
    callback theme-mode-changed(int);
```

- [ ] **Step 2: 加设置面板分组**

在「关闭行为」那组的说明文字 `Text { … }`（约 `:1928-1934`）之后、`Bezel` 结束之前插入：

```slint
                Rectangle { height: 16px; }
                Divider { }
                Rectangle { height: 16px; }

                Eyebrow { text: "主题" }
                Rectangle { height: 10px; }
                HorizontalLayout {
                    spacing: 8px;
                    ChipButton {
                        label: "浅色";
                        accent: root.theme-mode == 0;
                        clicked => { root.theme-mode-changed(0); }
                    }
                    ChipButton {
                        label: "深色";
                        accent: root.theme-mode == 1;
                        clicked => { root.theme-mode-changed(1); }
                    }
                    ChipButton {
                        label: "跟随系统";
                        accent: root.theme-mode == 2;
                        clicked => { root.theme-mode-changed(2); }
                    }
                }
                Rectangle { height: 12px; }
                Text {
                    // ⚠ 这里必须诚实交代标题栏的限制，否则用户会当成 bug 报：
                    // 强制档只改窗口内部，系统标题栏跟着的是**系统**设置。
                    text: "跟随系统时窗口与标题栏会一起变。选「浅色」「深色」只改窗口内部 —— 系统标题栏由 Windows 画，它跟的是系统设置，本程序无法覆盖。";
                    color: Tokens.ink-4;
                    font-family: Font.text;
                    font-size: 11.5px;
                    wrap: word-wrap;
                }
```

- [ ] **Step 3: 编译确认两份契约未破**

Run: `cargo build 2>&1 | tail -8`
Expected: 仍是 Step 2 里那条 `non-exhaustive patterns`（Task 8 待办），**不得**出现 Slint 语法错误或 `Cannot access id`。

- [ ] **Step 4: 确认设置面板高度够用**

面板宽度 400px，三枚中文 ChipButton（浅色 / 深色 / 跟随系统）加间距约 200px，装得下；但「关闭行为」组的三枚（隐藏至托盘 / 彻底退出 / 每次询问）已较宽，两组合计不会同排。**必须**跑一次 Task 9 的截图矩阵确认第二组没有被裁掉 —— Slint 的 `VerticalLayout` 会把 `Bezel` 撑高，理论上不会被裁，但这是像素级结论，必须眼看。

---

### Task 8: Rust 接线

**Files:**
- Modify: `src/main.rs`：`use` 区（`mod` 后）、`AppState`（约 `:60-95` 的 struct 与 `:103` 的 `new`）、`project()`（`:249`，`:349` 附近）、`drain()`（`:360` 起，处理新变体）、`wire_callbacks()`（`:1196` 之后）、`main()`（`:1378` 的 `config::load()` 与 `:1392` 的窗口构造）

**Interfaces:**
- Consumes: `theme::{ThemeMode, resolve, spawn_watcher}`、`config::StateFile::theme_mode`、Task 6 已建好的 `AppState.theme_mode` / `AppState.system_dark`、Slint 侧 `Tokens` / `theme-mode` / `theme-mode-changed`
- Produces: 可运行的双主题应用

`AppState` 的字段与 `drain()` 的分支已在 Task 6 就位，本任务只做**投影、回调、启动顺序**三件事。

- [ ] **Step 1: `project()` 推主题**

在 `project()` 里「关闭行为」那两行之后加（**必须每帧都推** —— 与既有注释同理由：设置面板改的是 `AppState`，而唯一点投影点是本函数）：

```rust
    // ── 主题（Rust 是唯一真相源）──
    // mode 推给界面画选中态；resolved 推给 Tokens 驱动全部颜色。
    // ⚠ `Tokens` 是 `export global`，故 Rust 能拿到 setter：生成代码里
    // `impl slint::Global<'a, MainWindow> for Tokens<'a>` + `pub fn set_dark`。
    win.set_theme_mode(state.theme_mode.index());
    win.global::<Tokens>().set_dark(theme::resolve(state.theme_mode, state.system_dark));
```

`Tokens` 由 `slint::include_modules!()`（`src/main.rs:23`）带到 crate 根，无需额外 `use`；`global()` 需要 `ComponentHandle`，已在 `:18` 导入。

- [ ] **Step 2: 接线 callback**

在 `wire_callbacks()` 里「关闭行为」那块之后加（**逐字照抄该块的形状**：改 `AppState`、置 dirty、立即落盘、写失败只记日志）：

```rust
    // ── 主题模式 ──
    //
    // 与「关闭行为」同款：只有一条投影路径（落 AppState → `project()` 推回），
    // 点击不发任何 Job，所以必须置 dirty —— 稳态下没有消息可排空。
    {
        let state = state.clone();
        win.on_theme_mode_changed(move |idx| {
            let Some(mode) = ThemeMode::from_index(idx) else { return };
            {
                let mut s = state.borrow_mut();
                s.theme_mode = mode;
                s.dirty = true;
            }
            // 偏好立即落盘，设置面板里没有"保存"按钮。
            // 写失败只记日志、不回滚 —— 回滚界面上的选择会让"点了没反应"，
            // 而这次选择在本次运行里已经生效。
            if let Err(e) = config::update(|f| f.theme_mode = Some(mode)) {
                push_log(&state.borrow(), format!("主题设置保存失败（本次运行仍生效）：{e}"));
            }
        });
    }
```

- [ ] **Step 3: `main()` 读持久化值、设初始值、起监视**

`config::load()` 那个 `match` 的四个分支都要带上 `theme_mode`。改法：把元组扩成四元，`Ok` 分支读 `s.theme_mode.unwrap_or_default()`（= `Auto` = 跟随系统），其余三个分支用 `ThemeMode::default()`：

```rust
    let (preferred_port, close_behavior, theme_mode, startup_note) = match config::load() {
        config::Loaded::Ok(s) => (
            s.preferred_port.unwrap_or(3080),
            s.close_behavior.unwrap_or_default(),
            s.theme_mode.unwrap_or_default(),
            None,
        ),
        config::Loaded::Missing => (3080, CloseBehavior::default(), ThemeMode::default(), None),
        config::Loaded::Corrupt(e) => (
            3080,
            CloseBehavior::default(),
            ThemeMode::default(),
            Some(format!("state.json 损坏，使用缺省端口 3080：{e}")),
        ),
        config::Loaded::NoLocation => (3080, CloseBehavior::default(), ThemeMode::default(), None),
    };
```

`AppState::new` 调用处补第三个实参：`AppState::new(preferred_port, close_behavior, theme_mode)`。

**在 `Timer` 启动之前**加监视（顺序理由：先起监视，保证启动瞬间到事件循环就绪之间的系统主题变化不丢）：

```rust
    // 系统主题监视。⚠ 必须在 `run()` 之前起 —— 否则启动瞬间到事件循环就绪
    // 之间的变更会丢。消息走既有 UiMsg 通道，由 80ms timer 排空。
    theme::spawn_watcher(msg_tx_for_theme.clone());
```

`msg_tx_for_theme` 用既有的 worker→UI 发送端克隆（`drain` 用的那一个）。**执行时先读 `main()` 里该 sender 的实际变量名**，不要照抄这里的占位名。

**首帧之前把初始主题推下去**（否则首帧会用 `Tokens.dark` 的声明缺省 `true`，在浅色系统上闪一下）：

```rust
    // 首帧之前的初始主题。⚠ `Palette.color-scheme` 由 .slint 的 `changed` 处理器
    // 跟着 `Tokens.dark` 走（见 ui/app.slint 的 MainWindow），这里只写 Tokens。
    win.global::<Tokens>().set_dark(theme::resolve(theme_mode, theme::system_dark()));
```

- [ ] **Step 4: 在 `ui/app.slint` 里同步 Fluent 的配色方案**

`src/main.rs` 无法直接触达 Fluent 的 `Palette`（std-widgets 的全局不在生成代码的公开访问器里 —— 已实测确认）。**已验证可行**的写法是 `changed` 处理器里的赋值（绑定形态 `Palette.color-scheme: …;` 是语法错误，成因见规格 §2 F6）。

把 `ui/app.slint:1086` 的 `init => { Palette.color-scheme = ColorScheme.dark; }` 替换为：

```slint
    // ⚠ Fluent 控件（ScrollView 滚动条 / AboutSlint）的配色派生自 Palette.color-scheme，
    // 而 FluentPalette 是 std-widgets 的全局，Rust 侧拿不到访问器 ——
    // 所以必须在本文件里跟着 `Tokens.dark` 走。
    //
    // ⚠ 只能写成"局部属性 + changed 处理器里的赋值"：
    // 组件体里直接写 `Palette.color-scheme: …;` 是 Parse error（限定名走不进
    // parser 的绑定分支，见 parser/element.rs 的 parse_element_content），
    // 而 `changed` 里的赋值走的是语句路径，可用。
    // `init` 那行负责首帧，`changed` 负责运行期切换，两者缺一不可。
    property <bool> is-dark: Tokens.dark;
    changed is-dark => {
        Palette.color-scheme = is-dark ? ColorScheme.dark : ColorScheme.light;
    }
    init => { Palette.color-scheme = is-dark ? ColorScheme.dark : ColorScheme.light; }
```

`ColorScheme.dark` / `.light` 是既有代码已在用的名字（原 `:1086`）。**不要**写 `ColorScheme.unknown` —— 那会让 Fluent 跟随系统，与我们自己的强制档打架。

- [ ] **Step 5: 编译、测试、跑起来**

Run: `cargo build 2>&1 | tail -8`
Expected: `Finished`，0 警告（Step 2 的 `non-exhaustive patterns` 到此消失）。

Run: `cargo test 2>&1 | tail -6`
Expected: 全绿（原 95 项 + 本轮新增约 12 项）。

- [ ] **Step 6: 提交**

```bash
git add src/main.rs src/model.rs src/theme.rs ui/app.slint Cargo.toml Cargo.lock
git commit -m "feat(theme): 明暗双主题接线（探测/监视/持久化/设置面板）"
```

---

### Task 9: 文档留档与验证记录

**Files:**
- Modify: `docs/ARCHITECTURE.md`（加主题子系统一节）
- Modify: `docs/RULINGS.md`（记录平台事实与设计后果）
- Modify: `docs/VERIFICATION.md`（记本轮实测）

- [ ] **Step 1: 删除过渡期的 `dead_code` 抑制，并确认零警告**

**本步要删掉 `src/theme.rs` 里所有 `#[allow(dead_code)]`** —— 到这一步三批消费者（Task 2 / 6 / 8）都已落地，
抑制没有存在理由了。

⚠ **不要按行号找，用 grep**。Task 5 在文件头部加了一组 `use`，把行号整体下移过，而且抑制的**条数**也不是
常量：Task 1 加了三处（`ThemeMode` / `impl ThemeMode` / `resolve`），Task 5 又加了两处（`system_dark`
与其非 Windows 桩）。实测当前为 5 处（`:21` `:36` `:73` `:97` `:188`），但以 grep 结果为准。

Run: `grep -n "allow(dead_code)" src/theme.rs`
Expected: **无输出**（`grep` 退出码 1）。

⚠ 注意这里没有"只剩注释行"之说 —— 移除契约的注释里**并不含** `allow(dead_code)` 这个字面量
（Task 1 复审实测确认：注释命中数为 0），所以删干净后应当一个匹配都没有。
**若仍有匹配，那就是还有没删的生效属性**，逐个删掉并重跑本步。

删除后 Run: `cargo build 2>&1 | grep -c "^warning"`
Expected: `0`。

⚠ **若删掉后仍有 `dead_code` 警告**：那是**真实死代码**，按仓库规矩（Global Constraints「本仓库不接受
allow 掩盖死代码」）删除该代码，**不得**恢复抑制行。这一点是 Ruling 10 的"代价若错"所指，由本步兜住。

- [ ] **Step 2: 跑完整验证矩阵**

**截图矩阵**（复用 `target/shot.ps1` 的做法：启动 → `PrintWindow` → 杀进程）。⚠ **不得点击任何控件**，且运行前后都要核对 `state.json` 与 `3080` 的 pid 未变：

```powershell
# 依次把 state.json 的 theme_mode 置为 null / "light" / "dark" / "auto"，
# 各启动一次并截图到 target/theme-<档位>.png
```

对每张图**回读像素**（复用 `target/readback.py` 的思路）确认：浅色档的 canvas 接近 `#EDEFF7`、`accent` 渲染色接近 `#59659B`；深色档 canvas 接近 `#08080F`、accent 接近 `#9AA6E8`。

**实时跟随实测**：应用以「跟随系统」运行，改系统主题（设置 → 个性化 → 颜色 → 选择模式），确认窗口主体**与标题栏同时**变化，并记录延迟。

**持久化实测**：设置里选「深色」→ 退出 → 重启 → 仍是深色且三个 ChipButton 的选中态正确。

**旧文件兼容实测**：把 `state.json` 换回三字段版本启动，确认不崩、以跟随系统启动。

Expected: 全部符合。任何不符都记进 `docs/VERIFICATION.md` 的备注，**不得**只写"通过"。

- [ ] **Step 3: `docs/ARCHITECTURE.md` 加一节**

新增主题子系统小节，至少覆盖：`src/theme.rs` 的职责边界、Rust 为唯一真相源、`Tokens.dark` 与 `Palette.color-scheme` 两条投影路径及为何是两条、`UiMsg::SystemThemeChanged` 与 80ms 排空的关系。**注明 `system_dark` 刻意不进 .slint**，以免下一个人"顺手"把它加到 UI 属性里从而出现两个真相源。

- [ ] **Step 4: `docs/RULINGS.md` 记三条平台事实**

按本仓库惯例（平台事实要留档，避免下一个人重新踩），逐条记录并写明设计后果：

1. **应用读不到系统主题** —— `SlintInternal.color-scheme` 的不可访问是编译器语法测试断言的行为（`tests/syntax/lookup/global.slint:38`），`SlintContext::color_scheme()` 只在 `private_unstable_api`。后果：「跟随系统」必须自己探测。
2. **winit 的探测只作用于系统标题栏**，且用的是 `uxtheme.dll` 序号 132 而非注册表（`winit-0.30.13/src/platform_impl/windows/dark_mode.rs:130`），Slint 也无公开 API 覆盖窗口主题。后果：① 我们的探测必须与 winit 同源，否则主体与标题栏不一致；② **强制明/暗改不动系统标题栏**是平台限制，必须在设置面板里向用户交代。
3. **`Palette.color-scheme` 只能经 `changed` 处理器里的赋值切换**，绑定形态是 Parse error（成因：`parse_element_content` 只在 `Identifier Colon` 时走绑定分支，限定名进不去）。后果：Fluent 配色同步必须写成"局部属性镜像 + `changed`"。

- [ ] **Step 5: 提交**

```bash
git add docs/
git commit -m "docs: 主题子系统的架构、三条平台事实裁定与验证记录"
```

---

## Self-Review

**1. 规格覆盖**（逐节核对 `docs/superpowers/specs/2026-09-22-theme-system-design.md`）：

| 规格节 | 覆盖它的任务 |
|---|---|
| §3 数据流与职责 | Task 8（接线）、Task 6（消息）、Task 1（解析） |
| §4.1 令牌形状（`export global` + `dark`） | Task 4 |
| §4.2 暗色侧不改 | Task 4 Step 1（对表时逐行保留暗色现值） |
| §4.3 浅色完整表 | Task 4 Step 2（权威值表）+ Task 3（门槛测试） |
| §4.4 无竞态监视 | Task 6 Step 4（先武装后读，注释写明理由） |
| §5 四个新令牌 + 吸收 5 处字面量 | Task 4 Step 2/3 |
| §6 设置界面 | Task 7 |
| §7 持久化与旧文件兼容 | Task 2 |
| §8 已知限制（标题栏/高对比度/首帧顺序） | Task 5（高对比度）、Task 8 Step 3（首帧顺序）、Task 7 Step 2（向用户交代标题栏） |
| §9 验证计划 | Task 9 + Task 3 的回归测试 |
| §10 探针结论 | Task 4 Step 2（`export global`）、Task 8 Step 1（Rust setter）、Task 8 Step 4（`changed`） |
| §11 实施顺序 | 本计划的任务顺序即为该节展开 |
| §12 风险 | Task 4 Step 6（证明测试非恒真）、Task 9 Step 1（观感迭代的记录出口） |

**2. 占位符扫描**：计划内无 TBD / TODO / "类似 Task N"。唯一预设的占位名是 Task 8 Step 3 的 `msg_tx_for_theme`，已在该步内**显式要求执行者去读实际变量名**而非照抄 —— 因为该名字取决于 `main()` 里既有的 sender 变量，计划写死会制造一个编译错误。

**3. 类型与名字一致性**：`ThemeMode::{index, from_index, as_str, parse, default}`、`resolve(mode, system_dark) -> bool`、`system_dark() -> bool`、`spawn_watcher(Sender<UiMsg>)`、`UiMsg::SystemThemeChanged(bool)`、`Tokens::set_dark(bool)`、`Tokens::get_dark()`、`win.set_theme_mode(i32)`、`win.on_theme_mode_changed(i32)`、`StateFile::theme_mode: Option<ThemeMode>`、新增令牌 `well`/`well-hover`/`cta-hover-top`/`cta-hover-bottom`/`top-light`/`shadow` —— 各任务引用处已逐个核对一致。`CloseBehavior` 的既有 API 未改动任何签名。

**4. 任务边界与提交粒度**：每个任务结束时构建与测试都是绿的，9 个任务对应 9 个提交点。Task 6 刻意把"消息变体 + 它的 `drain()` 分支 + `AppState` 字段"放在**同一个**任务里 —— 若拆开，加完变体而没加分支会让构建红着跨过两个任务，执行者只能用 `_ => {}` 兜底，正好把"让编译器列出所有需要处理的位置"这个好处扔掉。

**5. 计划里的平台代码已对着依赖源码核过**（这几处是初稿写错后被查出来的，执行时不必再怀疑）：

| 事实 | 出处 |
|---|---|
| `HIGHCONTRASTW` 实现了 `Default`，字段为 `cbSize` / `dwFlags` / `lpszDefaultScheme` | `windows-sys-0.61.2/src/Windows/Win32/UI/Accessibility/mod.rs:464-472` |
| 高对比度标志用 `HCF_HIGHCONTRASTON`（= 1），不要写魔法数 | 同上 `:444` |
| **`CreateEventW` 被 `#[cfg(feature = "Win32_Security")]` 门住** → 该 feature 是必需项，不是可选 | `.../Win32/System/Threading/mod.rs` |
| `WAIT_OBJECT_0` 在 `Win32::Foundation`；`INFINITE` 在 `Win32::System::Threading` | `.../Win32/Foundation/mod.rs:10027`、`.../Threading/mod.rs` |
| `HKEY = *mut c_void`（故 `null_mut()` 正确）；`KEY_READ \| KEY_NOTIFY` 同为 `REG_SAM_FLAGS` 可合并 | `.../Win32/System/Registry/mod.rs:147` |
| `RegQueryValueExW` 的 `lptype` 是 `*mut REG_VALUE_TYPE`(u32)、`lpdata` 是 `*mut u8`、`lpcbdata` 是 `*mut u32` | 同上 |
| `Orb` 的默认 tint 必须是 `glow-brand`（带 α），**不能**是 `accent`（不透明）—— 否则缺省光晕会变成实心色斑 | `ui/app.slint:136` 与两个调用点实传的 `glow-*` |
