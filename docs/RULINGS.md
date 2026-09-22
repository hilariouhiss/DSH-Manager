# 实现期裁决存档（RULINGS）

这里保存的是 `feat/implementation` 实现期间由**执行者（实现代理/控制器）而非人类**做出的裁决。
每条都带三样东西：**裁决本身**、**理由**、以及**代价若错**（这条裁决错了会付出什么）。
它们不是新的设计决定，而是对 `docs/IMPLEMENTATION-PLAN.md` 与 `docs/SRS.md` 的字面要求
在执行中暴露出的缺陷、歧义与覆盖缺口所做的处置记录 —— 记下来是为了让后来者能判断
"当时为什么这么办"，而不是把结论当成天经地义。

**来源**：这些裁决原本逐条追加在 git-ignored 的 SDD 草稿账本
`.superpowers/sdd/IMPLEMENTATION-PLAN/progress.md` 里，而该目录在分支合并时会被删除。
本文件是它们的**持久副本**：共 **101 条**记录，覆盖编号 **1–95**（**没有 57 号** —— 账本的编号从 56 直接跳到 58）；其中 7 个编号带后续的确认/修正记录（60×2、61×2、67×2、68×2、69×2、75×2、80×2），已排在各自编号之后。
正文按裁决原文逐字保留（仅规整了缩进与多余的空行），未做摘要。
每条提到的提交号仍可在 git 历史里查到。

---

### Ruling 1 — `WebState::Running` 不含 `owned: bool`，以计划为准。

- 理由：ARCH §4.1 是草案；T14 的设计决策 2 把"自己启动的"与"外部启动的"合并成同一条停止路径（统一走 `taskkill /PID`），`owned` 因此失去用途。计划自检章节已声明"以本计划为准"。
- 代价若错：需在 `WebState` 加回一个字段并在 `project`/停止路径中分支 —— 约 10 行改动。

### Ruling 2 — T7 的 Files 块 "Create: src/dsh.rs" 是笔误，忽略；T7 全部代码落在 `pm.rs`，`src/dsh.rs` 由 T11 创建。

- 理由：T7 的代码块（`run_cmd`/`path_dirs`/`probe_pm`/`read_dsh_version_at`/`read_dsh_version_on_path`/`probe_env`）全部标注插入 `src/pm.rs`；且 T13 把 `dsh::shim_in` 定义为 `pm::shim_in` 的转调，说明 `shim_in` 的归属是 `pm.rs`。若按 Files 块在 T7 创建 dsh.rs，T11 的 "Create: src/dsh.rs" 会冲突。
- 代价若错：文件归属混乱，T11 需改成 "Modify" —— 零功能影响。

### Ruling 3 — T9 与 T10 **合并为一次派发**（一个子代理、一套测试、一个提交）。T9 Step 5 明说"与 Task 10 合并提交"。

- 理由：T9 的简版 `compensate` 是脚手架，单独提交会留下一个已知错误的实现。计划的 Step 5 已作此要求。
- 代价若错：无 —— 合并只是把两次派发并为一次，产出相同。

### Ruling 4 — T8 的 `FakeBackend::run` 中，`fail_install`/`fail_uninstall` 的处理**在 T8 内补齐**，不留给 T9。

- 理由：T8 定义了这两个字段却不在 `run` 中使用，属 T8 自身的不完整（会产生 `dead_code` 警告且使 T8 的测试无法覆盖它们）。T9 Step 4 的注记本是补救，前置到 T8 更干净。
- 代价若错：无功能影响，仅派发顺序微调。

### Ruling 5 — T17 提交时允许存在未使用的 `_job_tx`/`_job_rx`/`_msg_tx`；并删除 `let _ = had_state;` 这一行死代码。

- 理由：T17/T18 是同一文件的分步推进，中间态必然含未接线变量。前缀下划线已表明意图。但 `had_state` 纯属多余，直接删。
- 代价若错：一次 `cargo build` 警告，或将来忘记接线 —— 由 T18 的验证步骤兜住。

### Ruling 6 — `busy` 指示有 ≤ 约 80ms + worker 拾取延迟，**接受，不修**。

- 理由：`execute` 对 `Job::Transact` 的第一件事就是发 `UiMsg::Log("事务开始…")`，drain 因此会在下一 tick（80ms）置 changed=true 并 project，读到已置位的 `busy`。worker 空闲时拾取是即时的。用户感知不到。
- 代价若错：按钮点亮略迟 —— 可用一个显式 `ctx.request_repaint()` 风格的空消息唤醒，约 3 行。

### Ruling 7 — T18 Step 3 孤儿探测线程中的 `job_tx2` 克隆**删除**，不留 `let _ = job_tx2;`。

- 理由：该线程只需向 UI 发消息，不需要派发 Job。保留一个未使用的克隆会误导读者以为它要派发任务。
- 代价若错：无。

### Ruling 8 — T11/T12/T14 的手动验证要求临时修改 `main`；**临时改动一律不得入库**，验证后必须还原。

- 理由：这些临时 `main` 只为打印验证输出，属一次性脚手架。T14 Step 3 还要求临时注释 `windows_subsystem` 属性（否则 stdout 不可见），同样必须还原。
- 代价若错：仓库里留下调试用的 `main`，覆盖真实入口 —— 由每个任务的评审 + T20 的端到端验证兜住。

### Ruling 9 — `[profile.release]` 未经编译验证，接受不补测。

- 理由：Task 1 只要求 debug 构建（brief Step 5）。release + LTO 是数分钟级开销，且首次打包时必然被验证。
- 代价若错：打包时才发现 release 构建问题 —— 那时修，代价是一次构建等待。

### Ruling 10 — 【计划缺陷】二进制 crate 中未使用的 `pub` 项会触发 `dead_code` 警告（已实测：2 个警告）。`model.rs` 的类型要到 Task 17 才被消费，会导致 Task 2~16 构建输出持续不干净。

- 措施：Task 2 在 `src/main.rs` 加**一行带移除契约**的 `#![allow(dead_code)]`（含 4 行说明注释）；Task 20 新增 Step 3 **强制删除该行并确认零警告**，且要求出现的任何 dead_code 都按真实死代码处理、不得恢复抑制。
- 为什么不逐个加 `#[allow]`：受影响项分布在 model.rs / pm.rs / dsh.rs 多个文件，逐个标注更啰嗦且更容易忘记清理。
- 为什么不放任警告：会让"输出必须干净"的检查失效，并掩盖真实警告。
- 代价若错：Task 20 若漏删，抑制会永久留存 —— 由 Step 3 的显式指令 + 最终全分支评审兜住。

### Ruling 11 — 【计划缺陷】Task 3 的 `src/config.rs` 里 `use crate::model::*;` 未被使用（`StateFile`/`Loaded` 都由 config.rs 自己定义）。已从计划中删除该 import。

- 理由：`unused_imports` 是独立 lint，`allow(dead_code)` 不覆盖它。留着必然产生警告。
- 代价若错：零 —— 若后续确实需要 model 类型，补回 import 即可。

### Ruling 12 — 【计划缺陷】Task 13 的 `dsh::shim_in` 没有任何消费者（版本读取实际走 `pm::read_dsh_version_at`）。已从计划中删除。

- 理由：留下的是永久死代码，Task 20 的清理步骤必然会要求删它 —— 不如现在不写。
- 代价若错：若某处确实需要目录级 shim 查找，`pm::shim_in` 仍然存在，直接调用即可。

### Ruling 13 — 【计划缺陷】Task 8 的 txn.rs 里 `RefCell`/`HashMap`/`pm` 三个 import 只被 `#[cfg(test)]` 的 FakeBackend 使用；非 test 构建下它们是未使用 import，而 `unused_imports` **不被 `allow(dead_code)` 覆盖**。已给三者加 `#[cfg(test)]` gate。

- 备选方案（否决）：把 `unused_imports` 也加进临时 allow。否决理由：那样会连**真实写错的 import** 一起掩盖，而 `unused_imports` 是唯一能在实现者本地就拦住这类错误的机制。
- 代价若错：gate 写错位置会导致非 test 构建缺类型 —— 实现者本地立刻可见，零延迟。

### Ruling 14 — 【计划缺陷】Task 11 的 dsh.rs 声明了 `use std::path::{Path, PathBuf};` 但该文件从未使用路径类型（Ruling 12 删掉 `dsh::shim_in` 后更是如此；Task 12/13/14 的代码也都不需要）→ 删除。

- 代价若错：后续若需要路径类型，补回 import 即可。

### Ruling 15 — 【我自己的流程问题】Task 2 的评审 BASE 用 **68e6be3**（实现者实际的父提交），而不是派发前记录的 99ca696。

- 原因：我在实现者运行期间提交了计划修订 68e6be3。若按技能字面用 99ca696，评审 diff 会把【我的】文档改动算成 Task 2 的工作，评审者可能报为 "Extra"。
- 技能原意是"评审者看到的是该任务的改动"，用实现者实际父提交才符合该原意。
- **流程改进**：不要在有实现者运行时改计划。本次实现者处理得当（用显式 pathspec 提交，未碰我的改动），但下次应避免。

### Ruling 16 — Task 2 的测试 `pm_command_table_is_complete` **永久保留**，计划里"临时"的措辞已改为"永久保留"并注明其本职（覆盖 FR-14 命令表完整性）。

- 理由：它断言 4 个 PM 各有 exe/install/uninstall 参数，是有效测试；删除一个通过的测试会减少覆盖，属无意义改动。实现者指出"计划称临时却无移除步骤"是对的，修的是措辞而非代码。
- 代价若错：一个较浅的测试长期存在 —— 无实际代价。

### Ruling 17 — 【计划缺陷，Critical】从 `model.rs` 删除 `NotesStatus`。

- 原因：`app.slint` 的 `export enum NotesStatus` 让 Slint 在 crate 根生成同名 Rust 枚举；`main.rs` 的
`use model::*` 是 glob 导入，被 crate 根本地定义遮蔽 → `model::NotesStatus` 零消费者 →
**Task 20 删除 `allow(dead_code)` 后必然报 dead_code，Step 3（强制零警告）会失败**。
- 发现方式：控制器在等待 Task 2 评审时主动审计后续任务，用 grep 比对两处定义。
- 代价若错：若 Slint 其实不生成同名类型，则 `NotesState.status: Option<NotesStatus>` 会编译失败 ——
立即可见，非静默。

### Ruling 18 — 保留 **crate 级** `#![allow(dead_code)]`，仅修正注释中对其覆盖面的错误陈述。

- 评审者建议改为 `#[allow(dead_code)] mod model;`（更窄）。**不采纳**，理由：受影响的不止 model.rs ——
`pm.rs` 的 `sorted_desc` 要到 T11、`probe_env` 要到 T18；`dsh.rs` 的 `fetch_catalog` 要到 T18；
`txn.rs` 的 `run` 要到 T18。只给 `mod model;` 加属性会让 Task 5~16 重新带上警告，即**该建议在本项目里
达不到它想要的效果**。
- 评审者指出的**实质问题成立**：注释声称受影响的是 model.rs，实际覆盖面更宽。已改注释陈述真实范围。
- 代价若错：真实死代码在 Task 20 之前被掩盖 —— 由 Task 20 Step 3 的强制移除+零警告检查兜住，该检查是
计划明文、不可跳过。

### Ruling 19 — 命令表测试从"仅断言非空"改为**逐 PM 断言确切参数序列**（含 `label()`），更名

`pm_command_table_is_exact`。
- 原因：只查非空时，把安装子命令误写成 `uninstall` 仍会通过 —— 而那正是这张表唯一可能出错的方式。
FR-14 的全部价值就在这些确切字符串上。
- 本属 Minor（技能规定 Minor 不进修复循环），但该缺陷与 Task 20 阻塞项同在 Task 2 的文件里，
合并进同一轮修复比分两轮更省；且改动仅约 40 行测试代码。
- 代价若错：无 —— 更严的断言不会漏报。

### Ruling 20 — 【流程】评审/重评审的 BASE 一律取**被评审提交的实际父提交**，而非技能字面要求的

"派发前记录的 BASE"。
- 原因：控制器会在任务进行中提交计划修订（Task 2 期间就发生了两次：0705106、038fde0）。若按字面用
记录值，评审 diff 会把**文档**改动算成实现者的工作，评审者可能报为 "Extra"，且无谓放大 diff
（17.7KB → 6.8KB）。
- 技能原意是"评审者只看到该任务的改动"，取实际父提交才符合该原意。
- **代价若错**：若某次修复确实依赖中间的计划提交，评审者看不到该提交 —— 但评审者本来就会读
（已修订的）brief，信息不缺。
- **流程改进**：计划修订尽量在派发任务**之前**落地；若在任务进行中发现，按 Ruling 17 的做法
**先记入 ledger、等该任务的循环结束再改**，避免与评审 desync。

### Ruling 21 — 【计划缺陷，Task 17 编译错误】`StyledText::from_markdown` 的签名已核实为

`fn from_markdown(markdown: &str) -> Result<StyledText, StyledTextFromMarkdownError>`，而计划直接把它
当 `StyledText` 传给 `set_notes_text`。
- 措施：`unwrap_or_else(|_| StyledText::from_plain_text(&state.notes.body))`。`from_plain_text` 返回
`StyledText`（已核实，非 Result）。
- 为什么不用 `unwrap_or_default()`：那样解析失败会显示**空白**，而状态标签同时显示 "ok" —— 用户看到
"加载成功但什么都没有" 的矛盾。降级为纯文本至少让内容可见。
- 代价若错：若 `from_markdown` 其实对任意输入都不失败，这行降级分支永不触发 —— 无害。

### Ruling 22 — 【计划缺陷，Task 18 逻辑错误】等端口释放的代码误用了 `wait_port_ready`。

