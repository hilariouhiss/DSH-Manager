# 明暗双主题与三种切换模式 —— 设计

日期：**2026-09-22**　状态：待评审　上游：`ui/app.slint`（当前单套暗色令牌）

目标一句话：**让应用有明、暗两套完整主题，用户可在「浅色 / 深色 / 跟随系统」之间切换，切换入口在设置里，默认跟随系统。**

---

## 1. 非目标（明确不做）

- **不做主题包 / 自定义配色。** 两套主题是常量，不是数据。
- **不做每令牌的独立配置。** 模式是唯一开关。
- **不换设计语言。** Ethereal Glass（hairline 玻璃、径向光晕、内高光）在浅色下逐条翻译，不降级成扁平风。
- **不改系统标题栏。** 见 §8 已知限制，这是平台限制不是取舍。
- **不引入会新增编译单元的依赖。** 系统主题探测用 `windows-sys`，但**钉在已在 lock 文件里的 0.61 版本**上（winit 等 12 处已编译它），故依赖数与编译量都不变。见下方「规格修订」。

**规格修订（2026-09-22，评审后）：** 本节原写"用 4 个 `extern "system"` 声明，不拉 `windows-sys`"，**该估算偏低**。要写成**无竞态**的监视（先武装通知、再读值）就需要事件对象，真实的手写 FFI 面是约 11 个函数、跨 3 个 DLL，并要自己管 `HKEY`/`HANDLE` 的成对释放 —— 每处都是一个不会被测试抓到的泄漏或 UB 机会。改用 `windows-sys` 后：没有新增 crate（0.61.2 已在依赖树中），而且比手写 FFI **更短**。竞态为何必须消除见 §3 监视一节。

---

## 2. 已验证的平台事实（对着依赖源码核实，非推断）

这一节是设计的支点，每条都写清出处，以便复核。

| # | 事实 | 出处 |
|---|---|---|
| F1 | **应用代码读不到系统主题。** `SlintInternal.color-scheme` 对用户代码是硬错误 | `i-slint-compiler-1.18.0/tests/syntax/lookup/global.slint:38` 断言的报错即 `Cannot access id 'SlintInternal'` |
| F2 | `SlintContext::color_scheme()` 不是公开 API（只在 `private_unstable_api.rs`） | `slint-1.18.0/private_unstable_api.rs:173` |
| F3 | **winit 自己探测了系统主题，但只喂给系统标题栏。** 它不读注册表，用 `uxtheme.dll` 序号 132 的 `ShouldAppsUseDarkMode()` + `SPI_GETHIGHCONTRAST` | `winit-0.30.13/src/platform_impl/windows/dark_mode.rs:62,126,130` |
| F4 | Slint 无公开 API 覆盖窗口主题（无 `set_theme`） | grep `slint-1.18.0` 无匹配 |
| F5 | 「每令牌写成 `dark ? 暗 : 浅` 条件表达式」是 Slint 自己的做法 | `i-slint-compiler-1.18.0/widgets/fluent/styling.slint:36`：`dark-color-scheme ? #1C1C1C : #FAFAFA` |
| F6 | 组件体里写 `Palette.color-scheme: v;` 是语法错误：`parse_element_content` 只在 `Identifier Colon` 时走绑定分支，限定名进不去 | `i-slint-compiler-1.18.0/parser/element.rs:58`（dispatch）与 `:443`（`parse_property_binding`） |
| F7 | 但 `changed <prop> => { Palette.color-scheme = …; }` 合法 —— 赋值走语句路径，不是绑定路径 | 见 §10 探针实测 |

**F1 + F3 的合并结论：** 系统主题必须我们自己探测；且**探测必须与 winit 同源**（同走 uxtheme 序号 132 并同样处理高对比度），否则「跟随系统」时应用主体与标题栏会各说各话。

---

## 3. 数据流与职责

```
Windows 主题
   │  读：uxtheme 序号132 ShouldAppsUseDarkMode() && !高对比度   （与 winit 同源，F3）
   │      取不到则回落 HKCU\...\Themes\Personalize\AppsUseLightTheme
   │  变：RegNotifyChangeKeyValue 监视该键（事件等待，不轮询）
   ▼
src/theme.rs ── system_dark: bool ────┐
                                      │
   mode: ThemeMode（0=浅 1=深 2=自动） │  ← 持久化于 state.json
                                      ▼
        resolved_dark = match mode { Light => false, Dark => true, Auto => system_dark }
                                      │
                     ┌────────────────┴────────────────┐
                     ▼                                 ▼
        ui.global::<Tokens>().set_dark(..)   Palette.color-scheme（经 changed 处理器）
        → 全部自绘令牌                        → Fluent：ScrollView 滚动条 / AboutSlint
```