- `wait_port_ready(port, alive, timeout)` 检测的是"端口**变成**被占用"（成功时返回 true），而此处需要的是
"端口**变成**空闲" —— **方向正好相反**。且传 `|| false` 作 alive 回调使循环只探测一次即退出，
结果还被 `let _ = freed;` 丢弃。
- 后果：事务会抢在进程真正退出前开始，撞上 Windows 文件锁 —— 正是 TR-1 要避开的场景。
- 措施：改为显式轮询直到 `!port_in_use(port)`，10 秒超时则报错放弃本次变更。
- 代价若错：多等最多 10 秒 —— 换来的是 TR-1 真正生效。

### Ruling 23 — 【计划缺陷，Task 19 编译错误】`on_hide_to_tray` 的闭包直接 move 了函数参数

`win_weak`，紧接着托盘块的 `win_weak.clone()` 会因 use of moved value 失败。
- 措施：块内先 `let win_weak = win_weak.clone();`。
- 代价若错：零 —— 多一次 Weak 克隆。

### Ruling 24 — 【计划缺陷，Task 19 逻辑错误】`on_install_clicked` 传给 `Job::Transact` 的是

**偏好端口**而非**实际运行端口**。
- 后果：若 dsh web 跑在非偏好端口（上次用 8080、偏好仍 3080），TR-1 会去停一个空端口，真正持锁的
实例还在，事务照样撞 Windows 文件锁 —— **TR-1 形同虚设**。
- 措施：`s.web.port().unwrap_or(s.preferred_port)`。
- 代价若错：若 `web.port()` 返回过期值会停错端口 —— 但 WebState 是 drain 维护的唯一状态源，
不会过期。

### Ruling 25 — 【计划缺陷，Task 15 性能】日志区不应把 `ListView` 包在 `ScrollView` 里。

- 原因：`ListView` 自身就滚动；外套 `ScrollView` 会让它按**内容全高**布局，从而破坏虚拟化 ——
日志上限 2000 行（GC-12），等于一次渲染 2000 个 `Text` 节点。而虚拟化正是当初选 `ListView`
而非整段文本的**全部理由**。
- 说明区的 `ScrollView` 保留：`StyledText` 是静态内容，本身不滚动，需要它提供滚动。
- 代价若错：日志滚动变卡 —— 但这是可感知的，不会静默。

### Ruling 26 — 【文档瑕疵】Task 3 的 Produces 列表把 `save`/`update`/`save_to` 也列为本任务交付物，

但它们实际由 Task 4 交付（Task 3 只留 `todo!()`）。已改为只列读取侧，写入侧标注为 Task 4 交付
并保留最终签名以防接口漂移。
- 理由：评审者据此提出了"brief 声明了但代码里没有"的疑问。虽是文档瑕疵、无代码影响，但会让后续
读者与评审者反复困惑 —— 一次修正消除反复成本。
- 代价若错：零。

### Ruling 27 — 【计划缺陷，测试自相矛盾】Task 8 的 `FakeBackend::run` 不产生任何副作用，导致 Task 9 的测试**互相矛盾**：

- `cross_pm_migration_installs_then_uninstalls` 期望 `Committed{Pnpm}`，这要求 install 之后
`dsh_version_at(pnpm目录)` 能读到目标版本 → 假 backend **必须真的写入** `installed`。
而原实现不写，所以 S2 必然失败 → 该测试**必然红**，且 `Committed` 这条成功路径在测试里永远走不到。
- 但 `tr4` 与 `tr11` 又依赖"install 返回成功却什么也没装上"来让 S2 失败。
- 即：同一套假 backend 被要求同时具备两种互斥行为。
- 措施：写明假 backend 语义（run 必须模拟安装/卸载副作用，从包规格解析版本写入），并补两个开关：
`install_noop`（返回成功但不写入，对应"装是装上了但版本不对"）与 `path_override`
（强行指定 `dsh_version_on_path` 的返回值 —— **这才是 TR-4 真正的判别条件**：走目录检查→失败，
走 PATH→成功，只有独立于 PATH 的实现才会通过）。
tr4/tr11 改用 `install_noop`；删除 Task 9 Step 4 的过时补丁注记（该逻辑归属 Task 8）。
- 已逐条核对全部 8 个事务测试在新语义下的相容性（含 v17/v20/v21 的失败路径与补偿分支）。
- 代价若错：若某个测试的期望值其实依赖旧语义，会在 Task 9/10 的 RED 阶段立刻暴露 —— 非静默。

### Ruling 28 — 【正确性，来自实现者的变异测试】Task 4 的原子写入**没有测试保护**。

- 实现者证明方式：把 `写 .tmp → rename` 整块换成朴素 `std::fs::write(path, text)`，
**10 个测试全部仍然通过** —— 因为 `save_leaves_no_tmp_file_behind` 在"从不创建 .tmp"的实现下
是**空转断言**（它断言 .tmp 不存在，而朴素写入本来就不创建 .tmp）。
- 后果：回退成截断写入会**一路绿灯**，同时静默重新引入 FR-31 存在要关闭的那个数据丢失窗口。
- 措施：补一个**能判别**的测试 —— 用目录占住 .tmp 路径使写入失败，断言**目标文件未被改动**。
截断写入会让目标被覆盖（断言失败），tmp+rename 则保持原样（断言通过）。
- 重要性：这不是"覆盖率可以更广"的 Minor，而是**关于 load-bearing 属性的正确性缺口**。
技能明说 DONE_WITH_CONCERNS 中涉及正确性的 concern 应在评审前解决。
- 代价若错：无 —— 多一个测试。

### Ruling 29 — 【计划缺陷，Task 12 测试必然失败 + 实现不完整】`preprocess_notes` 只识别 ATX 标题。

- 证据：测试输入含 `<h3 id="cn-x">新增功能</h3>` 并断言输出含 `**新增功能**`；但 `heading_body` 只认
`#` 开头的行，对 strip_html 之后的 `新增功能` 返回 `None` → 以纯文本输出 → **断言必红**。
- 更重要的：这不只是测试写错，而是**实现不完整**。DSH 的 release notes **同时**使用两种写法 ——
语言段是 `<h3 id="cn-...">新增功能</h3>`，小节是 `### 体验优化`。只处理 ATX 会让 HTML 那批
（**恰恰是层级最高的段标题**）以纯文本出现，与小节标题的粗体**视觉不一致** —— 那正是 FR-27
要消除的"垃圾文本"问题。
- 措施：新增 `is_html_heading`（必须在 `strip_html` **之前**判断，之后认不出来），
`preprocess_notes` 先判 HTML 标题、再判 ATX。
- 代价若错：若 notes 里出现行内 `<h3>`（非行首），整行会被加粗 —— 极罕见，且视觉上无害。

### Ruling 30 — 【计划缺陷，FR-22 在最常见情况下失效】启动时的孤儿探测被 `if rp != preferred_port`

守卫挡住，方向反了。
- 用户通常不改端口 → 孤儿**最常出现在偏好端口**上；而启动时**没有任何其他路径**探测该端口
（`Job::Probe` 只做 PM 探测）。结果：孤儿在偏好端口时界面一直显示"已停止"，直到用户点"启动"
才由 FR-17 发现 —— 这**不是** FR-22 承诺的"重新启动本程序时会识别出来"。
- 即：加了守卫等于"只在少见情况下能发现孤儿"，恰与 FR-22 意图相反。
- 措施：去掉守卫，无条件探测持久化的运行态端口；顺带落实 Ruling 7（删除未使用的 job_tx2 克隆）。
- 代价若错：多一次 300ms 的 TCP 探测 —— 可忽略。

### Ruling 31 — 【Task 6 评审发现，真问题】`path_dirs()` 必须过滤空 PATH 段。

- 机理：`split_paths` 对 `;;` 或首尾 `;` 产出一个空 `PathBuf`；`空目录.join("dsh.cmd")` 得到
**相对路径** `"dsh.cmd"`，相对 CWD 解析，一旦 CWD 下有同名文件就被 `is_file()` 判为真，
并因 `find_dsh_on_path` 的提前 return **遮蔽后面真实的命中**；`owner_of` 随后拿到
`parent() == Some("")` 返回 `None` → owner 判定直接失败。
- 措施：抽出纯函数 `parse_path_var`（丢空段），`path_dirs` 转调；补 3 条用例的测试。
抽独立函数是为了可测 —— `path_dirs` 读环境变量，并行测试里改 PATH 不可靠。
- 代价若错：零（丢空段本就是正确行为）。

### Ruling 32 — 【不采纳评审者的建议】不按 PATH 顺序重排 `probe_env` 的 `bins`。

- 评审者与实现者都建议"Task 7 应按 PATH 顺序构建 bins，否则 Task 6 的顺序保证会被抵消"。
- **不采纳**：`owner_of` 按 bin_dir 匹配，而各 PM 的全局 bin 目录**互不相同**
（npm=`%APPDATA%\npm`、pnpm=`%LOCALAPPDATA%\pnpm\bin`、bun=`%USERPROFILE%\.bun\bin`、
yarn 亦独立），因此第一个匹配项唯一，顺序不影响结果。真正决定顺序语义的是
`find_dsh_on_path`，那部分已正确并有测试。
- 复排是"为不可能发生的两个 PM 共享 bin 目录"而写的防御代码，属 YAGNI。
- 代价若错：若某 PM 未来把自己的 bin 目录指向另一个 PM 的目录，owner 会取 `Pm::ALL` 顺序上的
第一个 —— 届时应改为按 PATH 排序或按 bin_dir 去重，并在该 PM 的适配任务中处理。

### Ruling 33 — 【Important，plan-mandated】`read_dsh_version_on_path`（S4 的读取器）补退出码守卫。

- 问题：它 `run_cmd(..).ok()?` 后直接解析 stdout，**不检查 code**；而兄弟函数 `read_dsh_version_at`
有 `if out.code != 0 { return None }`。于是一个"stdout 可解析但退出码非零"的 shim 会被 S4 接受、
被 dir 读取器拒绝 —— **两个读取器对同一次安装给出相反结论，S4 假通过**。这正是 TR-4 要防的方向。
- 措施：补上一致的 `code != 0` 守卫（计划已改，brief 7 已重生成）。
- 代价若错：一个 stdout 正常但退出码非零的 shim 会被判为未安装 —— 而 dir 读取器本来就这么判，
一致性优先。

### Ruling 34 — 【覆盖缺口，load-bearing 约束无测试】TR-4 目前唯一的测试对它没有牙齿。

- `read_dsh_version_at_missing_dir_is_none` 在 `shim_in(dir)?` 就提前返回，命令根本没跑 ——
一个"经 PATH 解析"的错误实现照样能通过它（只要 PATH 上没有 dsh）。它断言的内容没错，
但**测不到 TR-4**。
- 措施：新增判别性测试 —— 在临时目录放一个真实可执行的 `dsh.cmd`（该目录**不在 PATH 上**），
正确实现（执行该目录的 shim）拿到版本，错误实现（走 PATH）返回 None。
顺带覆盖 `.cmd` shim 的真实 spawn 路径（既有 run_cmd 测试用的都是 cmd.exe）。
- 为什么算必做：TR-4 是 SRS 的具名约束、验证清单 V-19 的条目，而它当前**零回归保护**。
与 Task 4 的 Concern 1（原子写入无测试）同类。
- 代价若错：无 —— 多一个测试。

### Ruling 35 — 【结构性根治】抽出 `version_from_shim(shim: &Path)`，两个读取器共用。

- 两个 Finding 的根因相同：要判别它们都需要改进程 `PATH`，而 brief 明确把 PATH 排除在测试之外。
- 措施（一并根治）：
1. `version_from_shim(shim: &Path) -> Option<Version>` 内含退出码守卫；`read_dsh_version_at`
收敛为 `version_from_shim(&shim_in(dir)?)`；`read_dsh_version_on_path` 改调同一个函数。
→ **守卫只有一份，不对称在结构上不可能再出现**（根治 Finding 1，而非再补一次）。
→ 它接收 shim 路径参数，因此**可直接单测，无需改 PATH**。
2. TR-4 测试的 shim 版本号改为 **`9.9.9-tr4probe`**（真实 dsh 不可能有的值）→ 任何 PATH 解析
都会返回**不同的**版本从而失败，**判别性不再依赖机器环境**。
3. 新增 `version_from_shim_rejects_nonzero_exit`：shim 先打印合法版本号、再 `@exit /b 3`
—— 正是 Finding 2 说没有保护的形状，现在无需改 PATH 即可覆盖。
- 计数 19 → 20。
- 代价若错：若 `9.9.9-tr4probe` 恰好等于某个真实版本，判别性又会失效 —— 概率可忽略，
且 Ruling 34 的变异要求会再次暴露。
- **教训**：我先前把"测试存在"当成"有保护"。判别性必须用变异**实测**，而不是推理。

### Ruling 36 — 【编译级，我的 Ruling 13 分析错误】`use crate::pm;` **不能** gate 到 `#[cfg(test)]`。

- 原因：`precheck` 是**生产代码**且调用 `pm::same_dir`（TR-3 的 PATH 检查），gate 掉后 `cargo build`
直接 E0433。RefCell/HashMap 确实只被 FakeBackend 使用，门必须保留。
- 我的错误来源：Ruling 13 时我按"读起来像"判断三个 import 的用途，没有逐个确认真实使用点。
- 代价若错：编译失败，立即可见 —— 但会浪费实现者一轮。

### Ruling 37 — 【编译级】`precheck_rejects_pm_bin_not_on_path` 需要 `let mut b`（bins 是普通 HashMap）。


### Ruling 38 — 【覆盖缺口，最有价值】新增 `fake_backend_semantics_are_modeled` 直接钉住假 backend 语义。

- 依据（实现者的变异 C）：删掉 `FakeBackend::run` 的**安装副作用**（**正是本计划最初的那个 bug**），
Task 8 其余 6 个测试**全部照样通过**；只有 Task 9 的一个测试会变红。
- 风险：假 backend 若被"简化"掉副作用，依赖它的**八个以上**事务测试会**集体变成空转**而无一报错 ——
与本项目已踩过的两次同类坑（Task 4 空转的原子性测试、Task 7 不能判别的 TR-4 测试）形状完全一致。
- 措施：在**定义假 backend 的同一个任务里**直接断言六个语义，不依赖下游间接覆盖。计数 6 → 7。
- 代价若错：无 —— 多一个测试。

### Ruling 39 — 【Important，plan-mandated】保留 `is_safe_version` 与 `RejectReason::InvalidVersion`，

把"该分支不可达"作为**已接受的计划偏差**记录在案。
- 已核实的事实：`Target.version` 是 `semver::Version`；其 `FromStr` 与受校验的
`Prerelease`/`BuildMetadata` 构造器让**不安全版本号不可表示**；全项目的 `Version` 构造点
（pm.rs:190 的 `parse()`）都走受校验的 API。因此 `precheck` 里那条字符集检查**从任何 Target
都无法触发**，`precheck_rejects_unsafe_version` 是空转的（两条断言必然相同）。
- **裁决：保留，不删。** 理由：
1. NFR-6 在 SRS 里是具名需求。审计者问"这条实现在哪"时，应当能找到 `is_safe_version` 这个
产物 —— 删掉它等于移除了该需求唯一的具名实现。
2. NFR-6 实际是**由类型系统结构性满足**的（非法状态不可表示），这比运行时检查更强。
而那行运行时检查是**在那个版本号真正变成命令行参数的位置**上的低成本双重保险。
3. "把校验挪到摄入边界"这一选项不成立：摄入边界**就是** `parse::<Version>()`，它已经在校验；
挪过去等于手写 semver 校验，严格更差。
- **记录在案的偏差**：该分支与那条空转测试都无回归保护 —— 删除它们不会有任何测试变红。
已列入最终评审的**批量清理候选**（与 Ruling 40、陈旧注释、"下面三个"措辞、
`precheck_passes_for_valid_target` 与另一条断言重复、`let joined` 死代码等一起处理）。
- 代价若错：若将来真出现了绕过 `semver::Version` 的版本号路径，这条检查是唯一的兜底 ——
而正因如此才保留它。

### Ruling 40 — 【Minor→需明确解读】TR-2 目前只拒绝 `Err`（命令没跑起来），而真实可用性

（`probe_pm`，pm.rs:146-150）还要求 `--version` **退出码为 0**。二者不一致。
- SRS TR-2 原文是"`--version` 执行成功" —— **措辞含糊**：可读作"命令能被执行"，也可读作"退出码为 0"。
- **裁决：本轮保留现状，列为真实但暂缓项。** 理由：当前行为下，一个"存在但 `--version` 非零"的
PM 会通过前置检查、在 S1 失败、随后补偿返回 `RolledBack`，**origin 完好、无数据丢失、无不一致状态** ——
只是报错更晚、更不具体。修它需要同时改 `precheck` 与假 backend（`unavailable` 目前只产出 `Err`），
即一轮修复 + 重评审，换来的是一条更清晰的错误信息。
- **成本若错**：坏掉的 PM 给出较模糊的错误；仍然安全。
- **已列入最终评审**：需要决定"修 `precheck`"还是"澄清 SRS 措辞" —— 两者选一，不能悬着。

### Ruling 41 — 【计数陈旧 + RED 预期有误，随派发携带】Task 9/10 brief 里的测试计数与 RED 预期均不准确。

- 计数：brief 说 `cargo test txn` 应是 11。实际应为 **Task 9 后 12**（7 + 5）、**Task 10 后 17**；
全量 **39 → 44 → 49**。差额来自 Task 8 修复轮按 Ruling 38 新增的
`fake_backend_semantics_are_modeled`（brief 生成于该修复轮之前）。
- RED：Task 10 Step 2 声称 `v17` 与 `tr11` 会失败。实际加入
`degraded_outcome_carries_runnable_manual_commands` 后**整个测试二进制编译失败**
（`manual_commands` 未定义），这才是真实 RED；且**`tr11` 对 Task 9 的简版 `compensate` 是通过的**
—— 简版根本不做卸载，该测试的两条断言都成立。
- 措施：随派发明确这两点，并**明令不得为了让测试变红而弱化/改动 `tr11`**；`tr11` 的判别性改由
M4 变异（去掉 C2a 的"先探测再卸载"守卫）实测证明。
- 代价若错：若实现者按 brief 数字去凑，会删/并测试 —— 已明文禁止，且评审会看到测试清单。

### Ruling 42 — 【变异要求，本任务强制】要求实现者做 4 组变异，一一对应本任务的 4 条具名 SRS 约束：

M1→TR-4（S2 改走 PATH）、M2→TR-6（卸载提到安装之前）、M3→TR-5（去掉 C1 探测）、
M4→TR-11（去掉 C2a 的探测守卫）。每组必须**恰好**杀死指定的那条测试。
- 理由：本 ledger 已记录两次"测试看着有保护、实则不能失败"（Ruling 28 的空转原子性测试、
Ruling 34 不能判别的 TR-4 测试），Ruling 35 的教训是"判别性必须用变异实测，而不是推理"。
而 SRS §4 是全项目错误代价最高的部分。
- 代价若错：实现者多花 4 轮测试运行时间。

### Ruling 43 — 【文档缺陷，Task 12】`is_html_heading` 的文档注释与 `preprocess_notes` 的**粘在了一起**。

- 现象（计划 2809-2823 行）：`preprocess_notes` 的整段说明（"FR-27 的强制预处理 … 尤其不要破坏链接。"）
后面直接跟了 `is_html_heading` 的说明，两者共用同一个 `///` 块挂在 `fn is_html_heading` 上；
而 2829-2838 行又**重复**了一遍 `preprocess_notes` 的说明。这是 Ruling 29 插入新函数时留下的接缝。
- 后果：`is_html_heading` 的文档开头在讲另一个函数；同一段说明在文件里出现两次。纯文档，无行为影响，
但会被评审者反复报为缺陷。
- 措施：把 2809-2823 行的块切成两块 —— 只保留 `is_html_heading` 自己的四行说明。
- 代价若错：零。

### Ruling 44 — 【覆盖缺口，Task 13】`parse_netstat_ignores_time_wait_rows` **不能失败**。

- 现象：`NETSTAT` fixture 里 3080 的 **LISTENING 行排在 TIME_WAIT 行之前**，而 `parse_netstat_pid`
取第一个命中就返回。于是一个**完全不做 LISTENING 过滤**的错误实现照样返回 `Some(13432)`，
`assert_ne!(…, Some(0))` 依然通过 —— 该测试没有牙齿。
- 判别条件应当是：TIME_WAIT 行**先出现**时，正确实现跳过它并返回后面 LISTENING 行的 PID；
错误实现返回 `Some(0)`。
- 措施：把 fixture 里 3080 的 **TIME_WAIT 行移到 LISTENING 行之前**（一行位置的调整）。
这样两条测试同时获得判别性：`…_finds_listening_pid` 必须跨过 TIME_WAIT 才能拿到 13432，
`…_ignores_time_wait_rows` 在无过滤实现下必然拿到 `Some(0)`。
- 这是本项目第三次遇到同类问题（Ruling 28 空转的原子写入测试、Ruling 34 不能判别的 TR-4 测试），
依据 Ruling 35 的教训，判别性必须**用变异实测**，所以本次直接要求实现者做变异验证（见派发）。
- 代价若错：无 —— 只调整 fixture 行序。

### Ruling 45 — 【计数陈旧，Task 12/13】brief 的期望计数比实际多 1。

- Task 12 的测试块实际只有 **7** 个 `#[test]`（brief 说 8），Task 13 实际加 5 个。
因此：Task 11 后 6、Task 12 后 **13**（非 14）、Task 13 后 **18**（非 19）。
- 同 Ruling 41 的性质，随各自派发携带即可，不改计划正文（改了也会再次漂移）。

### Ruling 46 — 【功能缺口，Task 14，FR-16 对 bun 用户失效】`spawn_web` 把 shim 名写死成 `dsh.cmd`。

- 依据：本项目自己认定"npm / pnpm / yarn 生成 `.cmd`，**bun 生成 `.exe`**"（`SHIM_NAMES` 的两个成员、
Task 6 的 `find_dsh_recognizes_exe_shim_for_bun` 测试）。而 `src/pm.rs:84` 的
`pub const CREATE_NO_WINDOW` 与 `Command::new("dsh.cmd")` 组合下，owner 是 bun 的机器上
**没有 dsh.cmd** → `spawn_web` 永远返回 Err → 该用户的 FR-16（启动 dsh web）直接不可用。
- 措施：`spawn_web` 改为**按 `SHIM_NAMES` 顺序尝试**（`.exe` 在前，与 PATHEXT 及
`pm::find_dsh_on_path` 的探测顺序一致），取第一个 spawn 成功者；全失败则报出尝试过的名字。
失败的 `Command::spawn` 无副作用（进程根本没起来），所以尝试是安全的。签名不变，
Task 18 的 `dsh::spawn_web(port, tx.clone())` 调用点不受影响。
- 为什么不按 owner PM 选：`dsh.rs` 不持有 `PmEnv`，而按顺序尝试是**零新增接口**的等价做法；
顺序与探测逻辑一致，不会出现"探测到 .exe 却去执行 .cmd"的分裂。
- 代价若错：若某机器 PATH 上同时存在无关的 `dsh.exe` 与正确的 `dsh.cmd`，会优先执行前者 ——
与用户敲 `dsh` 时的 PATHEXT 解析顺序一致，属可接受。

### Ruling 47 — 【随派发携带】Task 11/12 的手动验证要打印 stdout，而

`#![cfg_attr(not(test), windows_subsystem = "windows")]` 下 stdout 可能不可见
（Task 14 的 brief 第 188 行已就此给出提示，Task 11/12 的 brief 没有）。
- 措施：派发 Task 11/12 时明确"若 `cargo run` 看不到输出，临时注释掉该属性，验证后**必须还原**"
（Ruling 8 已要求临时改动一律不得入库）。
- 代价若错：手动验证拿不到输出，实现者会来问。

### Ruling 48 — 认可 33e2d26 中对 brief 的两处**编译级**偏差（D1 `let mut b`、D2 提前 `let detail`）。

- 理由：brief 的两处代码**根本编译不过**（E0596 / E0382），照字面执行不可能产出可提交的树。
两处修法都是最小改动，不改变任何参数、断言字符串或执行顺序。
- 代价若错：若实现者借"修复"之名改了语义 —— 已要求评审者**核验偏差的最小性与行为等价性**，
且 diff 为 +327/−0（无删除行）说明既有代码零改动。

### Ruling 49 — 【环境事实，记给最终评审】本仓库 `core.autocrlf=true` 且**无 `.gitattributes`**。

- 现象：实现者在验证过程中执行过一次 `git checkout --`，工作区被改写成 CRLF（`src/main.rs` 本来就是 CRLF）；
它随后转回 LF 并重建索引，最终提交为 LF 且工作区干净（我已用 `file` 复核）。
- 风险：任何后续的 `git checkout --` / `git stash` 都可能翻转行尾，产生**整文件级**的假 diff，
让评审者的 diff 看起来像"全文件重写"。各任务均可能踩到。
- 措施：不新增 `.gitattributes`（不在计划范围内；且它会改动所有文件的规范化属性，属独立任务）。
改为**随每个任务的派发携带提示**：若发现整文件 diff，先查行尾，不要当成实现者的改动。
- 代价若错：某个任务的评审包被行尾噪声污染，多花一轮辨认。

### Ruling 50 — 【Important，裁决：**修**】`compensate` 的两处 `.is_err()` 吞掉了补偿失败的诊断信息。

- 事实（txn.rs:303 / 321 / 339）：`install(..).is_err()` 与 `uninstall(..).is_err()` 丢弃了
`Err(String)` —— 而那正是 txn.rs:204/214 构造的"退出码 + stderr"载荷；`degraded(..)` 传的
`reason` 是**主流程**的 `detail`，日志行也只有一句无 `e` 的"补偿：重装 origin 失败"。
结果：用户被明确告知进入降级，却**看不到补偿为什么失败**。
- 与计划的关系：这是 brief Task 10 Step 3 的**逐字代码**（plan-mandated），故由我裁决。
- 裁决依据（spec 为绑定权威）：SRS §4.1 的契约是"要么回到操作前状态，要么**明确报告**降级状态"，
§4.2.3 C3 亦为"补偿失败 → 报告降级状态"。一个说不出原因、也丢掉了唯一诊断载荷的降级报告，
算不上"明确报告" —— 而诊断信息本来就在手上，只是被丢掉了。
- 措施：两处都捕获 `e`，并入**日志**与 `Degraded.reason`（`format!("{detail}；补偿失败：{e}")`），
原 `detail` **保留**（严格增加信息量，不做替换）。
- 代价若错：降级文本变长一行；无行为风险 —— 且不改变任何控制流（仍是同一条 `return degraded(..)`）。

### Ruling 51 — 【Important，裁决：**修 + 补判别性测试**】`manual_commands` 在**同 PM** 降级时给出自毁配方。