**职责边界：**

- **Rust 是唯一真相源。** 模式与系统态都在 Rust 里合成 `resolved_dark`，UI 不参与判断。
- **UI 只上报意图。** 新增 `callback theme-mode-changed(int)`，与既有 `callback settings-changed(int)` 完全同形（`ui/app.slint:1163`）。
- **UI 只接收结果。** 新增 `in property <int> theme-mode`（供三个 ChipButton 显示选中态）。
- **`system_dark` 不进 .slint。** UI 拿不到也不需要 —— 它只需要知道"现在是不是暗色"，而那是 `resolved_dark`。

**复用既有骨架，不新建机制：**

- 后台线程 → `UiMsg` 通道 → 既有 80ms 排空 timer（`src/main.rs:675` 已有 `std::thread::spawn` + `mpsc` 先例）
- 持久化 → `config::update` + `StateFile`（`src/config.rs:58`）
- 设置项 → 既有设置面板（`ui/app.slint:1865`）

---

## 4. 令牌方案

### 4.1 形状

`Tokens` 由 `global` 改为 **`export global`**（Rust 需要访问器），并新增一个输入：

```slint
export global Tokens {
    in-out property <bool> dark: true;
    out property <brush> canvas: dark ? #08080F : #EDEFF7;
    // …其余令牌同形
}
```

- **单一 global，不拆两套。** 同一令牌的明暗两个值写在同一行，无法各自漂移；拆成 `Dark`/`Light` 两个 global 会多约 45 行间接层，且没有换来任何东西。
- **`dark` 是 `in-out`** 而非 `in`：Rust 要写它，`in-out` 才能被 `impl Global` 暴露 setter（见 §10 探针）。
- 代价：`ui/app.slint` 里约 45 行令牌各加一个 `dark ? … : …`。这是 **F5** 的官方形状。

### 4.2 暗色侧**不改动**

现有暗色值一律保持 —— 本次是"加一套浅色"，不是"重做暗色"。这样上一次提交（`9c5a478`，配色锚定 logo）的验证结论继续有效。

### 4.3 浅色侧完整表（对比度已按 WCAG 验算）

最坏情况是**最深**的浅色面（canvas），故文字对比度一律对 canvas 校验。

| 令牌 | 暗色（不变） | **浅色（新增）** | on canvas | 门槛 |
|---|---|---|---|---|
| canvas | `#08080F` | `#EDEFF7` | — | — |
| core-top | `#12121C` | `#FFFFFF` | — | — |
| core-bottom | `#0D0D15` | `#F6F7FB` | — | — |
| shell | `#EEF0FF06` | `#2A2E4A03` | — | — |
| hairline | `#EEF0FF12` | `#2A2E4A09` | 1.17 | >1.02 |
| hairline-soft | `#EEF0FF0A` | `#2A2E4A05` | 1.10 | >1.01 |
| hairline-strong | `#EEF0FF33` | `#2A2E4A22` | 1.50 | >1.10 |
| inner-light | `#EEF0FF0E` | `#FFFFFF99` | — | — ⚠ 见下注 |
| ink | `#EFEFF7` | `#191B2A` | 14.85 | 4.5 |
| ink-2 | `#AFB0C6` | `#4C4F66` | 6.99 | 4.5 |
| ink-3 | `#7C7D95` | `#666980` | 4.69 | 4.5 |
| ink-4 | `#63647C` | `#82859B` | 3.17 | 3.0 |
| **accent** | `#9AA6E8` | **`#59659B`** | 4.87 | 4.5 |
| accent-ink | `#BCC5F7` | `#3A4270` | 8.34 | 4.5 |
| good | `#7FB4E4` | `#2E6C9E` | 4.87 | 4.5 |
| warn | `#E3B341` | `#8A6200` | 4.78 | 4.5 |
| danger | `#F07178` | `#9C3F41` | 5.71 | 4.5 |
| cta-top | `#F8F6FA` | `#59659B` | — | — |
| cta-bottom | `#DCDAE6` | `#59659B` | — | — |
| cta-ink | `#0B0B14`（深字，白胶囊上 18.5） | `#FFFFFF`（白字，强调胶囊上 5.59） | — | 4.5 |
| cta-hover-top/bottom | 见 §5 | `#4C5788` | 白字 6.94 | 4.5 |
| well / well-hover | 见 §5 | `#FFFFFF2E` / `#FFFFFF38` | — | — |
| glow-brand | `#9AA6E81F` | `#59659B14` | 1.11 | >1.01 |
| glow-blue | `#7FB4E417` | `#7FB4E41A` | — | — |
| fill | `#EEF0FF0D` | `#2A2E4A05` | 1.10 | >1.01 |
| fill-hover | `#EEF0FF17` | `#2A2E4A09` | 1.17 | >1.02 |
| fill-active | `#EEF0FF1F` | `#2A2E4A0D` | 1.26 | >1.05 |
| sunk | `#00000059` | `#2A2E4A06` | 1.11 | >1.015 |
| solid | `#15151F` | `#FFFFFF` | — | — |
| overlay | `#000000B8` | `#1A1C2E66` | — | — |
| accent-fill | `#9AA6E81A` | `#59659B14` | 1.11 | >1.02 |
| accent-fill-hover | `#9AA6E82E` | `#59659B24` | — | — |
| accent-line | `#9AA6E847` | `#59659B3D` | 1.37 | >1.10 |
| accent-soft | `#9AA6E812` | `#59659B0F` | — | — |
| warn-soft | `#E3B34112` | `#8A620012` | — | — |
| lamp-off | `#494C60` | `#B0B3C2` | 1.82 | >1.0 |
| top-light | 见 §5 | `#FFFFFFB3` | — | — |
| shadow | 见 §5 | `#2A2E4A33` | — | — |