- 事实（txn.rs:269-278）：它无条件返回 `[重装 origin 版本, 卸载 target 包]`。当
`origin.pm == target.pm` 时这两条命令作用于**同一个包**：照着执行 = 先装回 V0，再把 dsh 整个删掉。
而这恰恰是 TR-5/TR-6 存在的全部理由（"一份都没有"），却被当成**恢复配方**递给用户。
- 可达性（我复核过）：同 PM 事务下 S1 若把 shim 装坏（返回成功但 `dsh_version_at` 读不到）→ S2 失败 →
补偿时 C1 用**同一个目录**探测 → false → C2b 重装同版本若再失败 → 走 `degraded(..)` → 命中该分支。
即：**origin 与 target 是同一个 PM 目录时**，C1 与 C2a 的"不同 PM"假设同时失效。
- 措施：仅当 `target.pm != origin.pm` 时才追加第二条命令；并补测试
`manual_commands_same_pm_never_removes_the_only_copy`（同 PM → 恰好 1 条，且不含 remove）。
跨 PM 的既有测试（`degraded_outcome_carries_runnable_manual_commands` 断言 len == 2）不受影响。
- 代价若错：同 PM 降级时用户只拿到一条"装回 V0"的命令 —— 那正是该场景该做的事。

### Ruling 52 — 本轮**一并处理**的 4 项 Minor（与 Ruling 19 同一做法：省一轮修复）：

1. **S2 重复读**（txn.rs:238-243）：`dsh_version_at` 读两次，真实 backend 下会**执行两次 shim**，
且 `detail` 里的"实际"可能与触发失败的那次读数不一致 → 改为读一次存入 `let got`。
2. **C3 零回归保护**（覆盖缺口，评审者已用变异证明）：把 C3 的探测换成 `let restored = true;`
**全 49 条测试仍然全绿**。C3 是 SRS §4.2.3 具名步骤，其失效会让"其实没恢复"被报成
`RolledBack`（"已恢复"）—— 与本项目已修过三次的同类缺口（Ruling 28/34/38）形状完全一致。
→ 补测试 `c3_detects_incomplete_recovery_and_degrades`（origin 坏 + 重装 noop + S3 失败 → Degraded），
并用 `let restored = true;` 变异证明其有牙齿。
3. **错误的需求引用**（txn.rs:268）：注释写"FR-32 要求降级报告给出手动命令"，而 FR-32 是
**持久化失败**的处理；真实出处是 SRS §4.1 与 §4.2.3 C3 → 改正（计划同步改）。
4. **空转的断言合取**（txn.rs:581）：`b.called("Pnpm")` 被 precheck 的 `"Pnpm --version"` 记录
无条件满足，真正有牙齿的只有 `b.called("remove")` → 改为像 v21 那样断言**同一条记录**里同时含
`Pnpm` 与 `remove`。

### Ruling 53 — 【Task 20 阻塞项，已由实测界定】`TxStep` 的 5 个变体**永不构造**，Task 20 必须删掉它们。

- 实测（我亲自做的，非推理）：
1. `grep -rn "TxStep::" src/` → 全仓只构造 `S1Install` / `S2Verify` / `S3Uninstall` / `S4VerifyFinal`
四个；`Precheck` / `C1Probe` / `C2Restore` / `C2Cleanup` / `C3Confirm` **零构造点**。
2. 临时注释掉 `#![allow(dead_code)]` 后 `cargo build`：**55 条警告，全部属 dead_code 族**，
**没有任何一条 `unused_imports` 或其他 lint** —— 这反证 Ruling 11/13/14 的 import 处理是对的。
其中 model.rs 报的是 `enum TxStep is never used`（因整条链未接线，尚未下钻到变体级）。
3. 恢复后 `git status` 干净（用 cp 备份还原，未走 `git checkout --`，避开 Ruling 49 的 CRLF 陷阱）。
- 为什么变体级警告**接下来一定会出现**：Task 17 的 `describe_outcome_log` 调用 `failed.label()`，
Task 18 构造 `S1Install`，届时 `TxStep` 成为"被使用的枚举"，rustc 随即对**未被构造的变体**报警
（match 模式不算构造）。这正是 Task 20"删抑制 + 双零警告"那一步会撞上的东西。
- **裁决：Task 20 删除这 5 个变体及其 `label()` 分支**，理由不是"省事"，而是它们**按设计不可达且语义错误**：
1. `Degraded.failed` / `RolledBack.failed` 在 UI 里被渲染成「在「X」阶段失败」，
语义是**主流程**在哪一步失败（`reason` 才是补偿失败的说明，F1 已补上）。
把补偿步骤塞进 `failed` 会显示成"降级：在「确认已恢复」阶段失败"——**语义错位**，不是改进。
2. 补偿进度**结构上无法上报**：`TxProgress` 只能由 main.rs 发送，而 main.rs 看不到 `compensate` 内部；
且同一 tick 内连发两条会被 `drain → project` 只渲染最后一条，故"先发 Precheck 再发 S1Install"
在界面上等于没发 —— 为骗过 lint 而写这种代码是本末倒置。
3. 它们的 `label()` 文案（"前置检查"/"探测原状态"…）因此也无任何可显示的场合。
- 与 Ruling 39 的区别（避免误伤）：`is_safe_version` 与 `RejectReason::InvalidVersion` **有构造点**
（在 `precheck` 里，只是运行期不可达），故**保留**；本条要删的变体是**静态也构造不出来**。
- 代价若错：若将来给 `run`/`compensate` 加上进度通道，需要把这 5 个变体加回来 —— 届时按新语义重新命名即可。

### Ruling 54 — 【SRS 措辞偏差，接受并记录】F2 的收窄与 SRS §4.2.3 的字面"输出**两条**可复制的手动命令"不一致

（同 PM 降级时只给 1 条）。
- 裁决：**接受**。SRS 那句写在 C3 的通用分支上，且其表述意图是"给用户**可直接执行**的恢复配方"；
同 PM 场景下第二条命令（卸载 target 包）与第一条作用于同一个包，执行后会把 dsh 整个删掉 ——
即字面遵守 SRS 反而制造 §4.1 明令禁止的"两边都没有"。行为安全的一侧优先。
- 记入**最终评审的批量项**：需要二选一 —— 改 SRS 措辞（把"两条"改为"恢复所需的手动命令"），
或在 §4.2.3 注明"同 PM 时只有一条"。**不能悬着**（与 Ruling 40 同类）。

### Ruling 55 — 【具名风险确认 → 补一轮】F1 的修复**自身没有回归保护**，且 C2a 失败分支**零测试覆盖**。

- 重评审者核实（我复核其推理）：`v21` 走的是 C2b 失败路径，其断言为 `Degraded { .. }`
（`..` 忽略 `reason`）+ 调用记录，而调用记录在"提前 `return degraded(.., detail)`"的旧写法下**同样成立**；
C2a 分支（清理 target 失败）**没有任何测试到达**（文件里所有 `fail_uninstall` 都推的是 origin 的 Npm）。
把两处退回 `.is_err()` 仍可编译（`e` 无未使用警告）且 **51 条全绿**。
- 为什么这算 load-bearing（与 Ruling 52.2 同类，故进循环）：
1. C2a 的守卫若被删掉，清理失败会**穿透到 C3**；此时 origin 完好 → C3 为真 → 返回 **`RolledBack`**，
即界面显示"操作失败，**已恢复**"，而 target 上的残留其实还在 —— **假成功报告**，
与 C3 缺口同一性质。
2. F1 是我按 SRS §4.1「明确报告降级状态」强制要求修的；一个 revert 就能静默退回旧行为，
等于该裁决没有落地。
- 措施：**fix round 2**，只加 1 条端到端测试（用既有开关，零生产代码改动）：
origin Npm 完好 + target Pnpm 已装 + `fail_uninstall` 同时推 **Npm 与 Pnpm**
→ S3 卸载 Npm 失败 → C1 真 → C2a 清理 Pnpm 失败 → **Degraded**；
断言 `reason` 含 `补偿失败` **且** `failed == TxStep::S3Uninstall`。
变异验证：把两处 `if let Err(e)` 退回 `.is_err()`（丢弃 `e` 与 `补偿失败` 后缀）→ 该测试必须变红。
这一条同时闭合：F1 回归保护、C2a 分支覆盖、以及第三条 Degraded 出口的覆盖。
- 代价若错：一轮测试 + 一次 scoped 重评审（约 10 分钟）。

### Ruling 56 — 【重评审残留项，裁决：**parked**】F1 在 **C2b** 那一半仍无 reason 断言。

- 事实（重评审者实测）：只把 txn.rs:312-315 退回 `.is_err()` 而保留 C2a 的修复，**52 条仍全绿**
—— 因为唯一到达 C2b 失败路径的 `v21` 只断言 `Degraded { .. }`（`..` 忽略 reason）与调用记录。
- **裁决：parked（真实但暂缓），不进循环。** 与 Ruling 55（C2a）的区别是实质性的，不是双标：
- C2a 那条**守卫本身**零覆盖 —— 删掉守卫会让清理失败穿透到 C3 并被报成 `RolledBack`（**假成功**）；
Ruling 55 修的是这个。
- C2b 的**守卫有覆盖**（`v21` 断言 Degraded + "绝不盲目卸载 target"的调用记录），
未受保护的**只是消息末尾的诊断后缀**。其失效后果 = 少一句"补偿失败：…"，
即退回到 F1 之前的信息量 —— **诊断退化，非安全退化**。
- 与 Ruling 40 的既有先例一致（"报错更不具体"类问题列为真实但暂缓）。
- 记入**最终评审**：可一行断言闭合（在 `v21` 里解构 `reason` 断言含 `补偿失败`）。

### Ruling 58 — 【计数口径缺陷，我的错；须改计划】任务 11/12/13 的"Expected: N passed"口径混淆。

- 事实（实现者实测）：`cargo test dsh` 是**子串过滤**，除 `dsh::tests` 的 6 条外还命中
`pm::tests` 里 5 条名字含 `dsh` 的（`find_dsh_*` / `read_dsh_version*`）→ 实打 **11 passed**。
我在派发时写的"该过滤器只匹配本模块"是**错的前提**。计划里 Task 11 写 6、Task 12 写 13、
Task 13 写 18，都是"模块内条数"，但用的命令是 `cargo test dsh` —— 三者都对不上。
- 措施：把 Task 11/12/13 Step 4 的命令统一改为 **`cargo test dsh::tests`**（与 Task 6 的
`cargo test pm::tests` 同一做法），Expected 分别为 **6 / 13 / 18**，并加一句说明"不要用
`cargo test dsh` 数个数 —— 子串过滤会连 pm 模块里 5 条含 dsh 的测试一起算"。
- 与 Ruling 45 的关系：Ruling 45 改的是**数字**（14→13、19→18），本条改的是**命令**；两者合起来才自洽。
- 代价若错：实现者会为一个永远对不上的计数来回折腾（本次已被实现者用实测挡下）。

### Ruling 59 — 【携带目标纠正】GC-14 的**选择器**风险不在 Task 12/13，而在 **Task 17**。

- 评审者建议"T12/T13 必须自带通道感知的断言"以防有人用 `tags["latest"]` 选更新目标。**经查证，
该风险的正确落点是 Task 17**：全项目唯一的"求最新"选择器是 `main.rs` 的 `newest_in_channel()`
（计划 4000-4003 行），而它**已经**正确地写成 `pm::latest_in(&catalog.versions, ch)`；
Task 12（说明拉取）与 Task 13（端口探测）里根本没有选择器。
- 措施：随 **Task 17** 的派发携带"`Catalog.versions` 已按降序排好，`versions.first()` 读起来像'最新'
但它通道盲 —— 不得用它选安装目标，也不得读 `tags["latest"]`"的警示；并附 Task 11 的 Minor #1
（该 GC-14 测试只覆盖 `parse_catalog` 层，看不到选择器层的误用，Task 17 需自带断言）。
- 代价若错：把警示发给没有选择器的任务，等于没发 —— 这正是本条要纠正的。

### Ruling 60 — 【计划缺陷，编译级；实现者发现】Task 12 的测试字面量 `r#"…"body":"## 标题…"#` **无法编译**。

- 机理：字面量内容里出现了 **`"` 紧跟 `#`** 这一序列（`"## 标题` 的开引号 + markdown 井号），
而 `r#"…"#` 正是以 `"#` 结束 → 提前截断，报 `too many '#' when terminating raw string`。
- 实现者的诊断补充：惯用修法 `r##"…"##` 在本 crate 的 **edition 2024** 下也被拒（`##` 被保留），
故改用 `concat!(r#"…"body":""#, "## 标题\\n内容", r#""}"#)`，并**证明**拼出的 JSON 与 brief 意图逐字节相同、
brief 原本的断言照样成立（`"## 标题\n内容"` 中 `\n` 是真换行，而 JSON 里是字面的反斜杠 n）。
- 我的独立核查：把计划里**全部 6 处** `r#"` 站点逐个看过（751 / 2620 / 2675 / 2821 / 2874 行 + 2871 病灶），
**只有这一处**含 `"#` 序列，其余 5 处内容里根本没有 `#` 字符 → 该缺陷是孤例，不必大范围改。
- 措施：把计划中该行改成实现者验证过的 `concat!` 形式（等本任务循环结束后与其它计划修订一并提交）。
- 代价若错：若 `concat!` 形式与 brief 意图有细微差别，断言会红 —— 实现者已实测通过，且评审者会复核。

### Ruling 60（修正） — 前述"Rust 2024 禁用了 `r##`"的机理**是错的**，已由评审者用 `rustc` 实测推翻。

- 实测（评审者跑的探针）：`rustc --edition 2024` **接受** `r##"hello "world""##`（exit 0）；
Rust 2024 保留的是**无前缀**的 guarded string 字面量 `##"hello"##`（"unprefixed guarded string
literals are reserved for future use since Rust 2024"），与 `r##` 无关。
- 真正的规则是**内容相关**：本例里 JSON 内容本身含有 `"##` —— 那正是 `r##"…"##` 的终止序列，
所以升井号没用；且在 `--edition 2021` 下同样报错（4 个 error）→ **与 edition 完全无关**。
- 措施：计划注释已按正确机理改写（"内容里恰好也有它的终止序列 `"##`，实测 2021/2024 下同样失败"）；
`concat!` 的修法本身仍然正确且最小 ✓。
- **教训**：实现者把"我试了 `r##` 也失败"**正确地**报告了，但把原因**归给了 edition** —— 而评审者
没有采信，而是跑了探针。这正是本项目要的评审姿态（同 Ruling 34/35：判别性必须实测）。
我已把 ledger 里那条错误机理改正，避免它被当成"经验"传下去。

### Ruling 61 — 【计划缺陷，编译级；仅影响临时脚手架】Task 12 Step 5 的 `let v: dsh::Version = …` 无法编译（E0603）。

- 机理：`Version` 是 `dsh.rs` 内 `use crate::model::*;` 引入的**私有名字**，不经 `dsh::` 重导出。
- 实现者用 `model::Version` 写一次性脚手架（未入库）—— 正确处置。
- **我对"Task 18 也会踩到"这一预测的核验：不成立。** 全计划与 brief 13~19 中 `dsh::Version` 只出现在
这两行（3011/3015，均属 Task 12 的脚手架）；`main.rs` 自己 `use model::*;`，故 Tasks 17~19 里
直接写裸 `Version` 即可解析。**不向 Task 18/19 派发此警示**，以免制造伪问题。
- 措施：计划中这两行改为 `model::Version`。

### Ruling 61（已落地） — 计划中 Task 12 Step 5 的 `dsh::Version` → `model::Version`；

已全仓 grep 核实该模式仅此两行，**不向 Task 13~19 传播此警示**（伪问题）。

### Ruling 62 — 【跨任务小改，授权捆绑】`src/dsh.rs:290-291` 的注释带**已被推翻的错误机理**。

- 裁决：**捆进 Task 13 的派发**（该任务本就要改 `src/dsh.rs`），不作为独立修复轮；并在派发里
明确标注这是**我授权的 brief 外编辑**，让 Task 13 的评审者能看到它、而不是把它当成夹带。
- 代价若错：Task 13 的 diff 多两行注释改动，评审者会看到并复核 —— 非静默。

### Ruling 63 — 【**我的变异规格写错了**，实现者用实测挡下】M2 不能判别，而陷阱**确实已被钉住**。

- 我指定的 M2 是 `cols[1].contains(&format!(":{port}"))`，期望 `parse_netstat_does_not_match_port_suffix` 变红。
实现者实测：**它不变红**（唯一信号是 `warning: unused variable: target`）。
- 机理（实现者给出，我复核认可）：该"变异"**保留了 `':'` 锚点**，而 `":3080"` **确实不是**
`"127.0.0.1:13080"` 的子串（那里冒号后面是 `1`）—— 所以它根本没改动被守护的行为，属**假变异**。
- 真正的变异是**去掉锚点**：M2' `!cols[1].contains(&target)`、M2'' `!cols[1].ends_with(&target)`
—— 实现者实测**两者都红**（`left: Some(999) / right: None`，全量 69 passed / 1 failed，零连带）。
**结论：第三个陷阱（`:3080` 误匹配 `:13080`）确实有牙齿**，只是我给的变异形式不对。
- 记录正确的变异形式，供后续任务复用：**要变异"锚定比较"，必须真的去掉锚**，否则等于没变。
- 代价若错：无（陷阱已被 M2'/M2'' 实测钉住）。

### Ruling 64 — 【注释措辞，授权捆绑】`src/dsh.rs` 中 `parse_netstat_pid` 的文档注释第 2 条写

"否则 `":3080"` 会匹配 `":13080"`" —— **措辞不准**：带 `':'` 锚点时并不会匹配。
真实的危险是**无锚的末段匹配**（`ends_with("3080")` / `contains("3080")` 都会命中 `:13080`）。
- 措施：**捆进 Task 14 的派发**（该任务同样改 `src/dsh.rs`），并要求像 Ruling 62 那样
**在报告与提交正文里自曝**，避免被当成夹带。改动仅注释。
- 代价若错：Task 14 的 diff 多一行注释改动，评审者会看到并复核。

### Ruling 65 — 【Important，安全；裁决：**修**】`open_url` 是**面向远程内容的 shell 注入点**。

- 机理：`Command::new("cmd.exe").args(["/c", "start", "", url])` 把 URL 交给 **cmd 自己的解析器**；
Rust 的 Windows 参数编码**只对空格/制表/引号加引号**，**不转义** `& | ^ < > % !` → `&` 会被 cmd 当命令分隔符。
- 可达性：Task 19 的 `on_link_clicked` 把 **release notes 里抓来的链接**（网络内容）直接送进
`Job::OpenUrl` → 一条 `[x](https://a/&calc.exe)` 的说明，用户一点即执行任意命令。
- 为什么算 load-bearing 而非风格问题：这是**信任边界**（网络 → 本地 shell 执行），
且违反 GC-9 的精神（"禁止拼接 shell 字符串"）。本项目的既有裁决一贯把信任边界上的输入校验
列为不可简化项。
- 措施：改用 `explorer.exe` + **单个参数**（无 shell），并加 **http/https 协议白名单**
（`explorer.exe` 对 `file:` 或裸可执行路径会**执行**目标）。
- 要求的判别性验证：`open_url("file:///C:/Windows/System32/calc.exe")` 必须返回 `Err` **且不弹出计算器**；
并用旧 argv 跑一次证明旧代码**确实**会弹出 —— 一正一反，才能证明修复真的堵住了。
- 代价若错：若 `explorer.exe <url>` 在某些浏览器组合下不打开，用户点"打开网页"没反应 ——
属**可感知**的失败（且 Task 19 的清单会测到），不是静默错误。

### Ruling 66 — 【Important；裁决：**修**】waiter 的 `join → wait()` 顺序是基于**错误理由**的潜在活性漏洞。

- 事实：原注释称"先 join 才能保证日志不截尾"。**不成立** —— 管道里已缓冲的字节在写端关闭后**仍可读**，
且两个 reader 线程本来就并发排空；改成 `wait()` 在前**不会**截断任何日志。
- 真问题：该顺序让 `wait()` 成为最后一步，而 `wait()` 是唯一"回收子进程 + 发送 `WebExited`"的地方。
只要有孙进程仍持有我们的 stdout/stderr 写句柄（`dsh web` 自己就会起 `cmd /c start` 这样的孙进程），
reader 就永远读不到 EOF → `wait()` 永不执行 → **界面永远停在"运行中"**。
- 本次实测**未触发**（WebExited 到达时 Chrome 仍开着，说明 Chrome 没继承管道）→ 属**潜在**而非已坏。
- 措施：改成 `wait()` → 发 `WebExited` → `join` 两个 reader；并把注释改成正确理由。
- 代价若错：日志行可能在 `WebExited` 之后才到达（界面先置 Stopped 再追加日志）—— 无害。

### Ruling 67 — 【Important，但**不属于本任务**；裁决：改 **Task 19** 的 brief】NFR-7 在停止路径上有缺口。

- 评审者核实：计划的 `StartWeb` 与事务前置检查路径**都有** `is_node` 守卫（brief 18 的 98/185 行），
但**停止**路径没有：
- Task 19 窗口"停止"：`state.web_pid.or_else(|| find_listener_pid(port).ok())` —— 后一个分支**无守卫**；
- Task 19 托盘"停止"：`find_listener_pid(port)` —— **完全无守卫**；
- Task 18 的 `Job::StopWeb`：只拿 pid 调 `stop_by_pid`。
- 后果：若 dsh web 已死、而**别的程序**占住了配置端口，用户点"停止"会把那个程序 `/T /F` 杀掉 ——
正是 NFR-7 存在要防的事。
- **守卫不能下沉进 `stop_by_pid`**：我们自己启动的实例 pid 是 `cmd.exe` 包装进程，
`is_node` 对它为**假** —— 无条件守卫会拒绝停止我们自己的实例（实现者的 concern 1，评审者已确认）。
因此守卫必须加在"**pid 的来源**"处。
- 措施：改 Task 19 brief 的两处停止回调 —— 只有 `find_listener_pid` 那条分支（外部实例）需要
`is_node` 守卫，`web_pid` 分支（自启实例）不需要；守卫拒绝时发
`UiMsg::Failed { context: "停止 dsh web", message: "端口被非 node 进程占用，拒绝操作" }`。
托盘那条同样处理（它总是走 `find_listener_pid`，返回的是真正的监听者 `node.exe`，
所以守卫对"自启实例"也成立 —— 用 `find_listener_pid` 拿到的 node pid 带 `/T` 杀，包装进程会随之退出）。
- **实施时机**：等 Task 14 的修复轮跑完再改计划（Ruling 15/20）。届时一并把 Ruling 64 的注释再收窄一处
（M5：`:30800` 是合法端口，带锚的 `contains(":3080")` 对它**会**匹配，故注释应写明"不会误匹配 `:13080`"）。
- 代价若错：若 `is_node` 对某个正常的 dsh web 监听者返回假，守卫会拒绝停止 —— 用户看到明确的拒绝提示，
而非静默失败。

### Ruling 67 已落地：`233d081` —— Task 19 的两处停止回调补 `is_node` 守卫

（窗口那条：`web_pid` 分支不校验、`find_listener_pid` 分支必须 `is_node(pid)`，拒绝时写状态栏；
托盘那条同理）；同时订正 Task 14 代码块与提交正文模板里**已被推翻**的"先 join 再 wait"理由
（Ruling 66），并把 Task 13 的注释再收窄一处（M5：`contains(":3080")` 对 `:30800` 仍会命中）。
brief 13/14/19 已重生成。

### Ruling 68 — 【**用户可见缺陷，必须修**；待评审确认后进修复轮】暗色模式下大量文字**不可见**。

- 事实（实现者像素级证明）：本机 Windows 处于**暗色模式** → Slint 用 Fluent 暗色主题 →
`Palette.foreground` 为**白色**；而 brief 的卡片背景是**硬编码的浅色**（`#f5f5f5`/`#fafafa`/`white`），
于是**所有未显式写 `color:` 的 `Text` 都变成白底白字**。实测：`检测中…`（已安装版本）在 `#f5f5f5` 上
有 **231 个纯白字形像素、0 个深色像素**。
- 受影响的不止一处：**FR-9 的版本读数**、最新版本读数、web 状态词（运行中/已停止）、端口 `LineEdit` 的值、
web 卡片三个按钮的文字、关于对话框的标题与"关闭"。
- 为什么是 Important 而非样式问题：**FR-9 的核心读数在目标平台上不可读 = 需求未达成**。
GC-1 明确"只在 Windows 上验证"，而本机就是目标环境 —— 不能记为"环境假设"了事。
- 措施（实现者已实测有效、随后按规矩还原）：`std-widgets` 加导入 `Palette`，并在 `MainWindow` 上加
`init => { Palette.color-scheme = ColorScheme.light; }`（2 行）。整窗回归浅色主题，全部文字可读。
另注：直接在组件体里写 `Palette.color-scheme: …;` 是 **Parse error**，必须放 `init` 里；不导入会报
"Cannot access id 'Palette'"。
- 为什么选"强制浅色"而不是"处处显式上色"：窗口的**刻意设计就是浅色**（卡片底色全部硬编码），
让主题跟随系统只会让硬编码浅色与暗色控制件互相打架；强制浅色是唯一自洽的两行解。
- **本轮裁决**：等 Task 15 评审独立确认后，**在 Task 15 内开修复轮**修掉（含下面那个居中项），
不带到 Task 16 —— 保持"每个任务的评审门是干净的"这一惯例。
- 代价若错：若将来要做真正的暗色主题，需要把这 2 行换成整套主题感知配色 —— 那时本就是一次设计改动。

### Ruling 68（确认并加强） — 暗色模式不可见：**必须修**，且修法只能是"钉死浅色方案"。

- 加强证据：受影响面比初报更广（含 FR-27 正文与 AboutSlint 内部），
且 `Button` 标签色为 private → 逐处补色不可行；Rust 侧无对应 API → markup 是唯一落点。
- 措施不变：导入 `Palette` + `init => { Palette.color-scheme = ColorScheme.light; }`。

### Ruling 69 — 【Minor，一并修】关于对话框的卡片**左对齐**而非水平居中。

- 原因：`VerticalLayout` 的 `alignment: center` 作用于**主轴（垂直）**；交叉轴居中需要
`cross-axis-alignment: center`。
- 措施：与 Ruling 68 同轮修（1 行）。brief 的意图显然是"居中"，故这是**忠实实现意图**而非新增需求。

### Ruling 69（确认） — 关于卡片左对齐：`alignment: center` 是**主轴**居中，交叉轴需

`cross-axis-alignment: center`。评审者从 `layout.rs:1379-1387` 验证（stretch 下交叉轴位置 = `padding.begin`）。
**同一原因**还导致删除非法 `vertical-alignment` 后 web 状态小圆点跑到行首 —— 一并补
`cross-axis-alignment: center`。

### Ruling 70 — 【Important，**漏掉的需求**；裁决：**补进契约**】FR-29 的"关于"对话框缺 4/5 项。

- 证据（SRS 520-528 原文）：FR-29 要求对话框必须含 ①`AboutSlint` ②本程序版本 ③`dsh` 安装路径
④owner PM ⑤当前端口。而 markup 只有 ① 加标题/副标题，**且契约里没有任何属性可以接收 ②~⑤**。
- 后果为何严重：计划把 FR-29 记为由 Task 15 覆盖 —— 矩阵会显示**绿色**而实际不合规（追溯性失真）。
- 措施：Task 15 的修复轮补 4 个只读属性（`app-version` / `dsh-path` / `owner-pm` / `current-port`）
与对应显示行；**Task 17 的 `project()` 负责推送这四个值**（计划需同步改）。
- 附带收益：`PmEnv.dsh_path` 此前**没有任何消费者**（前瞻审计里记过），FR-29 的"dsh 安装路径"
正好消费它 ✓。
- 为什么现在做：这是"最后的廉价时刻" —— Tasks 17/19 尚未写，属性现在加是 4 行；
等推送逻辑写完再加就要同时改两处 brief 与已冻结的契约。