**浅色下 accent 用的是 logo 真实主蓝 `#59659B`（头发实测锚点，占 logo 9.2%）**，不是暗色 `#9AA6E8` 的变体 —— 浅底上必须压深才够 4.87，而压深后正好落回 logo 原色。这是浅色侧比暗色侧更贴 logo 的地方。

**`#E3B341` / `#F07178` 在浅底上不可用**（1.9 / 2.6），必须换成压深版 `#8A6200` / `#9C3F41`。这与暗色侧"warn/danger 不取自 logo"的结论一致：它们由**可读性**决定，不由 logo 决定。

**三条必须说清的注解：**

- ⚠ **`inner-light` 在浅色下基本不可见，这是有意的。** 暗色里"内胎顶缘高光"能成立，是因为白 α 落在近黑底上；浅色的卡片内胎是 `#FFFFFF`，再叠白高光等于没画。浅色**没有**这个 affordance 的对等物（真实浅色 UI 靠阴影而不是内高光表达隆起，而隆起已由 §5 的 `shadow` 承担）。保留该令牌让 `Bezel` 的代码两套共用，不为此加分支。
- **浅色 CTA 是实心的，没有渐变**：`cta-top == cta-bottom == #59659B`。暗色的白胶囊靠渐变塑形，浅色的强调胶囊靠色块本身，加渐变只会让白字对比度在渐变端掉到门槛以下。
- **非颜色令牌两套共用**，不参与主题：`r-lg/r-md/r-sm`、`font-hero`、三档 `motion-*`、`status-h`、`disabled`(0.35)。其中 `disabled` 用整体 opacity 表达，浅色下同样成立（深字变浅灰），无需按主题分叉。

**强制模式下系统主题变化不产生任何视觉变化** —— 这是正确行为，不是漏接线：`resolved_dark` 由 mode 决定，`system_dark` 变了也不会进入它。监视线程照常更新 `system_dark`（用户随时切回「跟随系统」时立即是正确值），只是不触发重绘。

### 4.4 变更监视为什么要无竞态

`RegNotifyChangeKeyValue` 有两种用法，天真写法有坑：

- **同步（`fAsynchronous = FALSE`）**：调用即阻塞到变更发生。若写成"读值 → 武装通知"，则**读与武装之间那个微秒窗口内发生的变更会被永久跟丢** —— 因为武装之后不再有变更来唤醒它。后果不是"慢一拍"，而是**永久不一致**：winit 的标题栏早已变色（它自己独立响应），而应用主体停在旧主题，直到用户下次再切主题才自愈。
- **异步（`fAsynchronous = TRUE`）+ 事件对象**：**先**武装通知、**再**读值，然后 `WaitForSingleObject` 等事件。任何发生在读之后的变更都会置位事件，因此不可能丢。本设计采用这种。

正确顺序（每轮循环）：

```
RegOpenKeyExW(个人化键, KEY_READ | KEY_NOTIFY)
RegNotifyChangeKeyValue(键, watch_subtree=1, LAST_SET, 事件, fAsynchronous=TRUE)  ← 先武装
读 AppsUseLightTheme                                                              ← 再读值
送 UiMsg::SystemThemeChanged(系统态)
WaitForSingleObject(事件, INFINITE)   ← 任何后续变更都会置位
ResetEvent(事件); RegCloseKey(键); 回到循环开头
```

事件对象每轮复用（`CreateEventW` 一次），键句柄每轮开关 —— 句柄生命周期短且成对，避免长期持有的泄漏。UI 线程退出后 `send` 失败即 `return`，线程自行结束。

---

## 5. 需新增的令牌（吸收现有 5 处散落字面量）

当前令牌纪律很好，令牌块之外只剩 5 个颜色字面量；其中 4 个是**明暗方向相反**的，不抽成令牌就没法两套。这是必要新增，不是预留。

| 新令牌 | 吸收 | 暗色值 | 浅色值 | 为什么必须抽 |
|---|---|---|---|---|
| `well` / `well-hover` | `ui/app.slint:241` 的 `#0000001F/#00000024` | `#0000001F` / `#00000024` | `#FFFFFF2E` / `#FFFFFF38` | 暗色是白胶囊里的**黑**井；浅色 CTA 是实心强调色，井要翻成**白**井 |
| `cta-hover-top/bottom` | `:268` 的 `#FFFFFF`→`#E6E7F2` | `#FFFFFF` → `#E6E7F2` | `#4C5788` → `#4C5788` | **悬停方向相反**：暗色变亮，浅色必须变深（变浅会让白字掉到 4.36，不达标） |
| `top-light` | `:1190` 的 `#EEF0FF0B` | `#EEF0FF0B` | `#FFFFFFB3` | 暗色是"顶部一线微光"，浅色下同一层要么去掉要么换色 |
| `shadow` | `:471` 的 `#00000080` | `#00000080` | `#2A2E4A33` | 黑色的 50% 阴影落在浅底上会让玻璃卡发脏 |

`Orb` 的默认 tint（`:136`）改为取 `Tokens.accent`，不再是字面量 —— 它本来就是"tint 的缺省"，而 accent 就是缺省的含义。

---

## 6. 设置界面

**不新建对话框。** 在既有设置面板（`ui/app.slint:1865`）「关闭行为」组下方追加一组，形状逐字照抄既有那组：

```
Eyebrow { text: "主题" }
HorizontalLayout { 3 × ChipButton { label: "浅色" / "深色" / "跟随系统" } }
Text { wrap: word-wrap; color: Tokens.ink-4 }   // 说明文字
```

- 选中态复用 `ChipButton.accent`（既有约定：`ui/app.slint:1907` 注释明确"同一组件、只换颜色，不做单选圆点"）。
- 面板高度随内容增长；既有 `Bezel { width: 400px }` 内部是 `VerticalLayout`，加一组不需要改布局结构。
- 说明文字需交代：**「跟随系统」下标题栏与窗口会一致；强制模式只改窗口内部，系统标题栏跟随的是系统设置**（诚实交代 §8 的限制，避免用户以为是 bug）。

---

## 7. 持久化

```rust
// src/config.rs
pub enum ThemeMode { Light, Dark, Auto }   // CloseBehavior 同形

pub struct StateFile {
    // …既有三项
    /// `None` = 从没设过 = 跟随系统。与 close_behavior 同形。
    pub theme_mode: Option<ThemeMode>,
}
```

**默认「跟随系统」由 `None` 表达**，不写死 `Auto` —— 与既有 `close_behavior: Option<..>`（`None` = 从没问过）的表达方式一致，语义是"用户没表过态"。

⚠ `StateFile` 的 serde 反序列化必须容忍旧文件缺字段（既有 `update` 的语义，`src/config.rs:262`「损坏或缺失都不应让写入路径失败」）。新增字段需确认现有反序列化对未知/缺失字段的行为，并在测试中钉住"旧 state.json（只有三个字段）能读出 `theme_mode = None`"。

---

## 8. 已知限制（交付时必须写进说明，不留给用户踩）

1. **强制 浅/深 改不动系统标题栏。** winit 按 **OS 值**设 `DWMWA_USE_IMMERSIVE_DARK_MODE`（F3），Slint 无公开 API 覆盖（F4）。所以"系统浅色 + 强制深色"时窗口内部变深而标题栏仍是浅的。**只有「跟随系统」能保证两者一致。** 这是平台限制，非本设计取舍。
2. **高对比度模式**下 winit 判定为浅色（F3 的 `!is_high_contrast()`）；我们同源处理，即高对比度用户拿到的「跟随系统」= 浅色。不额外适配（自定义高对比配色超出本次范围）。
3. **启动首帧的顺序**：必须在 `run()` **之前**按下面顺序落地，否则首帧会闪一下（滚动条用错方案、卡片用错主题）：

   ```
   load state.json → mode → 读系统主题 → resolved_dark
     → ui.global::<Tokens>().set_dark(resolved_dark)
     → 设 Palette.color-scheme 初值（经 changed 处理器同一路径）
     → 启动监视线程
     → ui.run()
   ```

   监视线程在 `run()` 前启动，保证启动瞬间到事件循环就绪之间发生的系统主题变化不会丢。