### Ruling 71 — 【Important；裁决：**修**】ComboBox / LineEdit 的**单向绑定会被控件自身赋值解除**。

- 机理（评审者从源码给出，非推测）：`current-index: root.pm-index` 编译成普通绑定
（`app.rs` 可见），而 Slint 在**首次赋值**时**解除绑定**（`properties.rs:349-359,723`），
偏偏这些控件自己就会赋值（`combobox-base.slint:24` 的 `root.current-index = index`；
`TextInput` 的编辑路径 `items/text.rs:1681`）。于是用户第一次选择/输入之后，
Task 17 的 `set_pm_index` / `set_version_index` / `set_port_text` **再也推不动控件显示**。
- 后果：Task 19 的回写掩盖了常见路径，但**目录刷新后版本列表可能重排**（新版本落到 index 0），
高亮行会与实际 `selected_version` 不一致 —— FR-10/FR-11 的"下拉显示当前值"失效。
- 措施：三处改 `<=>` 双向绑定，三个根属性改 `in-out`（生成的 Rust API 不变：`set_*` 保留，另加 `get_*`）。
- 代价若错：无（双向绑定正是这两个控件需要的语义）。
- **后续（2026-09-21）**：Fluent 的 `ComboBox` / `LineEdit` 已换成自绘的 `GlassSelect` /
  `GlassField`（见 ARCHITECTURE §2.4.3）。**本条结论对替换后的组件同样成立** ——
  `GlassSelect` 内部照样会自己写 `current-index`（选项被点中时），
  `GlassField` 内部的 `TextInput` 照样会自己写 `text`。所以三处 `<=>`
  双向绑定**必须保持**，换成单向绑定会重现本条描述的问题。

### Ruling 72 — 【Minor，真实；裁决：随 Task 16 处理，且必须**实测**】说明区正文在 `ScrollView` 里**水平居中**。

- 机理（重评审者给出，比实现者的描述更准）：Slint 对**隐式尺寸的非布局子元素**执行
`maybe_center_in_parent`（`passes/default_geometry.rs:604`），而 `StyledText` 正是 `@implicit_size`；
于是短内容居中，长内容因**不换行**而比卡片更宽、两侧被裁。
- **实现者建议的修法是错的**（重评审者纠正）：`horizontal-alignment: left` 无用（内层本来就是 `start`）；
`width: 100%` 对**直接的 flickable 子元素**也不够（`default_geometry.rs:459` 的 fill 分支要求非 flickable）。
可行方向是显式定宽/定 x，或**把 `StyledText` 包进一个布局**（布局会把宽度交给子元素）。
- 措施：随 **Task 16** 派发（同一文件、同一轮视觉验证），要求先试 `width: 100%`、不行就包布局，
并用**长样本**实测"左对齐 + 在卡片宽度内换行 + 无横向裁切"，报告 x 范围与换行行数。
- 为什么算 Minor 而非 Important：内容短时只是观感；内容长时才会裁切，而真实 release notes 尚未接入
（Task 17/19 才推 `notes-text`）—— 在接入之前修掉即可。
- 代价若错：若三种做法都不换行，就需要在 Task 17 侧限制文本宽度或改用手写换行 —— 届时会由实测暴露。

### Ruling 73 — 【Important，plan-mandated；裁决：**授权扩展 markup**】`on_clicked` 在 Slint 1.18 对托盘组件**不生成**。

- 评审者**独立核验成立**：生成物 `impl AppTray` 里**零** `on_clicked`/`r#clicked`，
且**没有任何** `set_icon/set_tooltip/set_title/set_visible` —— 即**根元素的内建成成员从不进入公开 API**
（同理 `MainWindow` 拿不到 `on_close_requested`，SRS §429 已记录该限制）。
编译器自带的 `tray.on_clicked(...)` 文档示例**对 1.18.0 而言是错的**（上游文档 bug，非本项目问题）。
- 而 **SRS §450 与 V-9 是具名要求**（"左键单击触发 `clicked()`"），brief 16 Step 3 也写着"Task 19 接上"，
但**计划里没有任何地方**接线它 → **V-9 按原计划永远无法通过**。
- 措施：在 `AppTray` 内**自行声明** `callback tray-clicked();` 并在组件体内
`clicked => { root.tray-clicked(); }` 转发（**不能**与内建同名）。
- 依据：内建 `clicked` 确实会在左键时触发（`windows.rs:295-298` → `:82-85`，作用于**组件根元素**，
正是组件体内绑定能挂上的位置）；私有 unstable API 路线不可行（item tree 类型是私有的）。
- 代价若错：多一个回调名，零行为风险。

### Ruling 74 — 【配套】Task 19 的 `wire_callbacks` 增加托盘左键接线：

`tray.on_tray_clicked(..)` → **显示主窗口**（与菜单第一项 `show-window` 同一动作，可共用闭包）。
- 依据：V-9 只要求"回调被触发"，但一个什么都不做的回调不构成可观察行为；
左键显示主窗口是托盘应用的通行语义，且与 V-8/FR-23 的"关窗隐藏"形成闭环。
- 落地方式：计划里 Task 16 段（回调声明）与 Task 19 段（接线）同步改写；brief 16/19 重生成。

### Ruling 75 — 【Important，plan-mandated；裁决：**修**】"首帧显示加载态"是**注释与提交正文里的空头承诺**。

- 事实：`project()`（src/main.rs:193-201）对 `env.installed == None` **无条件**映射为 `"未检测到 dsh"`，
而首帧 env 是空的 → 窗口从第一帧起就写着"未检测到 dsh"。但 `ui/app.slint` 给 `installed-version` 的
默认值是 `"检测中…"`，且计划正文"已实测确认的平台事实"里明确写着
**"数据到达前的空窗期必须显示加载态"** —— 需求与实现不一致。
- 影响时长不是 290ms：首个 `UiMsg::Probed` 要等 `pm::probe_env` 跑完，而它**最多起 8 个子进程**（Task 7 的
minor 已记过），实际是**秒级**。也就是说：**装好了 dsh 的用户，会在启动后数秒内被告知"未检测到 dsh"** ——
这不是观感问题，而是给出**错误结论**。
- 措施：`AppState` 加 `probed: bool`（初值 false），在 `drain` 的 `UiMsg::Probed` 分支置真；
`project()` 在 `!probed` 时推 `"检测中…"`，之后才轮到 `"未检测到 dsh"`。约 5 行。
- 依据：这是**计划自己写下的**要求（且 `app.slint` 的默认值就是为它准备的），不是新增需求。
- 代价若错：若 probe 永远不返回，界面会一直停在"检测中…" —— 但那只在 PM 探测彻底卡死时发生，
且比谎报"未检测到"更诚实。

### Ruling 75（确认并加强） — 加载态：**实测为未满足的需求**（不只是注释不准）。

- 评审者补强：**SRS:786 明文禁止**未检测先下结论；真实等待是**一次 `pm::probe_env`** ——
4 个 PM **各探测两次**（外层 + `read_dsh_version_on_path`），每个 2 条命令 → **约 9 次 shim 启动**，
实测机上 **1~3 秒**，比注释声称的 290ms **大一个数量级**；而 290ms（SRS:782）是**定时器回调**里程碑，
被 Task 17 的注释误用了。→ 装了 dsh 的用户会在启动后数秒内被告知"未检测到 dsh"。

### Ruling 76 — 【配套，Task 19；计划缺陷】Task 19 的 brief **同样缺 `win.show()?;`**（brief 19:315 附近）。

- 后果：Task 19 的 V-1 检查（"启动 → 窗口出现"）**必然失败**，且症状与 D-3 完全一样（只剩托盘）。
- 措施：给 Task 19 的 `main` 在 `run_event_loop_until_quit()` **之前**补 `win.show()?;`，并同步进计划。
- 为什么现在就要改：Task 19 按 brief 逐字实现的话会重演一次"实现对了、brief 错了"，白花一轮。

### Ruling 77 — 【Important；**归属 Task 18**】§5.2 的"worker 已死"检测**按计划的写法永远不会触发**。

- 机理：`_msg_tx`（main.rs:433）是**真绑定**（前导下划线 ≠ `_`），活到 `main` 结束；而 `try_recv` 只有
**所有** 发送端都被 drop 后才返回 `Disconnected`。Task 18 的 brief 也把 `msg_tx` 留在 main 里。
- 措施（写进 Task 18 的计划与派发）：`spawn_worker(..)` 与孤儿恢复之后 **`drop(msg_tx);`**
（此后 main 只发 `Job`，不再需要它）。**另需一个 `worker_dead` 闩锁** ——
该分支一旦可达，会**每个 tick 重复触发**（`changed = true` → 每 80ms 全量 `project()` 一次，
并把之后任何状态文案覆盖掉）。
- 代价若错：崩溃检测仍不生效（但那是**计划原有**状态，不是本任务引入）。

### Ruling 78 — 【Important；本轮已修】FR-32 / V-26 的"记日志"在**部署形态下是空操作**。

- 机理（评审者从 std 源码给出）：`windows_subsystem = "windows"` 下没有控制台 →
`GetStdHandle(STD_ERROR_HANDLE)` 返回 NULL → std 映射为 `ERROR_INVALID_HANDLE` →
`Stderr::write_fmt` **恰好吞掉这个错误并返回 Ok**（所以既不 panic 也不输出）。
- 陷阱（重要）：**用 `cargo run` 跑 V-26 会因为继承控制台而"看起来通过"** —— 只有在部署形态才暴露。
- 措施：把消息带出 `config::load()` 的 match（`(preferred_port, Option<String>)`），
`state` 建好后 `push_log` 进日志面板；端口回退语义保持不变。

### Ruling 79 — 【Minor 但同属"假日志"一类；裁决：**修**】孤儿恢复的兜底分支会打出不成立的结论。

- 事实（我读了 `src/main.rs:735-746` 与实现者的 s3 实测）：结构是
`if port_in_use { if let Ok(pid) = find_listener_pid { if is_node { …; return } } }` → 兜底
`"上次的运行端口已空闲，状态已清除"`。但兜底同时覆盖两种情况：**端口被非 node 进程占用**、以及
**端口被占用但 `find_listener_pid` 失败** —— 两种情况下"已空闲"都是**假话**。
- 为什么算缺陷而非措辞问题：这是**写进日志的诊断**，而日志面板正是 FR-28 指定的失败诊断载体；
与 Ruling 78 修的启动日志、Ruling 65 修的降级原因属同一类（"别让程序说出与事实相反的话"）。
- 措施：把兜底拆开 —— 非 node 占用时写 `"上次的运行端口 {rp} 被非 node 进程占用（pid {pid}），视为过期并清除"`；
`find_listener_pid` 失败时写 `"…仍被占用但无法定位进程，视为过期并清除"`；只有**真的空闲**才说"已空闲"。
清除 + 回落偏好端口的语义保持不变。
- 代价若错：无（只是措辞更准确）。

### Ruling 80 — 【计划缺陷；裁决：改 brief 而不是改代码】Task 18 Step 4 第 4 条的**位置写错了**。

- 事实：`环境探测完成` 由 `drain` 写进 `s.status`（底部状态栏，FR-15 指定的进度/状态位），
**不是**日志行；brief 却要求"日志区出现"。实现者按字面判定为 FAIL 并把判断交回来 ✓ 正确做法。
- 裁决：**FR-28 要求日志承担"过程反馈与失败诊断"，并不要求把每一条状态都复述一遍** ——
状态栏已经承担了"当前状态"。若再 push_log 一条，同一句话会出现两遍（实现者也指出了这点），
属噪声。故：**改 brief 的措辞**（把该条改成"底部状态栏显示 `环境探测完成`"），**不动代码**。
- 代价若错：若审计者坚持要日志留痕，补一行即可 —— 但那是可选增强，不是本项目的既定要求。

### Ruling 80（确认） — Task 18 Step 4 第 4 条：**改 brief 措辞，不改代码**（详见上条）。


### Ruling 81 — 【Important；裁决：修，且**比我最初的裁决更保守**】孤儿兜底会把四种状态混成一句"已空闲"。

- 评审者枚举出四种到达该分支的状态：端口空闲（唯一与文案相符）、**被非 node 进程占用**、
**`find_listener_pid` 失败（此时已确知端口被占）**、**`is_node` 因 tasklist 失败返回假**。
后两种情况下 `running_port` 被清掉 = **把孤儿永久遗忘**（SRS 413-417 明说要避免的"静默失联"）。
- 额外的**真实场景**：`is_node` 是"进程名含 node.exe"，而 **bun 托管的 dsh web 是 `bun.exe`** ——
于是 bun 用户的孤儿会被判为"非 node"。
- 根因（评审者点出，我采纳）：**`is_node` 是 NFR-7 的 fail-closed 击杀守卫，被复用成了"身份判定"** ——
它的 `Err → false` 对 `taskkill` 是对的，对"收养孤儿"是错的。
- 措施：拆成三态 —— 占用且是 node → 收养为外部实例；占用但非 node → 记录日志；
占用但定位失败 → 记录日志；**后两种都保留 `running_port` 记录**，只有真的空闲才清除。
- **与评审者建议的分歧（我裁决）**：评审者建议"非 node 时清除"，我选择**保留**。
理由：非 node 与"bun 托管的 dsh web"在现有谓词下**不可区分**，而 FR-22 的目的是**找回孤儿**；
保留记录的代价只是下次启动多一次廉价探测 + 一行日志，而清除的代价是**永久失联**。
记录会在用户真正启动 dsh web 时被 `start_web` 覆盖，故能自愈。
- 代价若错：若某台机器上配置端口长期被无关程序占用，每次启动会多一行提示 —— 可接受。