---

## 9. 验证计划

1. `cargo build` **0 警告**（本仓库惯例）、`cargo test` 全绿。
2. **对比度脚本扩成覆盖两套主题**：浅色门槛与暗色一致（文字 4.5 / 非文字 3.0 / α 档可辨）。脚本解析 `ui/app.slint` 实际值，而非校验意图 —— 沿用上次的做法。
3. **截图矩阵**：3 种模式（浅/深/自动）各出一张，回读渲染像素确认令牌确实生效（上次用这招抓到了 accent 逐位一致）。
4. **「自动」实时性实测**：应用运行中改系统主题，确认窗口主体**与标题栏**同时变化。
5. **持久化实测**：选「深色」→ 退出 → 重启，确认仍是深色且设置面板选中态正确。
6. **旧 state.json 兼容实测**：用三字段的旧文件启动，确认不崩且 `theme_mode = None`。

---

## 10. 探针结论（已实测，非承诺）

实施前已用一次临时编译探针（改动随后 `git checkout` 完全还原，工作树与 `HEAD` 一致）确认三处机制：

| 机制 | 结果 | 证据 |
|---|---|---|
| `export global Tokens` + `dark ? 暗 : 浅` 条件令牌 | ✅ 编译通过 | `cargo build` 退出码 0 |
| Rust 能写 `Tokens.dark` | ✅ 生成 `pub struct Tokens<'a>`、`impl<'a> slint::Global<'a, MainWindow> for Tokens<'a>`、`pub fn set_dark(&self, value: bool)`、`pub fn get_dark(&self) -> bool` | 生成代码 `app.rs:253,254,575` |
| 运行中切 `Palette.color-scheme` | ✅ `changed probe-scheme => { Palette.color-scheme = probe-scheme; }` 编译通过 | `cargo build` 退出码 0 |

第三条是本设计唯一的语法风险点（F6 说明绑定形态不可用），**已证实 `changed` 处理器里的赋值形态可用**，故 §8 不需要"Fluent 配色下次启动生效"这种回落。回落方案（仅在 `init` 设一次）最终**未启用**。

---

## 11. 实施顺序（供后续计划展开）

1. `src/theme.rs`：探测（uxtheme 序号 132 + 高对比度 + 注册表回落）+ 监视线程 + `ThemeMode` 解析。**纯函数部分优先写单测**（注入 `system_dark` 与 mode，断言 `resolved_dark`）。
2. `src/config.rs`：`ThemeMode` + `StateFile.theme_mode` + 兼容性测试。
3. `ui/app.slint`：`export global Tokens` + `dark` 输入 + 45 个令牌加浅色分支 + 4 个新令牌 + 吸收 5 处字面量。
4. `ui/app.slint`：设置面板加「主题」组 + `theme-mode-changed(int)` callback + `theme-mode` 输入。
5. `src/main.rs`：接线（callback → 持久化 → 重解析 → 写 `Tokens.dark`）、`changed` 同步 Fluent scheme、启动时初始 push。
6. 文档：`docs/ARCHITECTURE.md` 增主题子系统一节；`docs/RULINGS.md` 记录 F1/F3/F6 三条平台事实与其设计后果（本仓库惯例：平台事实要留档，避免下一个人重新踩）。
7. 验证：§9 全部执行，结果记入 `docs/VERIFICATION.md`。

---

## 12. 风险

| 风险 | 缓解 |
|---|---|
| 浅色配色的**观感**只能上屏才判得准，表里的值可能需一轮微调 | 对比度门槛已先钉死（不会出现不可读）；观感微调属正常迭代，不改变结构 |
| `uxtheme` 序号 132 是未文档化导出 | 与 winit 同源（F3）；取不到时回落注册表，两条路都有 |
| 令牌从常量变表达式，可能影响渲染性能 | 条件表达式是 Slint 原生形状（F5），Fluent 自身就这么用；无额外机制 |
| 45 行令牌改动面大，易漏 | 验证脚本解析实际文件值，漏改会让两套主题不匹配而被脚本/截图抓到 |