### Ruling 82 — 【Important，**漏掉的需求**；裁决：修】FR-28 的"执行的命令原文"在成功路径上被丢弃。

- 事实：`SystemBackend::log` 是空实现（计划明文如此，理由是"Backend 拿不到 Sender"），
于是 `txn::install`/`uninstall` 精心构造的 `$ npm.cmd install -g @deepseek-ai/dsh@…` 从未进入日志面板。
FR-28 的第一条日志内容就是"所有执行的命令原文"，其依据是"npm install 可能耗时 30 秒以上，
日志面板承担该期间的过程反馈与失败诊断"。
- 措施：`SystemBackend { tx: Sender<UiMsg> }` + `log` 转发为 `UiMsg::Log`，调用点用 `SystemBackend::new(tx.clone())`。
**trait 签名不变，`txn` 仍不依赖 UI** ✓（这正是计划里那句"trait 不持有 UI 依赖"的本意：依赖应放在实现里，而不是把日志丢掉）。
- 代价若错：日志多几行（正是用户要的）。

### Ruling 83 — 【Important；裁决：修】worker 没有 panic 保护，而 §5.2 的兜底在 `dsh web` 运行时**接不住**。

- 事实：`catch_unwind` 只包了 `Job::Probe`；其余全部裸跑。一次 panic 会 unwinding 掉 worker、丢掉
`Receiver<Job>`，此后所有 `job_tx.send` 返回 `Err` —— 而调用点**丢弃**该 Err（Task 19 也会）→
**界面还活着，但所有操作静默无效**（§5.2 自己说这是最难诊断的故障）。
- 更关键：**这条兜底本身也被堵死** —— `dsh::spawn_web` 的两个 reader 线程与 waiter 各持一个
`Sender<UiMsg>` 克隆，只要 `dsh web` 子进程活着，通道就不会 `Disconnected`。
即"worker 死了 + dsh web 在跑"这一最常见组合下，**不会有任何提示**。
- 措施：整圈 `execute` 包 `catch_unwind(AssertUnwindSafe(..))`，失败时 `Log` + `Failed{context:"任务执行"}`，
让故障**可见**（GUI 子系统没有 stderr）。
- 代价若错：无（多一层 catch）。

### Ruling 84 — 【Important；裁决：修**根因**】`config::update` 是**非原子的读-改-写**，而现在有三个线程在调它。

- 事实：`update` = `load()` → 改 → `save()`。Task 18 引入了**第三个**写入者（孤儿探测线程），
与 worker、UI 线程并发 → 后写的一方用**自己读到的旧快照**覆盖前一方刚写入的字段。
具体损失：`start_web` 刚写入 `running_port = Some(3080)`，孤儿线程（或上一个子进程迟到的 `WebExited`）
用旧快照保存 `None` → **活着的实例记录消失**，正是 FR-31 要防的事，且事后完全不可见。
- 修法：在 `config::update` 内部加一把**进程内 `Mutex`**（一处守卫，四个调用点全部继承），
并**容忍中毒**（`unwrap_or_else(|e| e.into_inner())`）—— 锁中毒不该把程序带走。
- 为什么改 `config.rs`（Task 4 的产物）而不在调用点各自加锁：调用点已有四个且 Task 19 还会加，
锁必须住在"读-改-写"这一步里才成立。
- 代价若错：`update` 之间串行化（一次文件 I/O 的时长）—— 这些调用本来也不在热路径上。

### Ruling 85 — 【GC-16 违反 + NFR-7 守卫位置；裁决：**并入 Task 19**】`Job::StopWeb` 载荷改为 `{ port, own_pid }`。

- 事实：Task 19 的两个停止回调（窗口 / 托盘）原设计在 **UI 线程**上调 `find_listener_pid`（起 `netstat.exe`）
与 `is_node`（起 `tasklist.exe`）—— **GC-16 明令禁止 UI 线程出现可感知阻塞**，而这是**起子进程**。
- 第二重问题（Task 18 评审的 Minor #5）：NFR-7 的"是不是 node 进程"守卫被放在**调用方**、还要写两遍，
一旦未来有别的发送方，就能绕过守卫直接 `taskkill /T /F`。
- 措施：载荷改为 `{ port, own_pid }`；`own_pid` 优先（那是我们自己的子进程，无需校验），
否则**在 worker 线程**现场定位并加守卫；两个回调各缩为一行。
- 归属：改 `model.rs` 的 `Job` + Task 18 的 `execute` 分支 + Task 19 的回调 —— 一并由 Task 19 落地并披露。
- 代价若错：停止外部实例时多一次 worker 线程上的 netstat（约数十毫秒，可接受）。

### Ruling 86 — 【两项精度修复；裁决：**并入 Task 19**】

1. **`dsh::port_in_use` 要把"连接被拒"与"连接超时"分开**。依据是 Task 18 实现者**实测撞到的假阴性**：
监听者的 accept 队列塞满时，回环连接会**超时**而非被拒 → 原实现把"有人监听"读成"端口空闲"。
后果两条：**FR-22 的孤儿记录会被错误清除**（正是 Ruling 81 想保住的），以及 `wait_port_ready`
在慢机器上可能误报"启动超时"。修法：`Ok(_) => true`，`Err(e) => e.kind() == TimedOut`
（回环上没有防火墙，超时只可能来自"有监听者但没 accept"）。
2. **worker panic 的载荷要进日志**：现在只报"内部错误，请查看日志"，而日志里没有细节，
且 GUI 子系统下 stderr 为空 → **第一次崩溃完全无法诊断**（与 Ruling 78 同类）。从 payload 取消息写进 `UiMsg::Log`。

### Ruling 87 — 【裁决：**采纳实现者的解法**】`port_in_use` 的超时分支改为**查权威表**。

- 提交形态：
`Ok(_) => true`；`Err(e) if e.kind() == TimedOut => find_listener_pid(port).is_ok()`；`Err(_) => false`。
- 为什么对：超时在**本机**是**歧义**信号（空闲端口也会超时），不能直接当"占用"；而 netstat 的
LISTENING 表是权威的 —— 超时路径上问它一次即可判别。
- 为什么不用"把探针超时提到 2.5s"：那会让**每次**空闲端口探测都等 2.5 秒，而 `wait_port_ready`
是**每 200ms 轮询一次、最多 20 秒** —— 会直接毁掉启动就绪检测。查表只在超时路径发生，成本约 40ms。
- 代价与安全性：所有调用者都在 worker / 分离线程上（GC-16 ✓），`find_listener_pid` 走 `run_cmd`
带 `CREATE_NO_WINDOW`（GC-8 ✓，实测 0 个控制台窗口）。
- **教训（第三次同类）**：我又一次把"在这台机器上会怎样"当成了"必然如此"。实现者用六个端口的
实测数据挡下，而不是照着我写错的指令改 —— 正确做法。
- 代价若错：若某机器上空闲端口会被**立刻拒绝**（更常见的情形），那走的是 `Err(_) => false` 分支，
同样正确；若被占用则 `Ok(_) => true` 或超时后查表为真 —— **三条路径在新的实现下都成立**，
即它比我的版本**更宽**地正确。

### Ruling 88 — 【待评审确认；倾向：修】说明区的"选中版本"竞态。

- 事实（实现者自曝，并称它导致了自己第一次 V-6 读错）：`notes.status` 在切换版本时**不重置为 Loading**，
且 `UiMsg::Notes { version, .. }` 的落地**不与当前选中版本比对** → 串行拉取下，**迟到的那份会
覆盖/标注成当前选中的版本**（用户会看到另一个版本的说明）。
- 倾向修法（2 行）：在 `version-changed` 里把 `notes_status` 置 Loading；并在 drain 的 Notes 臂
比对 `version != selected_version` 时丢弃（"最新选择胜出"）。
- 严重性：显示**错版本**的说明属正确性问题（FR-27 要求的是所选版本的说明），但只在快速连续切换时出现。

### Ruling 89 — 【待评审确认；倾向：**记录为已知限制，不改代码**】系统代理不生效。

- 事实：ureq 默认只认 `HTTPS_PROXY`/`HTTP_PROXY` 环境变量，**不读 Windows/WinINET 系统代理**；
实现者实测本机直连 `api.github.com` 返回 **403（IP 小时级限流，remaining 0）**，而系统代理
`127.0.0.1:12450` 返回 200。V-6 因此是靠**注入 `HTTPS_PROXY`**（仅环境变量，无代码改动）才验成的。
- 为什么不改：读系统代理需要注册表/WinINET API —— 前者要 `winreg`、后者要 `windows` crate，
**GC-2 明令禁止新增依赖**（含 `windows`）。而失败路径是**优雅的**（非 404 的错误 → `NotesError::Net`
→ 界面显示"加载失败"）✓ 不阻塞任何主流程。
- 附带提醒（给人类）：实现者的 22 步下拉遍历**烧掉了本机 IP 的 GitHub 小时配额**；
这是**暂时性**的，一小时后自愈，且与本程序代码无关。

### Ruling 90 — 【裁决：进一轮修复】Task 19 的三项 Important + 四项小项。

1. **FR-21 漏洞（Important）**：退出只停 `web_pid`，而它要到 `Running` 才写入；`Starting` 不带 pid
→ **启动后约 2 秒内（或 20 秒超时窗口内）点托盘"退出"会留下无人认领的 `dsh web` 子进程**，
且 `running_port` 尚未写入使 FR-31 也无从恢复。修：`WebState::Starting { port, pid }`。
2. **退出路径无条件清 `running_port`（Important）**：`taskkill` 失败时**孤儿还在、恢复信号却被抹掉**
→ FR-22 与 FR-31 同时失效。修：仅在停止成功时清除，并把结果写进日志（worker 的那条分支本来就是对的）。
3. **回调写的拒绝原因永不被投影（Important）**：`project()` 只在 `drain` 有消息时跑，
于是 `on_install_clicked` 的两条拒绝路径只改 state、**界面上看不到**
—— "按钮像坏了"。修：加 `dirty` 标志，timer 在"有消息或有回调改动"时都投影。
4. **说明区会串版本（Important，评审者的裁决）**：切版本不重置 Loading，`Notes` 回复也不按当前选择过滤
→ 连续切换时显示**另一个版本**的正文/状态（且这正是实现者第一次 V-6 读错、并烧掉 GitHub 小时配额的原因）。
修：切换时重置为 Loading 并清 body/version；`Notes` 臂只接受 `version == selected_version` 的载荷。
5. **`WebExited` 不带身份（Minor，但同属 NFR-7）**：迟到的重复消息会清掉**新实例**的 `web_pid` →
陈旧 pid 会被退出路径 `taskkill`（pid 复用），而这一类我们已经修过两次。修：载荷带 pid，不匹配则忽略。
6. **`execute` 内层 catch_unwind 仍丢载荷（Minor）**：与外层同理由，把 panic 消息写进日志。
7. **系统代理不生效（Minor，诊断性）**：在两个 `fetch_*` 的错误串里加一句
"若本机仅允许系统代理出网，请设置 `HTTPS_PROXY` 后重启"。
为什么不做代理发现：`ureq` 默认 feature 集**编译掉了**系统代理发现，而 GC-2 禁止新增
registry/WinINET crate；改由 SRS 与 `docs/VERIFICATION.md` 明确记为**环境假设**（Task 20 的活）。

### Ruling 91 — 【Important；裁决：**修**（round 2）】`Starting` 期间"启动"按钮仍可点 → 可能起第二个子进程。

- 事实（实现者的 round-1 验证发现，我已从代码确认）：`is_running()` 只认 `Running`/`External`，
故 `Starting` 期间 `web-running` 为 false；而两个 `on_start_web` 回调**都不设 `busy`**
→ 按钮/菜单项在整个就绪窗口（正常约 2 秒，**超时路径最长 20 秒**）里都可点。
第二次点击会再派一个 `StartWeb`，而 `start_web` 的端口探测在第一份**还没绑定**时会判为"空闲"
→ 起第二个子进程，落选的那个成为**无人认领的孤儿**（正是本轮已修两次的 FR-21/FR-22 类）。
- 措施：两个回调各加"仅 `Stopped`/`Failed` 时接受启动"的守卫并把忽略原因写进日志。
**修在回调（源头）而不是 `app.slint`**：`ui/app.slint` 不在本任务的允许文件里，
且"拒绝重复启动"本就该由发起动作的一侧判断。
- 验证：用 round 1 的假 shim 复现（两次快速点击 → **恰好 1 个子进程** + 1 条拒绝日志；
且实测按钮/菜单项当时确实 `enabled=True`，证明守卫是**承重**的）、失败后仍可再次启动、超时后退出无孤儿 ✓
- 代价若错：用户在极端情况下少点一次"启动" —— 而重复启动的代价是 289MB 的孤儿。

### Ruling 92 — 【Minor（用户可见的静默失败）；裁决：**修**（round 3）】20 秒启动超时**没有任何提示**。

- 事实：超时分支只发 `WebState::Failed { reason: "启动超时" }`，而**没有任何属性渲染该 reason**
（界面只把 `web-running` 渲染成"已停止"）→ 用户等满 20 秒，看到的只有"已停止"。
- 措施：超时分支同时发一条 `UiMsg::Failed { context: "启动 dsh web", message: "启动超时：端口未在 20 秒内就绪" }`
→ 经 drain 的既有分支进入**状态栏与日志面板**（其余 Failed 分支本来就有，只有超时漏了）。
- 验证：假 shim 驱动一次超时，UIA 读回**两处**都出现（日志行 202,696,496,13 + 状态行 202,829,97,32），
且半启动的子进程仍被清理、下次启动仍被接受、退出无孤儿 ✓

### Ruling 93 — 【两项 Minor 残留；裁决：**并入 Task 20 的一行级修复**】

1. `start_pending` 被**任何** `UiMsg::Failed` 清除（含目录拉取、环境探测、停止 web 等无关上下文）
→ 复现：刷新 → 启动（排队中）→ 目录拉取失败 → 标志被清而 `StartWeb` 仍在队列 → 第二次启动被接受
→ 两个启动 → 第二个把**我们自己的**监听者标成 External → 退出不再停止自己的子进程。
修法：`WebState`（每个 `start_web` 出口都会发一条）**加** context 为 `"启动 dsh web"`/`"任务执行"`
的 `Failed` 才清（后者因为 `start_web` 内 panic 不发 `WebState`）。
2. quit 的 pending 分支记录的是**退出时读到**的 `preferred_port`，而排队的 job 捕获的是**接受时**的端口
→ 期间编辑端口会让记录指向错误的端口、孤儿仍失联。修法：接受启动时把在飞端口存进 `AppState`，
退出时记那个值。

### Ruling 94 — 【流程；裁决：**为这一处回归再修一次，然后停**】技能规定"最终修复波只有一次、之后不再开第二波"，

但这里的情况是：**修复波自己的 Important 交付项（I-4）被重评审判定为未达成且引入了可见回归**
（说明区永久"加载中"）。把它"parked 给人类"等于明知树里有一个一行可修的缺陷还交付。
- 措施：只修这一处回归（`drained.insert(0, job)` + 修正注释与 `VERIFICATION.md` 里那句被证伪的话），
顺带两处一行级加固（启动回调置 `dirty`；`spawn_reader` 对非 `InvalidData` 读错误改为退出循环），
然后做**一次**针对该小 diff 的 scoped 重评审，**不再开第三波**。
- 代价若错：多花一轮往返（约十几分钟），换来的是"修复波真的达成其目标"而不是把回归留给用户。

### Ruling 95 — 【两项**仅文档**的 Nit；裁决：**parked（不改）**，记给人类一句话即可】

1. `docs/VERIFICATION.md:175`（M-4 的记录）仍写"`map_while` → `filter_map`"，而 `0f2cb9b` 已把实现换成**显式 match**
→ 它描述的行为仍成立，但**点名的机制**已过时。
2. `docs/VERIFICATION.md:155`（上一波 A/B 行）仍把当时读到的"加载中"解释为"那一个请求还在飞"，
而本波自己的对照构建表明：那个二进制里存活的是**被放弃的选择**、其回复会被丢弃 ——
即它**很可能是永久卡住而不是在飞**。
- 理由：两句都在文档里、不改行为；而"不再开第三波"的裁决（Ruling 94）与本项目的收尾节奏都指向"停"。
- 代价若错：未来读者会被这两句误导一次 —— 一句话即可改，且**本 ledger 已把真相记下**（就在你正读的这一段）。

---

**尾注**：每个任务的评审包、修复轮报告与本账本原件都只存在于 SDD 草稿工作区
（`.superpowers/sdd/IMPLEMENTATION-PLAN/`，git-ignored），**分支合并后即被删除**；
它们引用的提交本身留在 git 历史里 —— 需要复核某条裁决时，按该条提到的 commit 号去查。

---
---

# 主题系统的平台事实与设计后果（`feat/theme-system`）

上面 101 条属于 `feat/implementation`。本节属于 **`feat/theme-system`**，记录的是那条分支的
**平台事实**（不是本仓库的设计选择，是平台行为）与 **§5.2 可达性变化**。

⚠ 本节裁决引用的是**另一条分支的编号序列**：原文在 git-ignored 的
`.superpowers/sdd/2026-09-22-theme-system/progress.md`（到 Task 9 为止共 27 条）。
**两套编号不共享序列** —— 本节的「Ruling 26」与上面的「Ruling 26」（Task 3 的 Produces 笔误）
是两个不同的编号。引用时请写明分支。

为什么平台事实要单独留档：本子系统里"看起来对"的接线被实测推翻过或差点被当成理所当然，
而它们全都**不是读自家代码能看出来的**，只能在依赖源码里核实或实测。记下来是为了让下一个人不必重踩。

---

## 平台事实 1 — 应用**读不到**系统主题

| 入口 | 结果 | 出处（已核实） |
|---|---|---|
| `SlintInternal.color-scheme` | **编译错误**：`Cannot access id 'SlintInternal'` | `i-slint-compiler-1.18.0/tests/syntax/lookup/global.slint:37`。该文件是编译器的**语法测试断言** —— 即这是被保证的行为，不是巧合 |
| `NativeStyleMetrics.color-scheme` | 同上：`Cannot access id 'NativeStyleMetrics'` | 同文件 `:35` |
| `SlintContext::color_scheme()` | 存在，但只在 `private_unstable_api` 下 | `i-slint-core-1.18.0/context.rs:252` |

**设计后果。**

1. **「跟随系统」必须自己探测** —— 于是有了 `src/theme.rs::system_dark()` 与它的监视线程。
   这不是"没找现成 API"，而是**没有现成 API**。
2. 反过来，**`Palette.color-scheme` 是可以写的**（见事实 3）：它是 `std-widgets` 的普通全局属性，
   而 `SlintInternal` 不是。两件事容易混为一谈，别混。
3. ⚠ 谁若哪天认为 `private_unstable_api` "能用"，先读仓库的 GC 约束：本仓库不接受依赖未稳定 API。

---

## 平台事实 2 — winit 的探测**只作用于系统标题栏**，用 `uxtheme.dll` 序号 132，**不读注册表**

| 项 | 值 | 出处 |
|---|---|---|
| winit 的判据 | `should_apps_use_dark_mode() && !is_high_contrast()` | `winit-0.30.13/src/platform_impl/windows/dark_mode.rs:126-127` |
| 系统态取值 | `LoadLibraryA("uxtheme.dll")` + `GetProcAddress(132 as PCSTR)` —— **按序号，不读注册表** | 同文件 `:130-135`（序号常量在 `:134`） |
| 三条失败路径 | 版本不达标 / 取不到模块 / 取不到序号 → 一律判**浅色**（`unwrap_or(false)`；`try_theme` 收尾落 `Theme::Light`） | 同文件 `:61`/`:80`/`:153` |
| 版本门槛 | `RtlGetVersion` + `status >= 0` + major==10 + minor==0 + build>=17763 | 同文件 `:46-53` |
| 作用对象 | `try_theme()` → `SetWindowTheme(hwnd, "DarkMode_Explorer"/"")` + `SetWindowCompositionAttribute`，**只改窗口框架 / 标题栏** | 同文件 `:60-82`；调用点 `src/platform_impl/windows/window.rs:963` |
| 能否强制 | winit **有** `Window::set_theme(Option<Theme>)`（能强制框架明暗）；但 **Slint 1.18 完全没有对等的公开 API** —— `i-slint-core-1.18.0/window.rs` 与 `slint-1.18.0/lib.rs` 里 `theme` 一词出现 **0 次** | 两文件的全文检索 |

**设计后果。**

1. **我们的探测必须与 winit 同源**，否则会出现"标题栏已变暗、主体还是浅色"（或反之）。
   所以 `system_dark()` 是 winit 判据的**逐条镜像**：同一个 uxtheme 序号、同一个版本门槛
   （**含 major/minor，不只是构建号**）、同样的高对比度排除、同样的"三条失败路径一律判浅色"。
   **它刻意不读注册表** —— 早先有一版注册表回落，Ruling 17（本分支）已判定它不是安全网而是
   **分叉源**（它只在 winit 判浅色的那几条路径上被触发，于是恰好制造出不一致），已删除。
2. **强制明/暗改不动系统标题栏，是平台限制。** 用户选"深色"时主体变暗、标题栏仍按系统主题绘制。
   这一点**必须向用户交代**（已在设置面板内写明），否则会被当成 bug 反复报。
   ⚠ **别去试着"修"**：唯一的路是绕开 Slint 拿 HWND 直接调 `DwmSetWindowAttribute`，
   那是在 UI 框架之外撬窗口，与本仓库的约束冲突，且升级 Slint 时必碎。
3. ⚠ **维护契约**：若哪天 winit 放宽了它的判据（例如支持主版本不再是 10 的系统），
   `dark_mode_supported()` 必须**同步放宽**，否则又分叉。该函数头部已写明这条。

---

## 平台事实 3 — `Palette.color-scheme` **只能**经 `changed` 处理器里的赋值切换

**事实。** 在 `.slint` 组件体里把 Fluent 配色写成绑定是**语法错误**：

```slint
Palette.color-scheme: dark ? ColorScheme.dark : ColorScheme.light;   // ❌ Parse error
```

而写成 `changed` 处理器里的赋值则可用：

```slint
changed is-dark => { Palette.color-scheme = is-dark ? ColorScheme.dark : ColorScheme.light; }  // ✅
```

**成因（已在编译器源码核实）**：`i-slint-compiler-1.18.0/parser/element.rs:66` 的
`parse_element_content` 只在 `SyntaxKind::Identifier` 且 `p.nth(1).kind() == SyntaxKind::Colon`
时走进 `parse_property_binding`。限定名 `Palette.color-scheme` 的第 2 个 token 是 `.`（Dot），
进不去绑定分支；而 `changed` 处理器体走的是**语句**路径，赋值天然可用。

**设计后果。**

1. **Fluent 配色同步只能写成"局部属性镜像 + `changed`"**：

   ```slint
   property <bool> is-dark: Tokens.dark;     // 活的绑定：Tokens.dark 一变它就变
   changed is-dark => { Palette.color-scheme = is-dark ? dark : light; }
   ```

2. ⚠⚠ **`init => { … }` 那一行不构成同步，它只会永远写 dark。** `init` 在 `MainWindow::new()` 内执行，
   早于 `main()` 的第一次 `set_dark()`；那一刻 `Tokens.dark` 还是**声明缺省值 `true`**。
   于是浅色档的 Fluent 控件（`ScrollView` 滚动条 / `AboutSlint`）正确与否，
   **完全取决于之后 `set_dark(false)` 时 `changed is-dark` 是否真的触发**。
   这曾经只是"看起来对"的推理（Ruling 26 把它列为必测项）—— **Task 9 用离屏探针实测确认它确实触发**：
   AboutSlint 的 logo 药丸与默认前景色在 `dark=true/false` 两档逐像素互换，数字见 `docs/VERIFICATION.md`。
   **若删掉 `changed` 那一支，浅色档的 Fluent 控件会整片停在深色。**
3. **不得**从 Rust 侧另找路子写 `Palette`：`FluentPalette` 是 `std-widgets` 的全局，
   生成的 `MainWindow` 上不存在这个 global（Rust 侧没有访问器）—— 这正是"两条投影路径"的原因，
   见 `docs/ARCHITECTURE.md` §4.7.2。

---

## §5.2 可达性变化 — 「worker 已死」兜底从无条件可达变为**条件可达**（已接受，不重构）

**事实（Task 6 引入）。** 系统主题监视线程为了送 `UiMsg::SystemThemeChanged`，
为**进程生命周期**持有一个 `Sender<UiMsg>` 克隆。后果：

- `msg_rx.try_recv()` 再也不会返回 `Disconnected`（只有当**所有** sender 都丢弃时才会）；
- 于是 `drain()` 里 §5.2 那条"worker 已死"兜底分支、以及配套的 `worker_dead` 闩锁，
  **从无条件可达变为条件可达** —— 仅当监视线程自己 panic（或它的两条早退路径
  `CreateEventW` 失败 / `WaitForSingleObject` 失败）因而释放那个 sender 时，才重新可达。

这与 **Ruling 77**（`feat/implementation`）记的"该兜底按计划的写法永远不会触发"叠加：
那条兜底被**进一步**收窄了。

**决定：接受，不重构。** 依据是仓库自己的先例 —— Ruling 83 已判定那条兜底本就不可靠
（`dsh::spawn_web` 的 reader 线程与 waiter 各持一个 sender，"worker 死了 + `dsh web` 在跑"
这一最常见组合下它根本接不住），并把真正的保护换成了 worker 的**整圈 `catch_unwind` + 显式
`Log`/`Failed`**（"让故障可见 —— GUI 子系统没有 stderr"）。那个保护**不受本改动影响**。
为恢复一个已被判定不足的分支去把 `drain` 改成双通道轮询，不划算。

**保留**分支与闩锁（不删）：监视线程若 panic，其 sender 随之释放，该分支仍可能重新可达；
删掉它等于把这条本就变窄的路彻底堵死。`src/main.rs` 里相关注释已由"兜底有效"改为事实描述。

**代价若错**：worker 崩溃时的提示能力比计划设想的更弱 —— 但 `catch_unwind` 那条路仍在，
且它是本轮真正依赖的那条。

---

## 附：`state.json` 的向后兼容（本分支唯一改动的持久化契约）

`feat/implementation` 的 `state.json` 是三字段（`preferred_port` / `running_port` / `close_behavior`）。
本分支增第四键 `theme_mode`，取值 `"light"` / `"dark"` / `"auto"`：

- **缺 key → `None` → `ThemeMode::Auto`（跟随系统）**。于是"默认跟随系统"这条需求不需要任何额外标志：
  没记过就是跟随。**不得**把缺 key 当成文件损坏，也**不得**回落成某个固定主题。
- **无法识别的值**（手改过、降级运行留下的）同样当"没记过"→ 跟随系统，比替用户猜一个固定主题安全。
- 旧三字段文件照常可用：单测 `legacy_state_without_theme_mode_reads_as_none` 钉住
  （且钉住旧字段不受影响）。Task 9 另用**真实二进制**在旧三字段文件上启动 12 秒做了端到端确认
  （不崩、不重写该文件），见 `docs/VERIFICATION.md`。
