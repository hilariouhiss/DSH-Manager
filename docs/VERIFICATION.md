# DSH Manager 验证记录

对应 SRS v1.2 §8.2 / §8.3 的验证清单。日期：**2026-09-20**。

> ⚠ **2026-09-22 增补**：SRS 已升至 **v1.3**，§8.2 新增 **V-27（系统代理，FR-33）**。
> 本文件下方「已知限制与环境假设」的第 1 条（代理只认环境变量）随之**改为已解决**，
> 但 **V-27 本身尚未人工执行** —— 它的第 ② 步（切「直连」后请求必须失败）是判别性的。
> 本文档其余部分的数字（如 `cargo test` 的通过数）停留在各自写作时点，未随之重跑。

被验证的代码：`feat/implementation` @ **`d8114ad`**（Task 19 的收尾提交）+ Task 20 的工作树改动
（`src/main.rs` / `src/model.rs` / `src/pm.rs` / `src/txn.rs` / `src/dsh.rs` / `src/config.rs`，
见本次提交信息），随后由 **`a646c9a`**（端到端验证记录）与 **最终修复轮**
（`fix: 收尾最终评审的四项 Important 与加固（TR-1/TR-2/FR-3.3/队列合并等）`，见
「最终修复轮记录（I-1 ~ I-4）」一节）继续。构建：`cargo build` **0 警告**，`cargo test`
**0 警告 / 84 passed**（最终修复轮，`cargo clean -p` 后强制全量重编译；Task 20 那一轮为 70 passed）。

**证据来源图例**（每行都标明来源，不混用）：

| 标记 | 含义 |
|---|---|
| **T20 实测** | 本次 Task 20 会话中在**本机实测**，用 UI Automation 读回 + `netstat` + `state.json` 观测 |
| **T19 实测** | Task 19 报告记录的实测（提交 `d8114ad`，同一份已评审代码；本次未重复执行） |
| **T18 实测** | Task 18 报告记录的实测（提交 `8f00bb4`，`SetWinEventHook` 控制台窗口对照） |
| **终修实测** | **最终修复轮**的实测（I-1 ~ I-4）：UIA 读回 + `netstat -ano` + `state.json` + GitHub 配额计数，端口一律用 3099 |
| **真实事务实测** | 本分支**唯一一次真实环境变更**（同 PM 重装 `@deepseek-ai/dsh@0.1.6-alpha.2`）的观测：UIA 读回 + `netstat` + 文件 mtime + 独立端口采样器，端口 3099，见「真实事务实测」一节 |
| **单测** | `cargo test` 中的具名单元测试（当前 84 项全绿） |

端口纪律（安全包线）：**3080 是本次验证所在会话的 live `dsh web`（pid 13432），全程未触碰**；
需要真实监听的场景一律用 **3099**，V-23 按原文用 8080（它从不启动监听）。每次观测前后都核对
`3080 pid` 未变。

---

## 环境

| 项 | 值 |
|---|---|
| dsh 版本 | **0.1.6-alpha.2**（`dsh --version`） |
| owner PM | **npm** —— `where.exe dsh` → `C:\Users\xueyu\AppData\Roaming\npm\dsh.cmd`；UIA 读回下拉值为 `npm  ·  dsh 安装于此` |
| Node / npm / pnpm | **v24.16.0** / **12.0.2** / **12.3.4**（pnpm 已安装但**不是** dsh 的 owner） |
| 操作系统 | Microsoft Windows 11 专业版，build 26200（**会话处于锁屏状态**，见「验证方法说明」） |
| 日志上限 / 排空间隔 | `LOG_CAP = 2000`（NFR-11）；UI 排空 timer 80 ms（架构决策 3） |

---

## 功能验证（§8.2）

| 编号 | 结果 | 证据 |
|---|---|---|
| **V-1** 启动程序 | **PASS**（T20 实测 + T18 实测） | T20：`dsh-manager.exe` 启动后主窗口出现，UIA 读回 `已安装` / `0.1.6-alpha.2` / `alpha 通道`。**无控制台窗口**：T18 用全局 `SetWinEventHook(EVENT_OBJECT_CREATE/SHOW)` 对照实验证明 —— 同一父进程、同一 argv/stdio 形状，只有 `CREATE_NO_WINDOW` 标志不同；探针 A（无标志）产生 `CASCADIA_HOSTING_WINDOW_CLASS` 控制台窗口，探针 B（有标志）11 秒内 0 个；**app 全程 12 个窗口事件中 0 个是控制台类**。T19 的四次启动同样为 0。 |
| **V-2** PM 探测 | **PASS**（T20 实测 + T19 实测） | T20：折叠态 `[ComboBox]` 值 = `npm  ·  dsh 安装于此`（两个空格 = `label` + 分隔符）。T19：展开后 `ListItem` 恰为 `npm  ·  dsh 安装于此`、`pnpm` —— **无 bun、无 yarn**。 |
| **V-3** owner 判定 | **PASS**（T20 实测 + T19 实测） | `  ·  dsh 安装于此` 后缀只出现在 npm 一行；与 `where.exe dsh` 的输出一致（见「环境」）。 |
| **V-4** 版本列表 | **PASS**（T19 实测） | 展开弹层 UIA 子树是**虚拟化**的（渲染行数 7 → 12 不等于总数），因此 T19 用键盘遍历模型的 22 步逐步读 `ValuePattern`：**恰好 22 个**、严格降序、每行带通道标注、`← 当前` 落在 `0.1.6-alpha.2`，第 22 步后选择不再变化（即计数上界）。 |
| **V-5** "已是最新"判定 | **PASS**（T20 实测 + T19 实测） | T20 首次读回即含 `最新` = `0.1.6-alpha.2`、`✓ 已是最新`。T19 全量确认窗口文本集合中**不存在** `↓ 可更新`，且 `0.1.5-rc.2`（npm 的 `latest` tag）**从未**被当作目标 —— GC-14 成立。单测另行钉住：`dsh::tests::gc14_latest_tag_must_not_be_used_as_newest`。 |
| **V-6** 更新说明 | **PASS**（T19 实测；正文有一处不可读，见备注） | T19 走完 22 个版本、每次读状态：`0.1.6-alpha.2` → 标题 `更新说明`（`NotesStatus.ok`）；`0.0.1-rc.1` → `更新说明 · 该版本无更新说明`（`NotesStatus.missing`）；Ok↔Missing 的翻转点恰好落在 `0.1.0-rc.7` / `0.1.0-rc.6` 之间，与独立从 `api.github.com` 拉到的 18 条 release 一致（交叉核对，不依赖阅读顺序）；0 次拉取失败。**备注**：正文本身（中英双语 markdown）**UIA 读不到** —— Slint 的 `StyledText` 未声明 `accessible-role`，而锁屏会话又排除了像素证据；因此"显示双语正文"这一条由 `NotesStatus.ok`（`Missing`/`Failed`/`Loading` 三态互相可区分且当时都不是）＋ API 侧 4849 字节正文支撑。 |
| **V-7** 启动 dsh web | **PASS**（T20 实测 + T19 实测） | T20（3099）：点 `启动` → UIA 在 **2107 ms** 内出现 `运行中` + `http://127.0.0.1:3099`；`netstat` → `127.0.0.1:3099 LISTENING`，持有者是 `node.exe`，命令行为 `…\@deepseek-ai\dsh\lib\bin.js web --port 3099`；日志含 `dsh web 已就绪：http://127.0.0.1:3099`。T19（3099，4 次）：1948–3011 ms，另观察到 dsh 自己打开了默认浏览器（新技术 `chrome.exe` 子进程）且**无黑窗口**。 |
| **V-8** 关闭主窗口 | **PASS**（T20 实测 + T19 实测） | T20：点 `隐藏到托盘` → 顶层窗口从 UIA 根的子级中消失（`present=False`），**进程仍存活**；随后（不重启进程）托盘左键又把窗口显示回来（`offscreen=False`）。T19：标题栏关闭 → `IsWindowVisible=false`、进程存活（约 104 MB）、托盘 message-only 窗口仍在、**3099 仍在 LISTENING 且 pid 不变**、日志新增 `主窗口已隐藏，程序仍在托盘运行`。 |
| **V-9** 托盘左键 / 右键 | **PASS**（T20 实测左键 + T19 实测） | T20 实测左键路径：见 V-8（隐藏 → posted `WM_TRAYICON` + `WM_LBUTTONUP` → 窗口重新可见）。T19：右键 → 真实 Win32 弹出菜单（`#32768`），5 个菜单项。**方法学注**：托盘窗口是 **message-only 窗口**（`EnumWindows` 列不出，`FindWindowEx(HW_MESSAGE,…)` 才找得到），且锁屏会话无法投递合成鼠标输入（`SendInput` 会打到锁屏界面），故托盘事件是 **posted message**，与物理点击走同一条 `TrackPopupMenu` 命令路径。 |
| **V-10** 托盘菜单状态 | **PASS**（T19 实测） | T19：运行中 → `显示主窗口` T、`打开 DSH 网页` **T**、`启动 dsh web` **F**、`停止 dsh web` **T**、`退出` T；已停止 → `打开 DSH 网页` **F**、`启动 dsh web` **T**、`停止 dsh web` **F**（逐项读 `MenuItem.IsEnabled`）。**本次未能复现该项的读回**（见「验证方法说明」第 4 条），故本轮未重复执行。 |
| **V-11** 托盘"退出" | **PASS**（T19 实测） | 运行中选 `退出` → 进程消失（两次运行分别 3 ms / 485 ms）、`3099` 不再监听，消失的 node pid 包含 3099 的持有者及其助手进程 → **无孤儿**；`3080` 仍为 `13432`；`state.json` 的 `running_port` → `null`。 |
| **V-12** 端口冲突 | **PASS（含一处呈现差异，见备注）** —— **T20 实测（本条为首次执行）** | 先在 3099 上跑一个**不是本程序启动**的真实 `dsh web`（`node.exe` pid 6184），再在 GUI 点 `启动`：日志出现 **`端口 3099 已被外部 dsh web 占用（pid 6184）`**，状态变为 `运行中` + `http://127.0.0.1:3099`，`state.json` 写入 `running_port: 3099`。**备注（呈现差异）**：SRS 写的是"提示…提供三个出口：打开网页 / 停止 / 改用其他端口启动"。实现里没有模态对话框 —— 检测结果直接落到日志与状态栏，`打开网页` / `停止` 两个出口是主界面按钮，"改用其他端口"由端口输入框 + 停止后重启动达成（`ui/app.slint` 无对应对话框；与 Task 14 的设计决策 2「自启/外部合并为同一条停止路径」一致）。另：按安全包线在 **3099** 而非 3080 上构造，端口号不影响该分支的代码路径。 |
| **V-13** 外部进程停止 | **PASS** —— **T20 实测（本条为首次执行）** | 两半都实测：(a) **非 node 占用者被拒绝**：用 PowerShell `TcpListener` 占住 3099（`pwsh.exe` pid 19184）→ 点 `启动` → 日志 `启动 dsh web失败: 端口 3099 被非 node 进程占用（pid 19184）`、状态 `启动 dsh web失败`、web 状态保持 `已停止`，**且该占用进程事后仍存活**（证明没有对无辜进程执行 `taskkill`，NFR-7 的守卫有效）。(b) **真正的 dsh web 能被定位并终止**：外部实例（node pid 6184）→ 点 `停止` → **445 ms** 内 `已停止`，`3099` 变为空闲，外部 node 进程消失，`running_port` → `null`。 |
| **V-14** 换版本事务（同 PM） | **部分执行** —— **同 PM 的真实安装已实测到 `Committed`**（见「真实事务实测」一节）；**"换版本"那一半仍未执行** | **已覆盖的那一半**：GUI 上一次真实事务端到端跑通 —— 点 `安装` → `事务开始：npm 0.1.6-alpha.2 → npm 0.1.6-alpha.2` → `$ npm.cmd install -g @deepseek-ai/dsh@0.1.6-alpha.2`（FR-28）→ 186.6 秒后终态 `已安装 0.1.6-alpha.2（npm）`（`Committed`）；同 PM ⇒ **S3 跳过**这一条也在真实日志里成立（全程无卸载命令）；安装后 `npm ls -g` / `where.exe dsh` / `dsh --version` 全部照旧。**仍未执行的那一半**：真正的版本迁移（例如 `0.1.6-alpha.2 → 0.1.6-alpha.1`）没跑 —— 那会把用户**唯一**一份 dsh CLI 降级，而它正是本次验证依赖的 CLI。**替代证据（单测）**：`txn::tests::same_pm_version_change_commits_and_skips_uninstall`（FR-13：同 PM 跳过 S3）、`model::tests::pm_command_table_is_exact`（钉住每个 PM 的确切 argv）、`txn::tests::cross_pm_migration_installs_then_uninstalls`（装/卸顺序）。 |
| **V-15** 迁移事务（npm → pnpm） | **未执行** —— 这是简报 Step 2，**已由控制器裁定不做**（本次实测后仍是这个结论） | 原因：`Job::Transact` 先按 TR-1 停掉 `dsh web`（本机就是**承载本次验证的 live 会话**，pid 13432），随后事务会 `npm uninstall` 掉 `@deepseek-ai/dsh` —— 半途失败会同时毁掉用户的环境与本次会话。**本次真实事务只走了同 PM 分支**（`origin.pm == target.pm` ⇒ S3 被跳过），因此**迁移分支本身（S3 卸载 + 跨 PM 的 S2/S4 判据）依然只有单测**：**20 个单元测试**（TR-4 / TR-5 / TR-6 / TR-11 及 V-17 / V-19 / V-20 / V-21）钉住其语义，这次真实运行另把**公共骨架**（precheck → S1 → S2 →【S3】→ S4 → 终态）在真实 PM 上走了一遍。**未观测**：任何一次真实的 `uninstall`、任何一次跨 PM 迁移。 |
| **V-16** 事务前置检查 | **部分执行**：拒绝路径仍未在真实事务里跑到 S1，但 **GUI 级构造已可行**（I-3 之后），并已实测 TR-3 的拒绝 | 拒绝路径的单测：`txn::tests::precheck_rejects_pm_bin_not_on_path`（TR-3）+ `rejected_target_produces_no_side_effects`（零副作用）；TR-2 有 `precheck_rejects_unavailable_pm` **与 `tr2_precheck_rejects_pm_whose_version_command_exits_nonzero`**（退出码非零 = 不可用，与 `pm::probe_pm` 同判据）。**NFR-6 由 `safe_version_rejects_injection_attempts` 覆盖**（`is_safe_version` 的字符集校验）—— `precheck` 里那条 `RejectReason::InvalidVersion` 分支经 `Target` **类型上不可达**（`semver::Version` 表示不出带注入字符的版本号），保留它是 Ruling 39 的具名实现，对应测试已改名 `precheck_invalid_version_branch_is_unreachable_by_type`，不再宣称覆盖 NFR-6（M-2）。**GUI 级（终修实测，新增可达）**：I-3 的 FR-3 第 3 步退化路径让"该 PM 的 bin 目录不在 PATH"变得可构造 —— PATH 去掉 `%APPDATA%\npm` 后 dsh 不在 PATH，而 `npm ls -g` 仍能报出已安装版本与 owner，于是点 `安装` 真的进到 `precheck` 的 TR-3：日志 `已拒绝：npm 的全局目录不在 PATH 中：C:\Users\xueyu\AppData\Roaming\npm。请先把它加入 PATH 再试`，**零副作用**（无安装命令、`state.json` 未变、真实 dsh 版本仍为 `0.1.6-alpha.2`）。**仍未观测**：一次真实事务中由前置检查拦下的场景（TR-3 之后的 S1 起都没跑）。**本次真实事务观测到的是它的通过路径**：`precheck` 返回 `None`（PM 可用、bin 目录在 PATH、版本号合法）后事务才进入 `S1`。**为什么之前构造不出来**：PM 的 shim 恰好住在它的 `bin -g` 目录里，把该目录移出 PATH 后 `probe_env` 连这个 PM 都探不到，根本走不到 `precheck` —— I-3 的退化路径恰好补上了这个缺口。 |
| **V-17** 事务失败补偿 | **未执行（GUI 级）** —— 需要一次真实失败事务 | 单测覆盖补偿全部关键分支：`txn::tests::v17_s3_failure_rolls_back_to_origin`（状态回到 `(PM_old, V0)`）、`c2a_cleanup_failure_is_reported_as_degraded_with_cause`、`c3_detects_incomplete_recovery_and_degrades`、`degraded_outcome_carries_runnable_manual_commands`、`tr11_compensation_probes_before_acting`。**未观测**：真实 PM 上的回滚。 |
| **V-18** 事务期间 UI | **部分执行**（真实事务实测） | 事务进行中 `安装` 按钮 `IsEnabled=False`（`busy` 经 `project()` 投影为 `enabled: !root.busy`，实测生效）；日志行**在事务运行期间**就能从面板读回（不是结束后一次性出现）；每 250 ms 一次的 UIA 读回在 185 秒里没有一次超时 —— 即 UI 线程没有被事务阻塞。**仍未观测**：事务期间用**物理输入**验证界面可交互（拖动/点击其他控件）、以及托盘侧在事务期间的表现。 |
| **V-23** 偏好端口持久化 | **PASS**（T19 实测按原文 8080；T20 另测 3099 读路径） | T19：端口 `[Edit]` `SetValue("8080")` → 字段读到 `8080` 且 `state.json` 变为 `{ "preferred_port": 8080, "running_port": null }`（FR-30 写路径，且**不派发任何 Job**）；杀进程重启 → 字段仍是 `8080`（读路径）。T20：预置 `state.json` 的 `preferred_port = 3099` 后启动，UIA 读到端口字段为 **`3099`**。 |
| **V-24** 运行态端口记录 | **PASS**（T20 实测全周期 + T19 实测） | T20（3099）完整周期：启动就绪后 `state.json` = `{ "preferred_port": 3099, "running_port": 3099 }`；管理器被强杀后**仍是** `running_port: 3099`（这正是孤儿恢复的信号）；点 `停止` 后 → `running_port: null`。T19：启动/停止/托盘停止/退出四条路径同样收敛。 |
| **V-25** **孤儿恢复（FR-22 + FR-31）** | **PASS —— T20 实测（FR-31 存在的唯一理由，必验项）** | 五步全过程见下方「V-25 实录」。要点：非默认端口 3099 启动 → 强杀管理器留下孤儿 → 孤儿存活且 `running_port` 仍在 → 重启后界面 `运行中` + `http://127.0.0.1:3099`，日志 `检测到外部 dsh web 运行在端口 3099（pid 23124）` → 点 `停止` 在 **432 ms** 内真正终止该孤儿（3099 变空闲、pid 23124 消失、`running_port` 清空）。 |
| **V-26** 持久化失败降级 | **PASS**（T19 实测） | `state.json` 写成 `{ this is not json` → 程序**正常启动**，端口字段回落 **`3080`**，日志恰好一条 `state.json 损坏，使用缺省端口 3080：JSON 解析失败: key must be a string at line 1 column 3`，进程拥有的顶层窗口中没有 `#32770` 对话框（0 个），且 3080 全程未被接触。 |

### V-25 实录（首次执行，端口 3099）

```
1) 预置 state.json = {"preferred_port": 3099, "running_port": null}，GUI 端口字段读回 3099
   点「启动」→ UIA 2107 ms 后读到 「运行中」+ http://127.0.0.1:3099
   netstat -ano | findstr :3099
     TCP    127.0.0.1:3099    0.0.0.0:0    LISTENING    23124     (node.exe, dsh web --port 3099)
   state.json → { "preferred_port": 3099, "running_port": 3099 }

2) 强制结束管理器（模拟崩溃；等价于 taskkill /F /IM dsh-manager.exe）
     manager pid 10340 → 进程消失；杀之前它唯一的子进程是 cmd.exe pid 9560
     （杀后未再复查那个 cmd —— 与本项结论无关：要紧的是**端口没被释放**，见下一步）

3) 确认孤儿仍在（关键：管理器已不在，监听未断）
   netstat -ano | findstr :3099
     TCP    127.0.0.1:3099    0.0.0.0:0    LISTENING    23124
   state.json → { "preferred_port": 3099, "running_port": 3099 }   ← FR-31 的恢复信号仍在

4) 重启管理器 → UIA 读回（原样摘自日志面板）
     Text  「上次的运行端口为 3099，探测中…」
     Text  「检测到外部 dsh web 运行在端口 3099（pid 23124）」
     Text  「运行中」 / 「http://127.0.0.1:3099」
   ← pid 与 netstat 的 23124 一致（不是"碰巧显示了运行中"，而是真的认出了那个孤儿）

5) 点「停止」→ 432 ms 后 UIA 读到「已停止」；netstat :3099 无 LISTENING；
   进程 pid 23124 已不存在；state.json → { "preferred_port": 3099, "running_port": null }
```

**若第 4 步显示"已停止"**意味着 FR-31 未生效、孤儿静默失联 —— **本轮未出现**。

---

## 关键约束验证（§8.3）

| 编号 | 结果 | 证据 |
|---|---|---|
| **V-19** TR-4：验证不走 PATH | **PASS**（单测） | 判别性测试 `pm::tests::tr4_read_dsh_version_at_executes_shim_in_that_directory`：在临时目录放一个打印 `9.9.9-tr4probe` 的真实 `dsh.cmd`，正确实现必须执行**该目录**下的 shim 拿到这个版本号；一个"忽略 `dir`、走 PATH"的实现会因为本机 PATH 上真有一份 dsh 而返回 `0.1.6-alpha.2` → 断言失败。事务侧另有 `txn::tests::tr4_verify_does_not_use_path`（S2 用 `dsh_version_at(target_dir)` 而非 PATH）。SRS 原文要求"构造 pnpm → npm 迁移场景"实测 —— 那正是本轮被裁定的 V-15（未执行，见上）。 |
| **V-20** TR-6：先装后卸 | **PASS**（单测 + 同 PM 真实事务） | `txn::tests::v20_s1_failure_touches_nothing`：构造 S1 安装失败，断言**任何 uninstall 都没有被执行**（npm 那份分毫未动）。顺序本身由 `txn::tests::cross_pm_migration_installs_then_uninstalls` 断言（先装后卸）。SRS 要求的"确认 dsh 仍完全可用"在**同 PM 的真实事务**里已实测：重装后 `dsh --version` 仍是 `0.1.6-alpha.2`、`where.exe dsh` 仍是同一对 shim，且整个重装过程发生在一个**正在运行的** `dsh web`（pid 13432）脚下而没有中断它；**跨 PM 顺序**（先装 pnpm 那份、再卸 npm 那份）仍只有单测。 |
| **V-21** TR-5：补偿前探测 | **PASS**（单测） | `txn::tests::v21_origin_broken_does_not_blindly_uninstall_target`：PM_old 已损坏 + S3 失败时**不会**盲目卸载 PM_new；`txn::tests::tr11_compensation_probes_before_acting` 另钉住"补偿动作前必须先探测"。 |
| **V-22** FR-25：双实例状态同步 | **部分执行 / 未完成** | **窗口 → 托盘**方向有观测：T19 的 V-10 在窗口驱动状态变化后读托盘菜单项 `IsEnabled`（运行中 ↔ 已停止两侧都读到了正确的启用关系）。**托盘 → 窗口**方向本轮**未执行**：锁屏会话下无法用 UIA 识别我们自己那个弹出菜单里的菜单项（详见「验证方法说明」第 4 条），因此没法可靠地选中 `启动 dsh web`。本轮实际观测到的托盘→UI 通路只有**可见性**那一条：点 `隐藏到托盘` 后窗口消失，posted `WM_TRAYICON`+`WM_LBUTTONUP` 后窗口重新可见。代码侧：`project()` 把同一份状态**同时**推给 `win` 与 `tray`（FR-25 存在的理由就是把两次赋值放在同一个函数里）。**待解锁桌面复验。** |

---

## 最终修复轮记录（I-1 ~ I-4、TR-2、M-2、M-4、M-5）

来源标记：**终修实测**（UIA 读回 + `netstat -ano` + `state.json` + GitHub 配额计数）。
端口纪律同前：**3080（pid 13432，承载本会话的 live `dsh web`）全程未触碰**，凡需要真实监听一律用 **3099**，
收尾时 `state.json` 恢复为**不存在**（本轮开始时它就不存在）。测试计数：`cargo build` **0 警告**，
`cargo test` **0 警告 / 84 passed**（70 → 84：新增 14 条，旧 70 条**一条未少**；唯一"消失"的那条是
被判为安慰剂的测试**改名重写**，见 M-2；全仓无任何 `#[allow]`/`#[expect]`）。

### I-1 `Job::StopWeb` 无法收敛"已经没了"的实例（`运行中` 的永久谎言）

| 项 | 内容 |
|---|---|
| 改动 | `src/main.rs` worker 的 `StopWeb` 臂：`own_pid` 为 `None`（= 外部实例，**没有任何存活监控**）时**先探测再动作**（TR-11 的同一条原则）—— `dsh::port_in_use(port)` 为假即视为"停止"的目标状态已经成立：清 `running_port`、发 `WebState::Stopped`、日志 `端口 {port} 上已无 dsh web，状态已清除`。`src/dsh.rs` 的 `stop_by_pid` 补上**语言无关**的**退出码 128** 判据，判据抽成纯函数 `taskkill_says_gone` 并单测。 |
| 实测（终修实测） | 外部 `dsh web --port 3099`（**不由本程序启动**，`--no-open` 以免弹浏览器）→ 管理器经 `state.json` 的 `running_port=3099` 认领（日志 `检测到外部 dsh web 运行在端口 3099（pid 19092）`）→ **out-of-band 杀掉那个 node**（3099 随即空闲）→ 界面**仍显示 `运行中`**（这就是 I-1 要修的谎言，因为外部实例没有 `WebExited` 通道）→ 点 `停止` → **422 ms 后 `已停止`**，日志新增 `端口 3099 上已无 dsh web，状态已清除`，`state.json` 的 `running_port` → `null`，**没有出现** `未找到监听端口 3099 的进程`。随后 `启动` 被正常接受并在 **2505 ms** 后就绪（真实 dsh web，pid 13040），**证明确实没有卡死**；再点 `停止` 433 ms 收敛、3099 空闲。全程 3080 = 13432 未变。 |
| `taskkill` 判据的本机实测（对 finding 的一处修正） | 把一个自己起的 sleeper 子进程 `taskkill /PID … /F`（退出码 **0**），紧接着对**同一个 pid** 再来一次：退出码 **128**、stderr = `ERROR: The process "11356" not found.`。两个要点：①**退出码 128 得到证实**，它正是 `stop_by_pid` 缺的那个判据；②但本机 taskkill 的错误文本是**英文 ASCII**，旧的 `"not found"` 文本匹配在本机**本来就能命中** —— 也就是说 finding 里"GBK `没有找到` 变成 U+FFFD 所以永远匹配不上"这一条在**本机不成立**（那是消息表为中文的 Windows 上的形状，本地无法复现，只能构造）。因此 128 判据的价值是"区域无关的兜底"，而不是"修好本机这个 bug"；单测把两种形状都钉住了（一种是实测，一种是构造）。 |
| 单测 | `dsh::tests::taskkill_says_gone_accepts_exit_code_128`：实测形状（英文 + 128）、构造的 GBK-lossy 形状（128）、纯文本旧判据（英文 + 退出码 1）、以及 `Access is denied`（退出码 5）**不许**被吞。 |
| 有意没做 | **80 ms tick 不做 `External → Stopped` 的自动收敛**。`port_in_use` 在空闲端口上并不便宜：300 ms 连接超时 + 一次 `netstat.exe`（约 40 ms）；12.5 Hz 地跑等于每秒起十几个子进程、持续唤醒 worker，与 NFR-4（空闲接近零）和 GC-16（UI 线程不阻塞）都相悖。用户手上的出口是 `停止`（实测 422 ms 收敛），也正是 SRS 给出的那个出口。 |

### I-2 `Starting` 窗口内 TR-1 无人值守

| 项 | 内容 |
|---|---|
| 改动 | ①`src/main.rs` 的 `on_install_clicked` 增加守卫：`s.start_pending \|\| matches!(s.web, WebState::Starting{..})` 时拒绝并写可见状态 `dsh web 正在启动，请稍候再执行变更`（+ `dirty`，否则稳态下没有消息可排空、提示投影不出来）。②`ui/app.slint` 新增 `in property <bool> web-starting`（由 `project()` 从 `start_pending \|\| Starting` 投影）：`安装` 与 `启动` 变灰，dsh web 卡片把 `已停止` 改成 `启动中…`（此前 `Starting` 期间卡片显示 `已停止` 本身就是假话）。**这是刻意的接口改动**：属性是新增的，生成 API 未破坏（`win.set_web_starting` 由 `project()` 调用）。 |
| 实测（终修实测） | 夹具：PATH 前置一个**永不绑定**的假 `dsh.exe`（应答 `--version`，`web --port` 只 sleep）+ 一个假 `npm.cmd`（应答 `--version`/`prefix -g`，任何安装请求都拒绝）→ 环境探测成功（`已安装 0.1.6-alpha.2`、下拉 `npm ·  dsh 安装于此`）→ 点 `启动`：**在同一拍的 80 ms 空隙内**（`project()` 还没把按钮变灰）立刻点 `安装`，结果 **140 ms 内**状态栏出现 `dsh web 正在启动，请稍候再执行变更` —— **守卫分支是被真实点击走到的，不是只靠读代码**；随后投影落地：`安装` `IsEnabled=False`、`启动` `IsEnabled=False`、卡片文本 `启动中…`、日志里 **`事务开始` 行数 = 0**（没有派发任何事务）。20 秒超时路径照旧可见（`启动超时` 在 19968 ms 出现），子进程被超时兜底收掉（无残留 `dsh` 进程），`state.json` 的 `running_port` 保持 `null`。 |
| 未覆盖 | **托盘菜单**没有 `web-starting` 属性（见「已知限制」第 2 条）：托盘里的 `启动` 在启动窗口内仍可点，但被同一道 `start_pending` 守卫拒绝。 |

### I-3 FR-3 第 3 步（PATH 找不到时的退化路径）此前根本没实现

| 项 | 内容 |
|---|---|
| 改动 | `src/pm.rs`：新增纯函数 `parse_global_list(pm, output) -> Option<Version>`（**不写 per-PM 解析器**：只认"同字段 `pkg@ver`"与"相邻字段 `pkg ver`"两种形态，版本号一律交给 `semver::Version::parse`）与 `version_in_global_list(pm)`（走 `pm::run_cmd`：shim 全名 + 参数数组 + `CREATE_NO_WINDOW` 都在既有 helper 里；**刻意不看退出码** —— `npm ls -g` 有 peer 依赖告警时会非零退出而 stdout 仍完整）。`src/model.rs` 新增 `Pm::list_args()`（四条命令，由 `pm_command_table_is_exact` 逐字钉住）。`probe_env` **只在 `find_dsh_on_path` 一无所获时**才依次问各 PM，命中即取其版本为 `installed`、该 PM 为 `owner`；`dsh_path` **保持 `None`**（它确实不在 PATH 上，不能伪造 → TR-3 随后照样拒绝事务，这是正确的）。 |
| 实测（终修实测） | **退化路径**：把 `%APPDATA%\npm`（dsh shim 所在目录）从 PATH 去掉 → `where.exe dsh` 报 `Could not find files for the given pattern(s).` → 界面仍读到 `已安装 0.1.6-alpha.2`、`alpha 通道`、下拉 `npm  ·  dsh 安装于此`；关于对话框的 `dsh 安装路径` 为**空**（`dsh_path` 确实是 `None`，没有被伪造）。**正常路径未回归**：恢复常规 PATH 后同样读出 `0.1.6-alpha.2` + npm，且 `dsh 安装路径 = C:\Users\xueyu\AppData\Roaming\npm\dsh.cmd`。 |
| 实测的真实输出形状（本机 2026-09-20，路径已替换） | **npm** `npm ls -g --depth=0`（退出码 0）：首行是全局目录 `C:\Users\<user>\AppData\Roaming\npm`，之后每行 `+-- pkg@ver`，最后一行是 `` `-- npm@12.0.2 `` —— 树状前缀是 **ASCII**（输出被重定向时不用 Unicode）。**pnpm** `pnpm list -g --depth=0`（退出码 0）：`Legend: production dependency, optional only, dev only` → 空行 → `C:\Users\<user>\AppData\Local\pnpm\global\v11 (PRIVATE)` → `│` → `│   dependencies:` → `├── pkg@ver`（**Unicode** 树状前缀）。**bun / yarn 本机未安装**（`where.exe` 报 not found），fixture 用的是各自公开的 `pm ls -g` / `global list` 形状（前者路径行带 `node_modules (N)`、条目 `├── pkg@ver`；后者有 `info "…" has binaries`、`warning … has unmet peer dependency`、`└─ pkg@ver`、`Done in …`）—— 与「已知限制」第 7 条同一性质，**未经真实运行**。 |
| 单测 | 7 条 `parse_global_list_*`（npm/pnpm/pnpm 两字段/bun/yarn/无 dsh/整字段匹配的判别性用例）+ `list_args_run_for_installed_pms`（对**本机确实装了**的 PM 断言列表命令真的打得出 stdout；未安装的 PM 跳过）。`parse_global_list_requires_whole_field_match` 是判别性的：用 `contains("dsh")` 的实现会把 fixture 里的路径行当包名，再把别的包的 `9.9.9-bogus` 当成 dsh 的版本。 |

**覆盖账目的缺口（本条 finding 的来历）**：计划的覆盖表把 FR-3 整体记在 **Task 6** 名下
（`docs/IMPLEMENTATION-PLAN.md` 的「Spec 覆盖检查」），而 Task 6 的接口清单只含第 1~2 步
（`find_dsh_on_path` / `owner_of`）—— 第 3 步（全局包列表退化）**从未有任务承接**，
所以它在 70 条单测全绿的情况下依然没实现。本节记的是补上的实现与实测。

### I-4 一次下拉浏览会给串行 worker 灌进 N 个阻塞式 GitHub 请求

| 项 | 内容 |
|---|---|
| 改动 | `src/main.rs` 的 `spawn_worker`：worker 出队一个 `Job::FetchNotes` 时，用 `try_recv` 把**此刻已排队**的任务一次取空，交给纯函数 `model::coalesce_notes` 只保留**最后一个** `FetchNotes`，其余任务按原序放回队首逐个执行。保序保证（已写进 `coalesce_notes` 的文档与单测）：结果是原队列的**子序列** —— 除"已被更晚的同类请求取代"的 `FetchNotes` 外**一个任务都不丢**，相对顺序不变（存活的说明请求就是**最后一次选择**那个，`coalesce_notes` 的"保留末位"因此要求交进去的批次按入队顺序＝旧→新排列：已出队的旧任务必须插到批次**队首**，`drained.push(job)` 会让最旧的那个占据末位）。丢掉是安全的：`drain` 只采信**当前选中版本**的回复，被取代的请求即使回来也会被丢弃 —— 它们唯一的作用就是让 worker 再阻塞最长 15 秒并多烧一次 GitHub 配额。 |
| 实测（终修实测，A/B） | 判别依据是 **GitHub 自己的配额计数器**（未认证 60/小时，两个 `rate_limit` 探测之间的差值 − 1 = 应用真正发出的请求数），配合 6 次快速版本切换（UIA 聚焦下拉 + `SendKeys` 六次 `{DOWN}`，实测选中项确实移动了 6 步：`0.1.6-alpha.2 (alpha) ← 当前` → `0.1.3-alpha.2 (alpha)`）。**保序版**：60 → 58 ⇒ **应用只发了 1 个请求**。**对照版**（把合并块摘掉后另编的 `target/debug/dsh-manager-nofix.exe`，源码随即按 blob 哈希还原）：58 → 52 ⇒ **应用发了 5 个请求**。同一台机器、同一个夹具、同样的 6 次选择：**1 vs 5**。 |
| ⚠ 本行的历史注记 | 上表这次实测的"保序版"当时读回说明区仍是 `更新说明 · 加载中…`，**被解读为"那一个请求还在飞" —— 这个解读是错的**。后续评审查明：该版本把**最旧**的选择留在了批次末位（`drained.push(job)`），其回复会被 `version == selected_version` 过滤丢弃，故面板其实**永久卡住**。`0f2cb9b` 改为 `drained.insert(0, job)`，并补上**正/红相位对照**：保序版派发**最后一次**选择、`REPLY accepted=true`、标题 2.0 秒离开加载中并稳定 70 秒、正文 13630 字符与同一 tag 的 release body（13817，去 HTML 后 13630）吻合；把那一行翻回去的对照版派发的是**被放弃**的选择、`accepted=false`、70 秒后仍"加载中"。 |
| 单测 | `model::tests::coalesce_notes_keeps_only_the_last_fetch`（3 个请求 → 只剩最后一个）、`coalesce_notes_preserves_every_other_job_in_order`（6 个任务 → 4 个，且结果是原队列的子序列）、`coalesce_notes_is_identity_without_fetch_notes`。 |

### TR-2：`precheck` 与 `probe_pm` 对"PM 可用"必须同判据

`src/txn.rs` 的 `precheck` 改为 `Ok(out) if out.code == 0`（此前只判 `is_err()`）。判别性单测
`txn::tests::tr2_precheck_rejects_pm_whose_version_command_exits_nonzero`：`FakeBackend` 新增
`fail_version` 开关（`--version` 命令跑得起来但退出码非零）—— 退回旧实现该测试即变红。

### M-2：安慰剂测试与过头的记录

`precheck_rejects_unsafe_version` 断言的是同一个合法版本两次（其注释自己承认那条分支不可达），却被
`docs/VERIFICATION.md` 的 V-16 引为"NFR-6 逐条覆盖"。现改为
`precheck_invalid_version_branch_is_unreachable_by_type`：断言**合法版本必然通过**，并钉住
"`Version` 的字符串形式必然满足 `is_safe_version`"（若有人把字符集校验改严，这条会红）；注释写明
NFR-6 的真实覆盖在 `safe_version_rejects_injection_attempts`、`is_safe_version` 与 `InvalidVersion`
按 Ruling 39 保留为具名实现。V-16 行已同步改写（见上）。

### M-4：管道日志流不能被第一个坏字节掐断

`src/dsh.rs` 的 `spawn_reader`：原为 `map_while(Result::ok)`，它在**第一个**读错误处结束整个循环，
于是一行非 UTF-8 字节就会让该管道此后的全部日志消失（本程序 GC-8 没有 stderr，日志面板是唯一出口）。
**最终实现是一段显式三分支（不是简单的 `filter_map`）**：`Ok(0)`（EOF）照常结束循环；
`Err(e) if e.kind() == InvalidData`（一行里混了非 UTF-8 字节）**只丢掉那一行**并继续 ——
`read_until` 此时缓冲区已前移，跳过坏行是安全的；其余读错误 `break`，这也**恢复**了 `map_while`
原有的"任何错误即结束该管道"语义，避免在持续性错误上自旋。EOF 仍由读操作的 0 长度表示。

### M-5：`strip_html` 静默吞掉未闭合 `<` 之后的内容

`src/dsh.rs` 的 `strip_html` 重写为"只有本行内存在配对 `>` 的 `<` 才算标签"：未配对的 `<` 连同本行剩余
内容按正文保留（`支持 <1s 启动` 此前渲染成 `支持 `）。判别性单测
`strip_html_keeps_text_after_unmatched_angle_bracket`（含 `a < b`、`<未闭合`、以及"真标签 + 未配对 `<`"
的混合行）；既有的 `strip_html_removes_tags_but_keeps_inner_text` 与 `preprocess_*` 全部照旧通过。

### 规格漂移（spec drift）

以下是**已交付代码刻意不按 SRS 字面**的地方。`docs/SRS.md` 不改（规格变更属于人的决定），此处只登记
漂移与当时据以裁决的理由：

| # | SRS 字面 | 实际实现 | 裁决理由 |
|---|---|---|---|
| 1 | FR-20：`cmd /c start "" <url>`（+ `CREATE_NO_WINDOW`） | `explorer.exe <url>` + **http(s) 协议白名单**（`dsh::open_url`） | URL 来自**网络**（release notes 正文里的链接），属信任边界；Rust 的参数编码**不**转义 `& \| ^ < > % !`，交给 `cmd` 解析就是 shell 注入向量（`https://a/&calc.exe` 会被拆成两条命令）。`explorer.exe` 单参数没有 shell 层，白名单是第二道防线。 |
| 2 | §4.2.3 C3：降级时"输出**两条**可复制的手动命令" | 同 PM（`origin.pm == target.pm`）时**只给重装那一条**（`txn::manual_commands`） | 两条命令作用于同一个包：照做会先装回 origin 再把它删掉，结果是"一份都没有" —— 正是 TR-5/TR-6 要避免的最坏结果。单测 `manual_commands_same_pm_never_removes_the_only_copy` 钉住。 |
| 3 | §3.8 文档的 JSON 形状 `{"running": {"port": …}}` | 平铺的 `running_port`（`config::StateFile` / `state.json`） | FR-31 的语义只需要一个端口号，嵌套对象不承载任何额外信息；SRS 自己的 §8.2 V-24 行与全部实现片段用的都是 `running_port`，本记录也一直按后者取证。 |
| 4 | FR-31："**唯一写入点约束（强制）**：写入与清除只允许发生在一处" | 写入/清除分布在 `dsh web` 生命周期的若干站点（就绪后、各停止路径、启动超时兜底、退出前、启动时的过期清除、事务中的 TR-1 停止与事务后重启） | "唯一写入点"的**目的**是让记录与实际运行状态不脱节。裁决多轮下来，收敛到单一函数反而会制造脱节：只有**确知已停**才能清（评审轮 1）、队列未完成时必须记下**接受启动时**那个端口（Ruling 93）、启动超时且杀不掉子进程时必须补记端口（评审轮 4）。因此纪律改成"每个站点都必须与一个真实生命周期事件一一对应"，全部站点都在 `src/main.rs` 的启停/退出路径内。 |
| 5 | FR-4："已安装版本一律通过执行 `dsh --version` 获取…**不得**解析各 PM 的全局列表输出格式来读取版本" | 退化路径（`dsh` 不在 PATH）下由 `parse_global_list` 从全局列表里取版本 | I-3 的修复要求。此时 `dsh --version` **根本无从执行**（没有 shim 可跑），而 FR-3 第 3 步本来就要求读全局列表来定 owner；若因此把 `installed` 留空，界面会对一个"明明装了"的用户报 `未检测到已安装的 dsh` —— 正是 SRS:786 禁止的未探测先下结论。正常路径（PATH 命中）仍**只**用 `dsh --version`，FR-4 的约束在那里完整成立。 |
---

## 真实事务实测（V-14 / V-15 的同 PM 路径）

来源标记：**真实事务实测** —— 本机 UI Automation 读回 + `netstat -ano` + 文件 mtime + 一个与夹具
无关的独立端口采样器。夹具与观测脚本在 gitignore 的 `target/realtxn/` 下
（`run-txn2.ps1` / `run-txn2.txt` / `before.txt` / `after.txt` / `port-watch.txt`）。
**这是本分支唯一一次真实的、会改环境的事务**，也是此前 V-14/V-15/V-18 一直缺的那条端到端证据。

| 项 | 内容 |
|---|---|
| 安全包线（为什么必须先改端口框） | `Job::Transact` 会按 **TR-1** 先停掉"它拿到的那个端口"上的 `dsh web`，端口取自 `state.web.port().unwrap_or(state.preferred_port)`（`src/main.rs` 的 `on_install_clicked`）。本机 **3080** 上跑的正是**承载本会话的 live `dsh web`（pid 13432）**，所以动手前先把端口框改成 **3099**：3099 空闲 ⇒ `dsh::port_in_use(3099)` 为假 ⇒ TR-1 的预停分支根本不进入，3080 从头到尾不在候选集里。 |
| 允许的唯一环境改动 | **同一个 PM（npm）重装同一个版本** `@deepseek-ai/dsh@0.1.6-alpha.2`。同 PM ⇒ `txn::run` 的 **S3 被跳过（FR-13）** ⇒ 全程**没有一条卸载命令**，不存在"两份都毁了"的窗口；也没有切换 PM、没有装别的版本。 |
| 动手前 | `npm ls -g --depth=0` → `+-- @deepseek-ai/dsh@0.1.6-alpha.2`（其余 6 个全局包同前）；`where.exe dsh` → `C:\Users\xueyu\AppData\Roaming\npm\dsh`、`…\dsh.cmd`；`netstat` **3080 → pid 13432**，3099 → 空闲；`state.json` → **不存在**。 |
| 点击前的界面读数（UIA） | 端口框 `3099`（先由 `state.json` 的 `preferred_port` 落地，再用**真实按键** `^a` + `3099` 改一次并读回，防止"字段其实指在 3080 上"这种致命误配）、PM 下拉 `npm  ·  dsh 安装于此`、版本下拉 `0.1.6-alpha.2  (alpha)  ← 当前`、`安装` 按钮 `IsEnabled=True`。三者同时成立才允许点击（硬闸门；不成立即中止且零副作用）。 |
| 日志面板实录（UIA 读回；时间为相对点击的时刻） | `+0.30s` `事务开始：npm 0.1.6-alpha.2 → npm 0.1.6-alpha.2`；`+0.66s` 状态栏 `正在安装新版本…`（= `TxProgress(S1Install)`）；`+1.72s` **`$ npm.cmd install -g @deepseek-ai/dsh@0.1.6-alpha.2`**（FR-28 的"执行的命令原文"）；`+186.57s` `事务结束，重新探测环境`。日志面板最终 4 行（另有启动行 `DSH Manager 启动`）。 |
| 最终状态 | **`Committed`** —— 状态栏 `已安装 0.1.6-alpha.2（npm）`，即 `describe_outcome(TxOutcome::Committed { pm: npm, version: 0.1.6-alpha.2 })` 的唯一形状。点击后 **186.6 秒**收敛（npm 真的要下载/重写整包，见下）。 |
| S2 / S4 的可见证据 | **日志里没有**，这是设计使然：`txn::install` 打命令原文，**S2/S4 不打日志**，成功路径也不回灌 PM 的 stdout（`describe_outcome_log(Committed) = vec![]`，只有 `Degraded`/`RolledBack`/`Rejected` 才写行）。所以"S2、S4 都通过"是由**两个否定证据**共同推出的：日志里**没有** `在「安装新版本」阶段失败`、**没有** `在「验证新安装」阶段失败`、**没有** `在「最终验证」阶段失败`、**没有** `已完成回滚`（S2/S4 任一不符都会走 `compensate`，`RolledBack` 必打两行），且终态是 `Committed`。想让 S2/S4 有**直接**读数，得给引擎补日志——本轮没有改代码。 |
| 这次确实是重装，不是 npm 的 `up to date` 空转 | `…\npm\node_modules\@deepseek-ai\dsh\package.json` 的 mtime 从 **09:43:22** 变为 **22:57:40**，`README*`/`LICENSE`/`lib/` 一并重写、`node_modules/` 到 22:58:03 —— 与 185 秒的耗时吻合。**关键事实**：整个重写发生在**那个 live `dsh web`（pid 13432）正在运行**的时候，Windows 允许替换（node/libuv 以共享删除方式打开文件），进程没有中断。 |
| 动手后 | `npm ls -g --depth=0` → 仍是 `+-- @deepseek-ai/dsh@0.1.6-alpha.2`；`where.exe dsh` → 同一对 shim；`dsh --version` → `0.1.6-alpha.2`；`netstat` **3080 → pid 13432（未变）**；3099 与 8080 → **无本地监听**；**无 `dsh-manager.exe` 进程**残留；`state.json` **已恢复为不存在**（本轮开始时它就不存在）。 |
| 3080 的连续性（独立观察器） | 一个与夹具无关的 `netstat` 采样循环在 **22:52:46 → 22:58:38** 采样 **155 次**：`3080=13432` **155/155**、`3099=`（空）**155/155**。事务那 185 秒的前后都被覆盖，不存在"短暂断开又回来"的可能。 |
| 顺带观测到的 V-18 片段 | 事务进行中 `安装` 按钮 `IsEnabled=False`（`busy` 经 `project()` 投影成 `enabled: !root.busy`，**实测生效**，不只是读代码）；日志新增行**在事务运行期间**就能从面板读回（不是结束后一次性出现）；每 250 ms 一次的 UIA 读回在 185 秒里**没有一次超时或失败**，即 UI 线程确实没有被事务阻塞（GC-16）。 |
| 有意**未**执行：跨 PM 迁移（V-15 的 npm → pnpm、V-16 的迁移路径） | **没跑，且不该跑**：迁移要求 `npm → pnpm`，而 TR-1 会先停掉 3080 上那个 `dsh web`（= **承载本会话的进程**），随后 `npm uninstall -g @deepseek-ai/dsh` 会删掉用户机器上**唯一**一份 dsh CLI —— 正是本会话依赖的 CLI。半途失败会同时毁掉用户环境与这次会话。因此迁移语义仍然只由 **20 个事务单测**（TR-4 / TR-5 / TR-6 / TR-11 及 V-17 / V-19 / V-20 / V-21）钉住；这次真实运行补上的是**公共骨架**（`precheck` → S1 → S2 →【S3 跳过】→ S4 → 终态）在真实 PM/真实网络下的端到端行为，**S3 与迁移分支本身依然没有被任何人真实运行过**。 |
| 方法学注记（给下一位用 UIA 驱动的验证者） | 主窗口的 UIA 子树里，**标题栏的 `关闭` 按钮排在任何自绘内容之前**。第一轮尝试在读完"关于"卡片后用 `Get-ByName '关闭'` 去关它，命中的是**标题栏关闭**，于是程序按正常路径退出（窗口关 = 退出），那一轮因此**没有点到 `安装`**（零副作用，3080 依然 13432）。教训：按名字找控件在 Slint 窗口上不安全，先按 `ControlType` + 矩形位置筛选，或干脆不去点"关于"。 |

---

## 未通过项与处理

**没有失败项。** 以下是**未执行**项及其处置，逐条列出以便追溯（不留空、不跳过）：

| 编号 | 现象 | 处理 |
|---|---|---|
| V-14 换版本事务 | **已部分执行**（原来是"未执行"）：同 PM 的真实 `npm install -g @deepseek-ai/dsh@0.1.6-alpha.2` 已在 GUI 上端到端跑到 `Committed`（185 秒、真实重写文件、3080 未动） | **仍未执行**：真正的"换版本"（降级到 `0.1.6-alpha.1`）—— 那会降级用户正在使用的 dsh 安装（本机唯一那份，也是本会话依赖的 CLI）。单测另钉住：`same_pm_version_change_commits_and_skips_uninstall`、`pm_command_table_is_exact`。细节与全部读数见「真实事务实测」一节 |
| V-15 迁移事务 | **未执行**：控制器裁定（TR-1 会先停掉承载本次会话的 `dsh web`，随后 `npm uninstall` 掉 dsh 本身）。本次真实事务只走**同 PM 分支**，没有触及迁移路径 | 20 个事务单测 + 同 PM 真实运行覆盖的公共骨架；**迁移分支（S3 卸载）依然零真实运行** |
| V-16 前置检查 | 部分执行：拒绝路径未在真实事务里走到 S1；原先连 GUI 级都无法构造。**本次真实事务观测到了前置检查的通过路径**（`precheck` 返回 `None` ⇒ 事务进入 S1），拒绝路径仍只有 GUI 级构造（I-3 的 TR-3） | **最终修复轮改变了"能不能构造"**：I-3 的 FR-3 第 3 步退化路径让"PM 的 bin 目录不在 PATH"可构造，TR-3 的拒绝已在 GUI 级实测（零副作用）；NFR-6 的真实覆盖是 `safe_version_rejects_injection_attempts`，`InvalidVersion` 分支按类型不可达（M-2）。细节见 V-16 行与 I-3 一节 |
| V-17 失败补偿 | 未执行：需要一次真实的 S3 失败（= 动用户的安装） | 5 个补偿单测覆盖回滚/降级/手动命令；本次真实事务**成功路径**没有触发任何补偿 |
| V-18 事务期间 UI | **已部分执行**（原来是"未执行"）：本次真实事务进行中读回 `安装` `IsEnabled=False`、日志行在运行期间实时出现、每 250 ms 一次的 UIA 读回 185 秒无一次失败 | 仍需补的是"界面不卡死"的**强**证据（例如手动拖动/交互）与托盘侧在事务期间的表现；细节见「真实事务实测」一节 |
| V-22 双实例状态同步 | 部分执行：托盘 → 窗口（状态）方向未执行（锁屏桌面无法识别弹出菜单项） | 窗口 → 托盘方向由 T19 的 V-10 覆盖；托盘 → UI 的可见性通路本轮实测；**待解锁桌面复验** |
| V-10 托盘菜单状态 | 本轮未重复执行（T19 已 PASS） | 本轮试图复读菜单项时 UIA 返回 0 个 `MenuItem`，方法学记录见下 |

---

## 已知限制与环境假设

1. ~~**代理只认环境变量**~~ → **已解决（2026-09-22，Ruling 89 修订 · SRS FR-33）**。
   *原记载（保留以说明来龙去脉）*：`ureq` 的默认 feature 集**不做系统代理发现**，只读 `HTTP_PROXY` / `HTTPS_PROXY`；
   GC-2 禁止为此引入读注册表 / WinINET 的依赖。因此两个出网函数（`dsh::fetch_catalog` / `dsh::fetch_notes`）
   的错误串尾部都挂了 `PROXY_HINT`（"若本机仅允许通过系统代理出网，请设置 HTTPS_PROXY 后重启本程序"）。
   本机实测就撞上过这个组合：直连 `api.github.com` 得 HTTP 403（出口 IP 配额），而系统代理返回 200 ——
   即**只有本程序会失败**，所以失败信息必须自己解释原因。
   **现在的行为**：程序自己读系统代理（`dsh::system_proxy()`，走 `windows-sys` **已启用**的
   `Win32_System_Registry`，**未新增依赖、未改 `Cargo.toml`** —— 当初"被 GC-2 挡住"的只有 `winreg` 那条路）。
   设置面板新增「网络代理」一档：**使用系统代理（缺省）/ 直连**，优先级 **环境变量 > 系统代理 > 直连**，
   所以本文件里 V-6 那条注入 `HTTPS_PROXY` 的验证路径**行为不变**。`PROXY_HINT` 保留，
   只是改成同时指向设置面板与 `HTTPS_PROXY`（关掉这一档、或目标机器没配系统代理时，它仍是唯一的解释）。
   实测：`system_proxy()` 读出 `http://127.0.0.1:12450/`，与注册表 `ProxyServer` 逐字一致。
   **仍未做**：`ProxyOverride` 绕过列表（上限见 FR-33）。**待人工补验**：SRS §8.2 的 **V-27**（其第 ② 步为判别性步骤）。
2. **托盘的 `Starting` 态不渲染**：`ui/app.slint` 的 `AppTray` 没有 `web-starting` 属性，托盘菜单的启用
   条件只看 `web-running`/`busy` → 从点击到就绪（正常约 2 s，超时路径最长 20 s）**托盘里的"启动"仍可点**。
   守卫（`start_pending` + 状态检查）会拒绝并写一条日志/状态栏（"已在启动或运行中，忽略重复的启动请求"），
   所以不会造出第二个子进程 —— 但托盘用户看到的是"点得动、被拒绝"，不是菜单项变灰。
   （**主窗口**自最终修复轮起有 `web-starting`：`安装`/`启动` 变灰、卡片显示"启动中…"，见 I-2。）
3. **被采用的外部实例没有存活监控**：孤儿恢复认领的（以及预检发现的外部）实例只能靠用户点 `停止` 或它自己退出；
   `dsh::spawn_web` 的三个监督线程只属于**本程序自己启动**的实例，外部实例没有对应的 `WebExited` 通知，
   它的状态因此可能**显示**为 `运行中` 直到用户操作。**最终修复轮（I-1）补上了出口**：点 `停止` 时先按端口探测，
   没有监听者即视为已达成"停止"（清 `running_port` + `已停止` + 日志 `端口 N 上已无 dsh web，状态已清除`），
   实测 422 ms 收敛；80 ms tick **刻意不做**自动收敛（`port_in_use` 在空闲端口上要 300 ms 连接超时 + 一次
   `netstat.exe`，12.5 Hz 地跑与 NFR-4 相悖）。
4. **UI 最多滞后一个 tick**：所有 `WebState` 驱动的界面变化要等下一次 80 ms 排空（架构决策 3），
   因此状态显示与 worker 的实际动作之间最多差一帧。
5. **`wait_port_ready` 的 `alive` 回调未被使用**：`main.rs` 传的是 `|| true`，所以即便子进程立刻死掉，
   也要**等满 20 秒超时**才会报"启动超时"（随后走 `stop_by_pid` + 补记端口的那条兜底）。回调本身是设计好的
   （`!alive()` 会提前返回 false），只是调用方没有接线。
6. **日志是唯一出口**：本进程是 `windows_subsystem = "windows"`（GC-8），`eprintln!` 在部署形态下是**空操作**
   —— 所有诊断信息（含 FR-32 的损坏提示）都必须经 `push_log` 进日志面板才有意义。
7. **bun / yarn 未实测**：本机未安装这两个 PM，命令表条目（`bun.exe pm bin -g`、`yarn global add` 等）
   仅由 `model::tests::pm_command_table_is_exact` 钉住字面量，未经真实运行（SRS §8.4 已记录）。

---

## 验证方法说明（给下一位验证者）

1. **假的 `taskkill.exe` / `netstat.exe` 在这台机器上永远赢不了**：Windows 解析可执行文件时，
   System32 **先于** PATH，也**先于**子进程自己的目录。因此"往 PATH/dll 目录里塞一个同名假程序来观测调用"
   这类测试缝在本机是**死的** —— 别把时间花在这上面；要观测这些调用，只能用 UIA / `netstat` / 进程表
   这类**外部**证据（本记录全部采用后者）。
2. **托盘是 message-only 窗口**：`EnumWindows` 列不出它，必须 `FindWindowEx(HWND_MESSAGE, …, "SlintSystemTrayWindow")`。
   它的 Win32 菜单**不能被 UIA `InvokePattern` 驱动**（Slint 用 `TrackPopupMenu(TPM_RETURNCMD)`，
   所以外部 posted 的 `WM_COMMAND` 会被忽略）→ 托盘交互要用 **posted `WM_TRAYICON` / `WM_LBUTTONUP`**，
   菜单项用 **posted 方向键 + Enter**。本轮另发现一处细节：Slint **自带**托盘实现
   （`i-slint-core-1.18.0/items/system_tray/windows.rs`），其 `WM_TRAYICON = WM_APP + 1 = 0x8001`
   且鼠标事件在 **lParam 低 16 位**，菜单命令 id = `0x100 + 菜单项下标` —— 而**不是** `tray-icon` crate
   的 `6002`（post 6002 无任何反应；换成 0x8001 后 `#32768` 菜单窗口立即出现）。下一步若要自动化托盘菜单，
   从 `0x8001` 出发。
3. **锁屏会话**：`SendInput` 会被送到锁屏界面，屏幕截图会返回**陈旧帧**（本轮据报告甚至出现过一帧
   根本不曾发生的 BSOD 画面）。因此**UIA 读回才是可靠仪器**；任何依赖物理输入或像素的结论都必须在
   **解锁的桌面**上重做一遍。
4. **本轮托盘菜单项读不回来（与 T19 的差异）**：即使 `#32768` 菜单窗口确实为我们的 pid 出现了，
   对 UIA 根做 `MenuItem` 搜索时只返回 VS Code 的菜单栏（`File`/`Edit`/…），**我们那个弹出菜单暴露 0 个
   `MenuItem`**；posted `VK_DOWN` + `VK_RETURN` 也没有产生可观测的状态变化。T19 曾读到过菜单项的
   `IsEnabled`，因此这很可能是**锁屏会话**造成的差异，而不是代码回归 —— 请在解锁桌面上复验 V-10 / V-22。
5. **V-25 的可复现配方**（本轮实际使用的步骤，端口可换成任意空闲端口）：
   1. 写 `%APPDATA%\dsh-manager\state.json` = `{"preferred_port": 3099, "running_port": null}`，启动 `dsh-manager.exe`，
      UIA 确认端口字段读回 `3099`，点 `启动`，等 UIA 出现 `运行中`；
   2. `netstat -ano | findstr :3099` 记下 LISTENING 的 node pid；`state.json` 应含 `running_port: 3099`；
   3. `Stop-Process -Name dsh-manager -Force`（等价 `taskkill /F`；**Git Bash 会把 `/F` 当路径，用 pwsh**）；
   4. 再查 `netstat`：监听仍在（孤儿存活），`running_port` 仍在（恢复信号存活）；
   5. 重新启动 `dsh-manager.exe`，UIA 日志面板应出现 `上次的运行端口为 3099，探测中…` 与
      `检测到外部 dsh web 运行在端口 3099（pid <上一步的 pid>）`，状态 `运行中`；
   6. 点 `停止` → `已停止`、端口空闲、`running_port: null`。
   收尾：结束 `dsh-manager.exe`，确认 3099/8080 空闲、3080 仍是原 pid、`state.json` 恢复原状。
6. **UIA 读回的形状**（本轮实测）：日志面板的每一行是一个 `Text` 元素（`Name` 即行内容），
   状态栏也是 `Text`；端口是 `[Edit]` 的 `ValuePattern`；按钮用 `InvokePattern`。窗口名 `DSH Manager`
   （类名 `Window Class`）。`StyledText`（更新说明正文）**没有** UIA 表示。
7. **最终修复轮的两条可复现配方**（夹具是临时物，已随本轮清理；照下面重建即可）：
   - **I-1**（外部实例自然退出后点 `停止`）：先 `node …\@deepseek-ai\dsh\lib\bin.js web --port 3099 --no-open`
     起一个**不由本程序启动**的实例（`--no-open` 免得弹浏览器），写 `state.json` =
     `{"preferred_port": 3099, "running_port": 3099}` 让管理器在启动时认领它（日志 `检测到外部 dsh web …`），
     然后 `Stop-Process` 掉那个 node → 界面会**仍显示 `运行中`**（外部实例没有 `WebExited` 通道）→
     点 `停止`：修复后 422 ms 内 `已停止` + 日志 `端口 3099 上已无 dsh web，状态已清除`。
   - **I-2**（`Starting` 窗口里的 TR-1）：PATH 前置一个假目录，内含**永不绑定**的 `dsh.exe`
     （`--version` 打印版本；参数含 `web` 就 sleep）与假 `npm.cmd`（`--version` → `12.0.2`、
     `prefix -g` → **打印该假目录自身**，于是 owner 判定成立；其余参数一律 `exit /b 1`，
     万一有事务漏出去也绝不会碰真实环境）。点 `启动`，**在同一拍的 80 ms 内**点 `安装` ——
     状态栏会出现 `dsh web 正在启动，请稍候再执行变更`；随后按钮变灰、卡片显示 `启动中…`、
     日志里不应出现 `事务开始`。用 `SendKeys` 连按 `{DOWN}` 驱动版本下拉，是 I-4 的输入手法
     （需要 `SetForegroundWindow` + UIA `SetFocus`；本会话桌面**未锁屏**，故可用）。

---
---

# 主题系统验证记录（`feat/theme-system`，Task 9）

日期 **2026-09-22**。被验证的代码：`feat/theme-system` 的工作树（Task 1–8 已合入，
本轮改动：`src/theme.rs` 删 6 处 `#[allow(dead_code)]`、`ui/app.slint` 两处注释、
`docs/` 三份文档）。基线声明为 **109 tests / 0 warnings**，本轮实测**复现**了该基线。

⚠ 本记录**与上面的 `feat/implementation` 记录互不相干**，不要混读。
上面的端口纪律那一套（3080 是 live server、观测一律用 3099）在本轮**同样成立**。

**证据来源图例（本轮）**

| 标记 | 含义 |
|---|---|
| **单测** | `cargo test` 中的具名单元测试（本轮 **109 passed**） |
| **探针实测** | gitignored 的 `target/theme-probe/`：**编译真实的 `ui/app.slint`**、用 Slint 软件渲染器离屏渲染、直接读像素。原始输出留档于 `target/theme-probe-out.txt`（gitignored） |
| **真机实测** | 启动**真实** `target/debug/dsh-manager.exe`，只用进程表 / 端口表 / 文件哈希观测，**不点击任何控件**，观测完即杀 |
| **未执行/待人工** | **没有做，也没有代偿证据** —— 不得读成 PASS。逐条列在下面 |

**本会话的环境约束（这是本轮验证方法被改写的原因）**

| 项 | 值 |
|---|---|
| 桌面状态 | **已锁屏**：`GetForegroundWindow() == 0`、`LogonUI.exe` 在跑（Ruling 23，本轮复核一致） |
| 后果 | `PrintWindow` + `PW_RENDERFULLCONTENT` 返回**全白位图**，且明暗两档的 **md5 完全相同** —— 一种"看起来成功"的失败；`CopyFromScreen` 截到的是锁屏桌面 |
| 处置 | **本轮不产出任何截图**，改用离屏探针（证据更强）；屏幕上长什么样留作人工项 |
| `dsh web` | 3080 监听 pid **23524**，全程未变（本轮每次观测前后都核对） |
| `state.json` | `%APPDATA%\dsh-manager\state.json`，启动前/恢复后 sha256 均为 `c83231f3546f5c223673a08b81e0c8a51503025548efbf13d710431a9c126a4a`（80 字节） |

⚠ **上述环境约束在当天的终审修复轮里不再成立**（桌面**已解锁**：`GetForegroundWindow() != 0`、
无 `LogonUI.exe`）。那一轮补做了真机点击与真机截图，**H-1 / H-2 / H-4 已据此关闭**，见 §2.5。
本节原样保留 —— 它记录的是 §2.1~§2.4 那些证据**当时**的生成条件。

---

## 1. 过渡期抑制删除（Step 1）

`#[allow(dead_code)]` 是 Task 1/5 为了让"尚未被消费的项"不破坏 0 警告规则而加的过渡脚手架。
到 Task 9 三批消费者（Task 2 / 6 / 8）都已落地，本步把它们全部删除。

| 检查 | 命令 | 结果 |
|---|---|---|
| 删除**前** | `grep -n "allow(dead_code)" src/theme.rs` | **6 处**：`:26`（`ThemeMode`）、`:41`（`impl ThemeMode`）、`:78`（`resolve`）、`:95`（`PERSONALIZE_KEY`）、`:116`（`system_dark` Windows）、`:228`（`system_dark` 非 Windows 桩） |
| 删除**后** | `grep -n "allow(dead_code)" src/theme.rs` | **无输出，grep 退出码 1** ✅（契约注释里不含该字面量，故"零匹配"就是"零生效属性"） |
| 构建 | `touch src/theme.rs ui/app.slint && cargo build 2>&1 \| grep -c "^warning"` | **0** ✅（强制重编译，非缓存命中） |
| 死代码 | `grep -c "dead_code" <build 输出>` | **0** ✅ —— **没有真实死代码浮出来**，因此不存在"删代码 vs 恢复抑制"的抉择 |
| 全量测试 | `cargo test` | **109 passed; 0 failed; 0 ignored**；输出中 `^warning` 计数 **0** ✅ |

⚠ 计划 Task 9 brief 写的是"实测当前为 **5** 处（`:21` `:36` `:73` `:97` `:188`）"，**实际是 6 处**
（brief 的列表漏了非 Windows 的 `system_dark` 桩那处，且行号已整体下移）。按 brief 要求的
"以 grep 结果为准"处理，未按 brief 的行号/条数操作。

**同时清掉的过期注释**（都是关于"过渡期抑制"本身的，抑制没了它们就是误导）：
文件头 7 行"分三批被消费 / 只标注已知待消费项 / Ruling 10 先例"的整段说明、
`PERSONALIZE_KEY` 上的"它的消费者在 Task 6"、`system_dark`(Windows) 上的"Task 6 的监视器是它的消费者"、
非 Windows 桩上那三行"别照抄 Windows 侧那句"。
非 Windows 桩的消费者事实（`AppState::new`）**保留**，它不是过渡期信息。

---

## 2. 验证矩阵

### 2.1 能跑的都跑了

| # | 项 | 仪器 | 结果 |
|---|---|---|---|
| M-1 | 构建 0 警告（强制重编） | `cargo build` | **0 warnings / 0 errors** ✅ |
| M-2 | 全量单测 | `cargo test` | **109 passed / 0 failed / 0 warnings** ✅ |
| M-3 | 暗色档 canvas 令牌真的生效 | 探针像素 (6,700) | `#08080F` —— 与 `Tokens.canvas` 的暗色**声明值逐位一致** ✅ |
| M-4 | 浅色档 canvas 令牌真的生效 | 探针像素 (6,700) | `#EDEFF7` —— 与浅色**声明值逐位一致** ✅ |
| M-5 | 翻转 `dark` 会让大范围绑定重算 | 探针模态色 | 暗档前三名 `#0F0F18`/`#0E0E17`/`#101019`；浅档 `#F8F9FC`/`#F7F8FB`/`#FCFCFD` ✅ |
| M-5b | 强调色令牌 `accent` 的明暗两值都真的到了渲染 | 探针全帧最近邻 | 见下面的**限定说明** —— 方向成立，但**没有**精确落像素，故**不按"渲染色 == 令牌值"记账** ⚠ |
| M-6 | 设置面板新增的「主题」组**没被裁** | 探针行扫描 | 面板内容行 **243..525**（窗口 0..779）→ 上下都留白 ✅ |
| M-7 | `theme-mode` 真的驱动 ChipButton 选中态 | 探针同帧 diff | 同帧只改 `theme-mode`（0 ↔ 2）：**764 个像素不同**，bbox `x 260..619, y 453..484` —— 恰是 ChipButton 行 ✅ |
| M-8 | 旧三字段 `state.json` 能启动、不崩、不被改写 | 真机启动 12 s | 见 §2.3 ✅ |
| M-9 | `state.json` 还原、3080 未动、无残留进程 | 哈希 + 端口表 + 进程表 | 见 §2.3 ✅ |
| M-10 | 旧文件兼容（解析层） | 单测 | `legacy_state_without_theme_mode_reads_as_none`、`unrecognized_theme_mode_reads_as_none`、`theme_mode_roundtrips_through_json`、`save_writes_theme_mode_key` 全绿 ✅ |
| M-11 | 真机两档观感（解锁桌面，`PrintWindow` 真截图） | 真机截图 + 像素 | 见 **§2.5.1** ✅（H-2 关闭） |
| M-12 | 真机点击路径 end-to-end（chip → 回调 → 落盘 → dirty → project） | UIA + 计算点击 + 像素 | 见 **§2.5.2** ✅（H-1 关闭） |
| M-13 | 真机持久化与重启后的选中态 | 真机截图 + 像素 | 见 **§2.5.3** ✅（H-4 关闭） |

探针（M-3..M-7）**编译的是真实的 `ui/app.slint`**（`target/theme-probe/build.rs` →
`slint_build::compile("../../ui/app.slint")`），并且驱动顺序与 `src/main.rs` 一致：
先 `MainWindow::new()`，再 `global::<Tokens>().set_dark(...)`。这几条**只**证明
"翻转 dark 会大范围换色 + 新组未被裁 + 选中态被驱动"，**不**支持逐令牌正确性 ——
后者归 Task 3+4 的对比度门槛测试（`both_palettes_meet_text_contrast_bars` 等）。

**⚠ M-5b 的限定说明（这是一条"没测成"的记录，不是 PASS）。**
brief 在截图路里要求"浅色档 `accent` 渲染色接近 `#59659B`、深色档接近 `#9AA6E8`"。
离屏探针对**静态帧**做全帧最近邻，结果**对不上**：

| 档 | 目标（令牌声明值） | 最接近的像素 | 曼哈顿距离 | 距离 ≤ 6 的像素 |
|---|---|---|---|---|
| 深色 | `#9AA6E8` | `#A5A6BC` | **55** | **0** |
| 浅色 | `#59659B` | `#64657D` | **41** | **0** |

（同一次扫描里，"最接近像素离**本档** accent 的距离 vs 离**另一档** accent 的距离"是
55 vs 174 与 41 vs 226 —— **方向明确成立**，两档的强调色系确实不同。）

**为什么对不上，以及为什么这不记成缺陷**：`Tokens.accent` 在本程序里几乎不裸用 ——
它进的是 Orb 的 `tint`（`ui/app.slint:1309`/`1493`/`1773`，带光晕与 alpha 合成）、
渐变中段（`:1600`）、以及聚焦态边框；而选中 Chip 用的是 **`accent-fill` / `accent-line` /
`accent-ink`**（`:309`/`:312`/`:332`），`accent-fill` 的浅色版 alpha 只有 `0x14`（8%）。
所以静态帧上**本来就**不该出现纯 `accent` 像素。
**结论**：这条 readback 在离屏探针上**无法复现 brief 的预期**，本记录不声称它通过；
"令牌值对不对"由 Task 3+4 的对比度门槛测试 + `parse_palette` 的下限测试负责（那些是逐令牌读值，
比像素反推强）。

### 2.2 ⭐ Fluent 控件同步——本轮的头条结论（Ruling 26）

**问题。** `ui/app.slint` 的 Fluent 同步是
`property <bool> is-dark: Tokens.dark;` + `changed is-dark => {…Palette.color-scheme…}` + `init => {…}`。
但 `init` 在 `MainWindow::new()` 内执行，**早于** `main()` 的第一次 `set_dark()` ——
那一刻 `Tokens.dark` 还是声明缺省值 `true`，所以 **`init` 那一行永远钉成 dark**。
于是浅色档用户的 Fluent 控件（`ScrollView` 滚动条、`AboutSlint`）正确与否，
**只**取决于之后 `set_dark(false)` 时 `changed is-dark` 是否真的触发。
本项目已有三处"看起来对"的接线被实测推翻，故这一条**必须实测结案**。

**方法。** 扩展离屏探针，在真实 `ui/app.slint` 上 `set_about_visible(true)`
（`AboutSlint` 是 `std-widgets` 的**真** Fluent 控件），在 `dark=true` 与 `dark=false` 两档各渲一帧，
读回同一批像素。两处独立信号：

- **信号 A —— logo 药丸。** `AboutSlint` 的 logo 源在
  `MadeWithSlint-logo-dark.svg` 与 `…-light.svg` 之间按
  `Palette.color-scheme == ColorScheme.dark` 二选一（`i-slint-compiler-1.18.0/widgets/common/about-slint.slint:17-18`）。
  两个 SVG 的主色**互补**（已读源码核实）：dark 版是 **white 底 + `#151D21` 字形**，
  light 版是 **`#151D21` 底 + white 字形**。故只需数这两个颜色。
- **信号 B —— 版本文字的默认前景色。** 那句 `Text { text: "Version 1.18.0…" }` **没有写 `color`**，
  默认色来自 `StyleMetrics.default-text-color`，而它绑定到 `FluentPalette.foreground`
  （`i-slint-compiler-1.18.0/widgets/fluent/style-base.slint:14`：`default-text-color: FluentPalette.foreground;`）。
  两个档位的取值在 `widgets/fluent/styling.slint:37`：暗 `#FFFFFF` / 浅 `#000000E6`。

**测得的数字（`dark=true` vs `dark=false`，同一帧、同一批坐标）**

信号 A —— AboutSlint logo 药丸，区域 `x∈[310,570) y∈[410,540)`，共 29900 px：

| 档 | `white` 底像素 | `#151D21` 底像素 |
|---|---|---|
| `dark = true` | **23518** | **1701** |
| `dark = false` | **1701** | **23518** |

→ **两个计数精确互换**（23518 ↔ 1701，镜像）。

信号 B —— 版本文字带 `x∈[330,550) y∈[540,572)`（先取带内众数色当底，再取亮度两端各 20 像素）：

| 档 | 带内底色 | 最暗 20 像素 | 最亮 20 像素 |
|---|---|---|---|
| `dark = true` | `#0D0D16`（暗） | `#0D0D15`（＝底色，无暗于底的字形） | **`#E7E7E7`（近白字形）** |
| `dark = false` | `#F7F8FB`（亮） | **`#2D2D2E`（近黑字形）** | `#F8F8FB`（＝底色） |

逐点样例（`x = 240/280/320/360/400/440/480/520/560/600`）：

```
y=430  dark=true : #171721 #0F0F18 #0F0F18 #FFFFFF #FFFFFF #FFFFFF #FFFFFF #FFFFFF #0E0E17 #0E0E17
y=430  dark=false: #F5F6F9 #FAFAFC #FAFAFC #151D21 #151D21 #151D21 #151D21 #151D21 #F9F9FC #F9F9FC
y=470  dark=true : #171720 #0F0F17 #FFFFFF #FFFFFF #FFFFFF #151D21 #FFFFFF #FFFFFF #FFFFFF #0E0E16
y=470  dark=false: #F4F5F8 #F9FAFC #151D21 #151D21 #151D21 #FFFFFF #151D21 #151D21 #151D21 #F8F9FC
```

**结论：`changed is-dark` 确实在之后的 `set_dark` 上触发了。两档的 Fluent 像素完全不同，
浅色档拿到了浅色的 Fluent 配色。Ruling 26 的疑点被实测排除 —— 这不是缺口。**

⚠ **这两个数字证明的是"颜色翻转了"，不是"某个声明的 α 被逐位复现"。** Fluent 的 `foreground`
声明在 `widgets/fluent/styling.slint:37`：`dark-color-scheme ? #FFFFFF : #000000E6` ——
**暗档是不透明的 `#FFFFFF`（没有 α）；带 α 的 `E6`（≈0.902）属于浅档那个值。**
若暗档字形被完整覆盖，像素应当是 255；实测最亮 20 像素均色只有 `#E7E7E7`，整个字形带
（578 个字形像素）的均色更低，是 `#7A7A7F` —— **说明这一带没有"被完整覆盖"的像素**，
观测值是抗锯齿的部分覆盖，而不是某个声明 α 的直接读数。

（这里原先写过一段"`#FFFFFFE6` 合成到 `#0D0D16` 得 `231 = 0xE7`，逐位一致"的**对账，那是错的**：
`E6` 不属于暗档的 `foreground`，`231 ≈ 255×0.902 + 13×0.098` 只是巧合；那段算术还被错用到了
浅色档那 20 个像素上，而那是另一行、另一个值。已删除。若要做对账，对象是**浅档**：
`#000000E6` 合成到浅底 `#F7F8FB`（248）得 ≈ `#18181A`，而实测最暗 20 像素均色是 `#2D2D2E` ——
同样说明这些是抗锯齿的部分覆盖，不是逐位合成值。）

**结论不受影响**（上面那条才是被测到的东西）：暗档字形近白（`#E7E7E7`）、浅档字形近黑
（`#2D2D2E`），即两档确实拿到了**不同**的 Fluent 前景色 —— 这就是 `changed is-dark` 触发了的证据。

**这一条恰好是"必须先测后信"的那条**：`init` 一行确实只兜构造那一刻（Ruling 26 的说法成立），
但 `changed` 接住了它。因此 `ui/app.slint:1114` 的原注释（"`init` 那行负责首帧，
`changed` 负责运行期切换，两者缺一不可"）把 `init` 说成能兜首帧是**不准的**，本轮已改准（见 §4）。

⚠ **范围声明**：本项实测的是 **`AboutSlint`**（`ScrollView` 滚动条未单独采）。
但两者读的是**同一个** `Palette.color-scheme` → `FluentPalette.dark-color-scheme` 链
（`widgets/fluent/style-base.slint:22-35`、`widgets/fluent/styling.slint:16-37`），
而这条链的**开关点**正是被实测到的那个属性。滚动条**未单独采样**，不必当成已实测。

### 2.3 真机：旧三字段 `state.json`（M-8 / M-9）

本机 `%APPDATA%\dsh-manager\state.json` **本身就是旧的三字段形式**（无 `theme_mode`）：

```json
{ "close_behavior": "quit", "preferred_port": null, "running_port": 3080 }
```

于是不必伪造夹具 —— 直接用它做端到端。做法：备份 → 原样启动真实二进制 → 12 s 后观测 → **强杀**
（`Stop-Process -Force`，即 `TerminateProcess`，**不**走退出处理器，因此不会 taskkill 任何 `dsh web`）
→ 还原备份 → 复验。

| 观测 | 结果 |
|---|---|
| 启动 12 s 后进程 | **存活**（CPU 1.875 s、工作集 125 MB）→ **旧文件不会让它崩** ✅ |
| 主窗口句柄 | `329010`（非 0）—— 注意：窗口**存在**但锁屏下 `PrintWindow` 取不到内容，这是 Ruling 23 的两回事，别混 |
| 启动后 `state.json` | sha256 **未变**、大小仍 80 B → **程序不会在启动时改写旧格式文件** ✅ |
| 还原后 `state.json` | sha256 = `c83231f3…`（**与备份逐字节一致**）、80 B → **还原检查通过** ✅ |
| 3080 监听 pid | 前 `23524` / 后 `23524` → **未动** ✅ |
| 残留 `dsh-manager.exe` | **0 个** ✅ |
| 点击 | **一次都没点**（面板 / 托盘 / 任何控件都没碰） ✅ |

⚠ **本节只证明了"不崩 + 不改写 + 不干扰 3080"**，**没有**证明"它以跟随系统启动" ——
那需要看到界面或读到选中态，锁屏下两者都拿不到。启动时的档位取自
`config::load()` → `theme_mode: None` → `ThemeMode::Auto`（单测 M-10 覆盖），
再经 `resolve(Auto, system_dark())` —— 这条链是**代码审查 + 单测**，不是本轮观测。

### 2.4 证据复现配方：离屏探针（⚠ 探针**不在仓库里**，这里写明如何重建）

本轮所有像素证据（M-3 ~ M-7、§2.2）都来自 `target/theme-probe/`，而 `target/` 是 gitignored 的
—— 也就是说**证据的生成器没有入库**。这是刻意的：它是一次性脚手架，不是产品代码（它只用
`slint` / `slint-build` 两个已在 GC-2 白名单里的 crate，不引入新依赖，但也没有长期维护价值）。
代价是"数字从哪来"无法从 git 里查。**下面就是配方**，供将来复核或扩测时原地重建，不必猜。

**它是什么**

- 一个独立 cargo 包：`target/theme-probe/`，`Cargo.toml` 里放一个空的 `[workspace]` 表
  （`# Standalone: keep it out of the dsh-manager workspace`），所以它**不并入**主 workspace；
- `build.rs` 只有一行：`slint_build::compile("../../ui/app.slint")` —— **编译的是真实的
  `ui/app.slint`**（不是副本），因此探针看到的 Fluent 样式/全局与产品逐字一致；
- 依赖与主程序同规格：`slint = { version = "1.18", features = ["image-default-formats"] }`
  （`slint-build` 同）—— 与主程序共用已编译产物，不额外引入 crate 版本。

**它怎么驱动（顺序是结论的一部分，必须与 `src/main.rs` 一致）**

1. 自写 `Platform` 实现，只交出一个 `MinimalSoftwareWindow`（`RepaintBufferType::NewBuffer`）
   并 `set_platform(...)` —— 纯软件渲染、离屏，**不需要真窗口、也不需要解锁的桌面**
   （这正是锁屏下唯一可行的路，见上面的环境约束）；
2. **先 `MainWindow::new()`，再 `global::<Tokens>().set_dark(dark)`** —— 与 `src/main.rs` 逐字
   一致。这个顺序正是 Ruling 26 的被测对象：`ui/app.slint` 的 `init` 在 `new()` 内跑，早于
   第一次 `set_dark()`。**顺序反了就测不到东西**；
3. `set_size(880×780)`；`render()` = `request_redraw()` + `draw_if_needed` **连渲两帧**
   （软件渲染器的脏区缓存需要一帧全量），回读 `Vec<Rgb8Pixel>`；
4. 全程**不截图、不依赖桌面**：读的是渲染缓冲区的像素。

**它采样什么**

| 采样 | 坐标 / 方式 | 对应 |
|---|---|---|
| canvas 令牌 | 单点 `(6,700)`（另加 `(300,12)/(6,400)/(874,400)/(440,770)`） | M-3 / M-4 |
| 全帧众数色 | 全帧直方图取前 3 | M-5 |
| `accent` 最近邻 | 全帧扫描：离目标色最近的像素 + 曼哈顿距离 + 距离 ≤ 6 的像素数 | M-5b（**未测成**，见 §2.1） |
| 设置面板占位 | 面板带 `x∈[240,640)` 里"亮行"的首尾行 | M-6 |
| `theme-mode` 0 vs 2 | 同帧全像素 diff + 差异 bbox | M-7 |
| AboutSlint logo 药丸 | 区域 `x∈[310,570) y∈[410,540)` 内数 `#FFFFFF` 与 `#151D21` | §2.2 信号 A |
| AboutSlint 版本文字 | 带 `x∈[330,550) y∈[540,572)`：先取带内众数色当底，再取**最暗 20 / 最亮 20 像素均色**（另报带内字形像素数与均色） | §2.2 信号 B |

**怎么重建**

```text
target/theme-probe/
  Cargo.toml    # [package] edition="2021"、空 [workspace]、slint/slint-build 1.18（同上）
  build.rs      # slint_build::compile("../../ui/app.slint")
  src/main.rs   # slint::include_modules!() 引入生成的 MainWindow/Tokens，按上面两节重写
# 然后：
cd target/theme-probe && cargo run > ../theme-probe-out.txt
```

原始输出（本轮读的那些行）留档在 `target/theme-probe-out.txt`，同样 gitignored。
⚠ **这些数字只在探针的驱动顺序仍与 `src/main.rs` 一致时才有意义** —— 若产品侧改了
`main()` 里 `new()` / `set_dark()` / `spawn_watcher()` 的先后，重跑探针前必须同步改探针，
否则跑出来的差异是探针与产品之间的差异，不是两档主题之间的差异。

---

### 2.5 终审修复轮：解锁桌面上的真机实测（H-1 / H-2 / H-4 据此关闭）

**环境（与 §2.1 的前提已不同，逐项实测）**

| 项 | 值 |
|---|---|
| 桌面 | **已解锁**：`GetForegroundWindow() = 2295668`（≠ 0）、`LogonUI.exe` **0 个** |
| 被测二进制 | 终审修复轮**重新 `cargo build`** 后的 `target/debug/dsh-manager.exe` |
| 3080 | 观测前 / 每次运行后 / 全部结束：监听 pid **恒为 23524**（另一个会话的 live server，全程未动） |
| `state.json` | 观测前 sha256 `c83231f3…`（80 B）；本轮为写夹具改过它，结束后用备份还原，**`cmp` 逐字节通过**、sha256 回到 `c83231f3…` |
| 残留进程 | 每次运行结束、以及全部结束：`tasklist` 里 `dsh-manager.exe` **0 个** |
| 触摸范围 | 只点设置面板里的三个主题 ChipButton。主窗口控制列（`启动 dsh web` / `停止 dsh web` / `安装`）与菜单里的「退出」**一次都没点**（§2.5.2 的落点核查见下） |

#### 2.5.1 H-2 —— 两档观感：截图是真的，且两档确实不同（M-11）

做法：备份 → 直接写 `state.json` 的 `theme_mode` → 启动真机 → `PrintWindow(PW_RENDERFULLCONTENT)`
→ 读像素 → **强杀** → 换另一档重来。窗口固定钉在屏幕 (20,20)，外框 **896x819**
（客户区 880x780 + 左右各 8px 边框 + 31px 标题栏；客户区原点在位图 **(8,31)**，由行/列扫描实测）。

| 档 | 写入的 `theme_mode` | PrintWindow | PNG md5 | 客户区像素 (6,700) | 精确 `#EDEFF7` | 精确 `#08080F` | 全帧不同色数 |
|---|---|---|---|---|---|---|---|
| 浅色 | `light` | `True` | `7b8af4cc321c425c43bbd05c35b39a98` | **`#EDEFF7`** | **40972** | **0** | 3879 |
| 深色 | `dark` | `True` | `169915265a6d0e50c1ec2bc05409cba6` | **`#08080F`** | **0** | **47071** | 3250 |

- **客户区像素 (6,700) 与 `ui/app.slint` 的 `Tokens.canvas` 声明值逐位相同**（`#EDEFF7` / `#08080F`）。
- **两档 md5 不同、两个画布令牌的像素计数精确互换（40972↔47071，另一档恒为 0）** ——
  这正是锁屏那次失败的反面：那时是全白位图且**两档 md5 相同**（§1 的环境约束）。
  "截图是真的"这次是**测出来的**，不是假设的。
- 两档观感的其余部分（Fluent 控件、滚动条、MadeWithSlint logo）由 §2.2 的离屏探针逐像素覆盖；
  本节补的是**真窗口上的真渲染**。

#### 2.5.2 H-1 —— 真实点击路径，端到端跑通了（M-12）

**方法（不用盲目合成输入）**：`文件` / `设置` 走 **UI Automation Invoke**
（`MenuTitle` / `MenuAction` 在 `ui/app.slint` 里声明了 `accessible-action-default`，所以 UIA 真的暴露
`InvokePattern`）；**ChipButton 没有声明无障碍动作**，在 UIA 里只以 `Text` 出现、**没有 InvokePattern**
—— 于是三个主题 chip 用**按 UIA 报告的标签矩形中心计算出的点击**，并且**点击之前**先用
`AutomationElement.FromPoint()` 证明那个坐标上的最上层元素**就是该 chip 的标签**：

| chip | UIA 标签矩形（屏幕坐标） | 点击坐标 | 点击前 FromPoint 结果 |
|---|---|---|---|
| 浅色 | `329,504 24x32` | (341,520) | `浅色|ControlType.Text` |
| 深色 | `444,504 24x32` | (456,520) | `深色|ControlType.Text` |
| 跟随系统 | `558,504 48x32` | (582,520) | `跟随系统|ControlType.Text` |

另外做了一个**正控**：`FromPoint` 在 `停止 dsh web` 中心返回 `|ControlType.Image`（该按钮里的图标）
—— 说明 FromPoint 确实按几何解析到该点元素，不是空转。⚠ 但它**不遵守浮层 z-order**（面板打开时
仍能返回面板**底下**的主窗口元素），所以它只被当作"该 chip 标签确实在这个点上"的核查，
**落点是否正确最终由结果反证**：三次点击产生的状态迁移恰好是三个 chip 各自的语义，
且像素差异**只出现在 chip 行**（见下表）。

**状态迁移**（每次点击后立刻读文件，值由**程序自己**经 `config::update` 写入）：

| 步骤 | 动作 | `state.json` 的 `theme_mode` | sha256 |
|---|---|---|---|
| 起始 | 夹具（外部写入） | `light` | `611024273c3b…` |
| 点击 1 | 深色 chip @ (456,520) | **`dark`** | `1f5ab2525197…` |
| 点击 2 | 跟随系统 chip @ (582,520) | **`auto`** | `3009258c219f…` |
| 点击 3 | 深色 chip @ (456,520) | **`dark`** | `1f5ab2525197…`（与点击 1 后同哈希） |

**像素迁移**（同一个进程、面板保持打开，所以下面两组只差"该差的东西"）：

| 对比 | 不同像素 | 差异 bbox（客户区） | 结论 |
|---|---|---|---|
| 起始(light) → 点击1后(dark) | **686387 / 733824（93.5%）** | `x 0..879, y 0..779`（**整个客户区**） | 调色板翻转被 `project()` 推到了窗口（`dirty → project` 那段是活的） |
| 点击1后(dark) → 点击2后(auto) | **7238** | `x 374..620, y 453..484` | **只有 chip 行变**。系统当前是暗色 ⇒ `resolve(Auto, dark) = dark`，调色板**不该**变，而它确实没变（全帧其余 726586 px 完全相同）→ 这一组隔离出的正是 `theme-mode → 选中态` |
| 点击2后(auto) → 点击3后(dark) | **7238** | `x 374..620, y 453..484` | 强调色移回「深色」。且点击 3 后的 PNG **md5 与点击 1 后完全相同**（`3378d5b22878…`）—— `dark → auto → dark` 往返后逐字节回到原帧 |

⚠ chip 行的 `y 453..484` 与 §2.2 离屏探针 M-7 测到的 ChipButton 行 bbox `y 453..484` **一致** ——
真机与探针在同一位置看到同一行控件。

#### 2.5.3 H-4 —— 持久化 + 重启后的选中态（M-13）

重启前**不再手写** `state.json`：文件里是**程序在 H-1 点击里自己写下的** `dark`
（sha256 `1f5ab2525197c01653629d6518099ac88598fb6dfc4cf05dd08aa91a9db22757`）。
重启后**零点击**（面板只用 UIA Invoke 打开），两张截图：

| 观测 | 结果 |
|---|---|
| 面板关闭时客户区像素 (6,700) | **`#08080F`** |
| 精确 `#08080F` / `#EDEFF7` 像素 | **47071 / 0** —— 与 §2.5.1 深色档**逐数一致** ⇒ 外观确实持久 |
| `state.json` 重启期间 | 哈希未变（程序没改写它） |

**三个 ChipButton 的选中态**用 chip **顶边条带**（位图 `y∈[483,487)`，取标签中段 32px）的
**最大亮度**量化 —— 强调边框（`accent-line`，alpha `0x47`）与未选中边框（`hairline`，alpha `0x12`）
相差约 1.85 倍，且"最大亮度"几乎不受面板背后内容影响：

| 采样（同一帧内） | 浅色 chip | **深色 chip** | 跟随系统 chip |
|---|---|---|---|
| H-1 点击1/3 后（`theme_mode=dark`，参考） | 42.65 | **77.60** | 41.72 |
| H-1 点击2 后（`theme_mode=auto`，参考） | 42.65 | 41.72 | **77.60** |
| **H-4 重启后** | 42.65 | **69.32** | 41.72 |
| H-4 重启后，chip 内部背景块均亮度 | 26.67 | **30.90** | 26.10 |

→ 重启后**唯一**带强调边框的是「深色」；另外两个与"未选中"参考值一致。
⚠ 重启那次的绝对值（69.32）比同进程参考（77.60）略低：面板是**半透明玻璃**，`accent-fill` 只有
`alpha 0x1A`，合成结果会随面板**背后**的内容（日志 / 说明文字，逐次运行不同）小幅漂移。
所以判据是**同一帧内**"强调 vs 非强调"的对比，不是跨运行的绝对值相等。

#### 2.5.4 本节的两条方法学限定（都影响"这条到底证明了什么"）

1. **"退出"是用强杀代替的**：三次运行都以 `Stop-Process -Force`（即 `TerminateProcess`）结束，
   **不经过退出处理器**，也没有点菜单里的「退出」。对 H-4 而言这只会让结论**更强**：
   偏好是在**点击那一刻**就落盘的，程序没有任何"退出时补写"的机会。
   代价是：**"优雅退出后再启动仍是所选档"这条没测**（它依赖的落盘动作已被同一条路径覆盖）。
2. **H-4 的 H-1 前置**：重启前的 `dark` 是 H-1 点击写进去的，所以 H-4 同时依赖 §2.5.2 那条链路成立。
   两条不是独立证据。

---

## 3. 未执行 / 待人工 —— ⚠ 以下**没有**通过，只是没做

**这一节里的任何一条都不得被读成 PASS。** 它们没有被代偿证据覆盖。

⚠ **H-1 / H-2 / H-4 已不在本节** —— 它们在终审修复轮里**实测关闭**，证据与数字见 **§2.5**。
本节只剩下面三条（外加随后那条终审新发现的高对比度缺口）。

| # | 项 | 为什么没做 | 交付给谁 |
|---|---|---|---|
| **H-3** | **实时跟随**：应用以「跟随系统」运行，改系统主题（设置 → 个性化 → 颜色），确认**主体与系统标题栏同时**变化并记延迟 | 需要改**用户的系统主题设置**（一个真实的系统级副作用）。终审修复轮**桌面已解锁**，但这条仍然没做 —— **不得**替用户改系统主题；且它要的是"延迟"这个连续量，得有人盯着窗口记时间 | **人**：解锁会话里按上面步骤做一次，记录延迟。⚠ 同族还有一条**终审新发现的缺口**（下一条） |
| **H-3b** | **高对比度切换不会叫醒我们的监视器**（终审发现，**已知缺口，不修**）：`system_dark()` 每次都咨询 `high_contrast()`，但监视线程只武装在 `HKCU\…\Themes\Personalize` 上；HC 开关写的是 `HKCU\Control Panel\Accessibility\HighContrast`（不同的键，`bWatchSubtree` 不覆盖兄弟键）⇒ **跟随系统**档下"只切 HC"会让 winit 按 `WM_SETTINGCHANGE` 重画标题栏、我们的主体不动，**两者分叉**，直到下一次真正的 `Personalize` 变更或重启 | 复现它必须**真的切换高对比度** —— 那是**系统级无障碍设置、属于用户**，本轮（与本分支）刻意没动。所以这条是**代码/依赖源码核实**的结论（winit 侧出处：`event_loop.rs:2423-2430`），**不是实测复现** | **人 / 下一轮**：机制、触发条件、为什么不在本分支修，全部记在 `docs/RULINGS.md` 的**平台事实 4**。⚠ 降级路径（武装失败 ⇒ 每 5s 重读）**没有**这个缺口 |
| **H-5** | **`ScrollView` 滚动条的 Fluent 配色**单独采样 | 见 §2.2 的范围声明：探针采的是 `AboutSlint`（同一 `Palette` 链，但不等于同一个控件） | 若要更硬的证据：探针里把 `notes-status` 设为 `ok` 并喂入超长 `notes-blocks` 让滚动条出现；或随 H-2 目视（H-2 的两张真机截图已入库到 `target/`，但 `target/` gitignored） |
| **H-6** | **绑定真实性的"逐令牌"校验**（40 条令牌每条都真的跟着 `dark` 走） | 探针只采像素，不逐条读回属性 | Task 3+4 的对比度门槛测试覆盖"值对不对"；"绑定是否逐条成立"目前只有抽样 + 代码审查 |

**另记两条不阻塞、也不属本轮范围的遗留**（来自 Task 7+8 评审的 Minor 分流，Ruling 27 定为"延到终审 triage"）：
`src/main.rs:1470` 重复探测了一次 `system_dark()`（可直接读 `state.system_dark`）；
新增的主题 ChipButton 行上方缺一条与「关闭行为」行同款的 rationale 注释。
本轮**没有**改它们（不在 Task 9 brief 范围内，且都改行为/结构而不只是注释）。

---

## 4. 本轮改动的两处注释（Task 7+8 评审提出）

| 位置（**改动前**的行号；改动后分别为 `:1114`、`:1905`） | 原说法 | 改成 | 依据 |
|---|---|---|---|
| `ui/app.slint:1114` | "`init` 那行负责首帧，`changed` 负责运行期切换，两者缺一不可" —— 把 `init` 说成能兜首帧 | 明说 `init` **不**负责同步（它在 `MainWindow::new()` 里跑，早于第一次 `set_dark()`，那时 `Tokens.dark` 还是缺省 `true`，故**永远**钉 dark）；运行期同步全靠 `changed`；并注明已由探针实测 | §2.2（Ruling 26） |
| `ui/app.slint:1901` | "目前只有一项（关闭行为）" | "目前两组：关闭行为、主题" | 设置面板实际已有两组（`Eyebrow { text: "关闭主窗口时" }` 与 `Eyebrow { text: "主题" }`） |

两处都只是注释，改后重新强编 + 全量测试：**0 警告 / 109 passed**（与改前一致）。

---

## 5. 本轮结论

- 过渡期抑制 **6 处全部删除**，删除后 **构建 0 警告、0 条 `dead_code`**，**没有真实死代码浮出**。
- 全量测试 **109 passed / 0 failed / 0 warnings** —— 与 Task 8 结束时的基线一致（无回归、无新增）。
- **Ruling 26 的 Fluent 疑点已实测排除**：`changed is-dark` 确实触发，两档 Fluent 像素精确互换
  （logo 药丸 23518 ↔ 1701，文字字形近白 ↔ 近黑）。
- 真机在旧三字段 `state.json` 上启动 12 s：不崩、不改写该文件；`state.json` 已按备份**逐字节还原**；
  3080 监听 pid `23524` 全程未变；**无残留进程**；**未点击任何控件**。
- **6 项未执行（H-1 ~ H-6）**，其中 H-1 / H-2 / H-3 / H-4 需人在**解锁**会话里做 ——
  它们**不是通过**，见 §3。
  ⚠ **（终审修复轮更新：H-1 / H-2 / H-4 已在解锁桌面上实测关闭，见 §2.5 与 §5.1；未执行的只剩
  H-3 / H-5 / H-6 与终审新发现的 H-3b。上面这行保留为当轮的原话。）**
- **1 项部分成立**（M-5b）：强调色的明暗区别方向成立，但 brief 期望的"渲染色 ≈ 令牌值"
  在静态帧上**测不出来**（纯 `accent` 本来就不裸用），已按"没测成"记账，见 §2.1 的限定说明。

### 5.1 终审修复轮的增补结论（2026-09-22 同日，桌面已解锁）

- **H-1 关闭**：真实点击路径**端到端跑通**。三次点击（深色 → 跟随系统 → 深色）让 `state.json` 依次
  变成 `dark` → `auto` → `dark`（由**程序自己**写入），light→dark 那次改动 **686387 px（93.5%）**，
  两次"只动选中态"的改动各 **7238 px 且只落在 chip 行 `x 374..620, y 453..484`**，`dark→auto→dark`
  往返后**帧 md5 逐字节回到原值**。证据见 §2.5.2。
- **H-2 关闭**：两档真机截图**真实可用**（`PrintWindow=True`、两档 md5 不同、画布令牌像素
  40972↔47071 精确互换、客户区 (6,700) 像素与 `Tokens.canvas` 声明值逐位相同）。见 §2.5.1。
- **H-4 关闭**：由**程序写下的** `dark` 在重启后仍生效（客户区画布 `#08080F`、精确计数 47071/0，
  与深色档参考**逐数一致**），且重启后**唯一**带强调边框的 chip 是「深色」（顶边条带最大亮度
  69.32 vs 41.7/42.7）。见 §2.5.3。
- **仍未执行**：**H-3**（实时跟随，需改用户系统主题）、**H-3b**（高对比度不叫醒监视器 —— 终审
  **新发现**的缺口，机制见 `docs/RULINGS.md` 平台事实 4；未实测复现，因为复现要动用户的无障碍设置）、
  H-5、H-6。**"未执行"就是未执行，见 §3。**
- 安全账：`state.json` 备份→还原 **`cmp` 逐字节通过**（sha256 回到 `c83231f3…`）；3080 监听 pid
  **恒为 23524**；`dsh-manager.exe` **0 个残留**；主窗口控制列（启动/停止 dsh web、安装）与「退出」
  **一次都没点**。


---

## 6. 插件卡与操作卡改版（2026-09-22，v1.4/v1.7 同批）

规格：`docs/superpowers/specs/2026-09-22-plugin-manager-design.md`；计划：`docs/superpowers/plans/2026-09-22-plugin-manager.md`。

### 6.1 自动化证据

| # | 项 | 证据 |
|---|---|---|
| P-1 | 单测 | `cargo test` **128/128** 通过（v1.3 基线 116 + 本轮 12：`plugin::*` 9 条、`model::plugin_op_*`/`updatable_*` 3 条） |
| P-2 | 零警告 | `cargo build` **0 warning**（本仓库门槛：删了东西就要复查死代码） |
| P-3 | 命令表精确性 | `plugin_op_args_are_table_driven` 断言四种操作的**确切参数序列**（与 `pm_command_table_is_exact` 同款粒度）—— 只断言"非空"的话，`add` 误写成 `remove` 也能通过 |
| P-4 | 信任边界 | `valid_spec_rejects_non_registry_sources`：`file:` / `link:` / `git+` / `github:` / URL / 相对与绝对路径 / 空白 / 引号 / `@scope/p@1@2` / `^1.0.0` **逐条**断言拒绝 |
| P-5 | spec F6 的回归钉子 | `read_installed_lists_only_dependencies_and_tolerates_missing_version`：`node_modules` 里**不在 `dependencies`** 的包不得出现（真机上就是 `@hilariouhiss/dsh-skill-kit`） |
| P-6 | 降级不算更新 | `updatable_requires_both_versions_and_a_strictly_newer_latest`：已是最新 / 未知 / 未安装 / **latest 更旧** 四种否定情形 |
| P-7 | 参数真的到了进程 | `run_plugin_forwards_args_verbatim_and_reports_exit_code`：假 `dsh.cmd` 回显 `ARGS:%*` + `exit /b 3` ⇒ 断言 stdout 含 `ARGS:plugin --profile web remove @s/p` **且** 退出码 3 是结果不是 `Err` |

### 6.2 真机 UIA 读回（`dsh-manager.exe`，启动后 14 s）

无障碍树里插件卡的实际内容（**顺序即版式顺序**）：

```
Text:插件            Text:6 个 · 1 个可更新
Edit:@scope/name 或 @scope/name@1.2.3      Text:安装
Text:@hilariouhiss/dsh-codegraph   Text:1.1.0 → 1.2.0   Text:^1.1.0   Text:更新  Text:卸载
Text:@hilariouhiss/dsh-colgrep     Text:1.2.1 → 1.2.1   Text:1.2.1    Text:已是最新  Text:卸载
…（其余四行同形，均"已是最新"+仅〔卸载〕）
Text:全部更新        Text:刷新        Text:更新说明
```

- **真实数据**：`dsh-codegraph 1.1.0 → 1.2.0` 是 registry 经**系统代理**（FR-33）查回来的真更新，
  汇总行"6 个 · 1 个可更新"与它一致；其余五行 `→ 同版本` + 「已是最新」。
- **FR-36 的一条硬要求**：〔更新〕**只出现在真有更新的那一行**（spec §8.2 "禁用不如不显示"）——
  首版实现成了恒显示 + 禁用，UIA 直接照出五行灰按钮，已改 `if` 并复测。
- 同一棵树里 `包管理器 / npm / 当前版本 / 0.1.7-alpha.1 / 已是最新 / 目标版本 / …` 按新顺序出现，
  `DSH 在此！`、行尾 `当前`、`通道最新` **均不在树里**（操作卡改版与版本主卡删除的回归检查）。

### 6.3 离屏探针（`target/ui-probe/`，配方同 §2.4）

编译的是**真实的** `ui/app.slint`。本轮量到的关键数字：

| 位置 | 测量 | 判读 |
|---|---|---|
| 左栏首条带 | `y 78..99 高 22`，x 聚类 `30..78 103..123 189..237 249..343 355..417` | 高 22 = `Tag` 声明高度 ⇒ 版本主卡已删、徽标落在操作卡内且**贴住内容右缘**（416/417） |
| 目标版本行 | `y 124..135`，x>197 处只有控件边框(417)与卡片边(434) | 行尾**无第三簇** ⇒ 「当前」提示确已移除 |
| 右栏两卡 | 插件卡 header 52..76、安装行 79..110、6 行 116..347、汇总行 **374..405**；更新说明标题 **430..438** | 插件卡在**上**、更新说明在**下**；行距 40px |
| 重装确认框 | 卡片 `y 307..474`、内容框 `[260,620)` | 400px 宽、水平居中（(780−167)/2 = 306.5）✓，未裁出窗口 |
| 遮罩 | 卡片区之外 **9394/9394** 采样点比无对话框时更暗 | 遮罩确实覆盖全窗 |

- 探针抓到并已修的两处：**①** 重装框正文的"？"被挤成第二行单独挂尾（带 x `260..277`）⇒ 拆成
  "版本号一行 + 固定问句一行"，现两行 `260..411` / `261..529` 各自单行；**②** `plugin_card_height`
  的行高常数 46 vs 实际 40 ⇒ `ListView` 比内容高 36px，最后一行与汇总行之间凭空多出 **63px** 空白
  （带 y347 与 y410）⇒ 改 40 后汇总行上移到 y374。
- ⚠ **探针不在仓库里**（`target/` gitignored，与 §2.4 同一处置）。重建配方见 §2.4；本轮新增的
  采样点是"右栏两卡的竖向分配"与"行内 x 聚类"（`x_clusters` 函数）。

### 6.4 环境限制（⚠ 影响下列端到端验证的执行者）

本会话沙箱里 **pnpm 跑不起来**：`pnpm --version` 直接返回
`The path cannot be traversed because it contains an untrusted mount point`，
`dsh plugin …` 因此在沙箱里必然失败（用户在真实终端里可正常运行 —— 那份
`dsh plugin --profile web list` 输出就是证据）。故 **V-P1 ~ V-P7 必须由用户在真实环境执行**。

### 6.5 V-P1 ~ V-P7（待用户执行）

| # | 步骤 | 通过判据 | 状态 |
|---|---|---|---|
| **V-P1** | 启动程序，看插件卡 | 6 行、包名与 `dsh plugin --profile web list` 逐条一致；已装版本 = `node_modules` 里的真实版本；最新版列在几秒内填好（并行，不是半分钟） | ⬜ |
| **V-P2** | 点某行〔更新〕（选确有新版的） | 日志出现 `$ dsh.cmd plugin --profile web add <包>@<版>` 与 pnpm 输出；`package.json` 里该包规格真的变了；列表自动刷成"已是最新" | ⬜ |
| **V-P3** | 输入 `@hilariouhiss/dsh-gitbash@1.0.1` 点〔安装〕 | 同上；`dependencies` 新增一条，且**新包自动进了 `dsh.profile.bundles`**（spec F3 的实证） | ⬜ |
| **V-P4** | 点某行〔卸载〕→ 确认框 → 取消 | **什么都不发生**：`package.json` 未变、日志里没有命令 | ⬜ |
| **V-P5** | 再点〔卸载〕→ 确认；**dsh web 正在运行时也要成功** | 依赖从 `dependencies` 移除、`dsh.profile.bundles` 里不再残留（F3）；**`dsh web` 进程未被打断**（F9 的核心断言） | ⬜ |
| **V-P6** | 输入 `file:../../evil` 点〔安装〕 | 状态栏报"包规格不合法"，**日志里没有命令**、`package.json` 未变 | ⬜ |
| **V-P7** | 〔全部更新〕 | **一次**命令、多条规格；全部更新完列表变"已是最新" | ⬜ |

### 6.6 本轮未执行项

- **真实 `dsh plugin` 的端到端**（V-P1~V-P7）：沙箱无 pnpm，见 §6.4。
- **`minimumReleaseAge` / `allowBuilds` 拦截路径**：需要 pnpm 真拦一次才能测；设计上只把 pnpm 原文
  送进日志（不代改 `pnpm-workspace.yaml`，spec §12 已知限制 3/4）。
- **行内文本被裁切的情形**：探针里 `plugin-card-height` 由探针自己推（与产品常量同步），
  "窗口被拉到很矮时两卡如何让位"只在设计层面定了规则（优先保更新说明的 `min-height`），未实测。

---

## 7. 本程序自身的更新（2026-09-23，SRS v1.5 / ARCHITECTURE v1.8）

需求：SRS §3.10 的 **FR-38**（检查）/ **FR-39**（下载并引导安装）；设计：ARCHITECTURE **§4.9**。

### 7.1 自动化证据

| # | 项 | 证据 |
|---|---|---|
| U-1 | 单测 | `cargo test` **134/134** 通过（v1.4/v1.7 基线 128 + 本轮 6：`config` 的 `skipped_app_version_*` 1 条、`dsh` 的 `parse_app_release_*` 2 条 / `parse_sha256sums_*` 1 条 / `parse_certutil_hash_*` 1 条 / `download_asset_rejects_non_api_github_hosts` 1 条） |
| U-2 | 零警告 | `cargo build` **0 warning** |
| U-3 | 解析的判别性 | `parse_app_release` 的表驱动用例覆盖：严格大于才算新版本（**相等 / 更旧 → `None`**，防降级）、`prerelease` / `draft` → `None`、**资产名不符 → `Err`**（发布漏传必须留痕）、tag 不带 `v` 也认、tag 非法 → `Err`、JSON 非法 → `Err` |
| U-4 | 校验和解析 | `parse_sha256sums`：`sha256sum` 的两种写法、CRLF、大小写不敏感、缺条目 → `None`、别的文件名不得误命中 |
| U-5 | certutil 解析**不依赖语言** | `parse_certutil_hash` 的夹具同时含**中文**与**英文**两种 certutil 输出 —— 按 64 位十六进制的形状取，不按文案（文案随系统语言变化，按文案匹配的实现在另一种语言上必然失败） |
| U-6 | 信任边界 | `download_asset` 拒绝非 `https://api.github.com/repos/` 的资产地址（URL 来自响应体，不是本程序拼的） |
| U-7 | 忽略记忆 | `skipped_app_version` 往返 + 缺 key / 非字符串 → `None`（旧 `state.json` 照常可用，且**不得**判为损坏） |

### 7.2 真实 API 的检查（一次性，跑完即删）

跑法：临时加一个 `#[ignore]` 测试调用 `dsh::fetch_app_release(&local)`，`cargo test tmp_net -- --ignored --nocapture`，**跑完删除**（真网络不进 CI，与既有的 `list_args_run_for_installed_pms` 同属"机器相关"一类，但那条是既有的、这条没有长期价值）。

真实的 GitHub 响应实测（本机、走系统代理）：

```text
local 0.1.0 → Err: release v0.2.1 里没有资产 dsh-manager-v0.2.1-windows-x64-setup.exe
local 0.2.1 → 已是最新
local 0.3.0 → 已是最新
```

三条都有信息量：**①** 真机取到了 `releases/latest`（代理、UA、JSON 全通）；**②** 相等与更旧都判为"已是最新"（严格大于的判据在真数据上成立）；**③** 第一条正是**想要**的响亮失败 —— v0.2.1 是用旧资产名发布的，于是"名字对不上"以精确的 `Err` 暴露出来，而不是被吞成"已是最新"。

### 7.2b 发布之后复验（2026-09-23，v0.3.0 已上线）

§7.2 那三条是**发布前**跑的（那时最新 release 还是 v0.2.1）。v0.3.0 发布后按同一配方再跑一遍，
这次才是真正的端到端：更新器能不能看到**我们自己刚发的那个 release**。

```text
local 0.1.0 → 有新版本 0.3.0 | setup=dsh-manager-v0.3.0-windows-x64-setup.exe | api_host=true | sums=true
local 0.2.1 → 有新版本 0.3.0 | setup=dsh-manager-v0.3.0-windows-x64-setup.exe | api_host=true | sums=true
local 0.3.0 → 已是最新
```

**FR-39 的校验链也在真实发布物上跑通了**（同一临时用例，跑完即删）：

```text
certutil 实算 = 66be154564d24d59f8013599fdb06101fb0c1e4e6ad1fcd4004348ac6c0a9950
发布物声明  = 66be154564d24d59f8013599fdb06101fb0c1e4e6ad1fcd4004348ac6c0a9950
```

即 `sha256_file`（真的起了 `certutil.exe` 进程并解析其输出）与 `parse_sha256sums`
（解析 release 里那份 `SHA256SUMS.txt`）给出**同一个**哈希 —— 这一条之前只有单测覆盖
（单测用的是夹具），现在有真进程 + 真发布物的证据。发布物本身的核对：

| 资产 | 大小 | 核对 |
|---|---|---|
| `dsh-manager-v0.3.0-windows-x64-setup.exe` | 8 719 847 | SHA256 与 `SHA256SUMS.txt` 逐字符一致；`ProductVersion=0.3.0`、`FileVersion=0.3.0.0` |
| `dsh-manager-v0.3.0-windows-x64.exe` | 17 186 304 | 与 `SHA256SUMS.txt` 一致（免安装副本） |
| `SHA256SUMS.txt` | 210 | 两行、文件名与资产名逐字一致 |

### 7.2c 发布之后复验（2026-09-23，v0.4.0 已上线）

同 §7.2b 的配方（临时 `#[ignore]` 测试 → `cargo test tmp_net_check -- --ignored --nocapture` → **跑完删除**，
已确认 `git status` 干净）：

```text
local 0.1.0 → 有新版本 0.4.0 | setup=dsh-manager-v0.4.0-windows-x64-setup.exe | api_host=true | sums=true
local 0.3.0 → 有新版本 0.4.0 | setup=dsh-manager-v0.4.0-windows-x64-setup.exe | api_host=true | sums=true
local 0.4.0 → 已是最新
```

三条各有分工：**①** v0.3.0 的用户点〔检查更新〕**真的会看到 v0.4.0**（发布这件事对更新器是可见的，
而不只是"Release 页上多了个 tag"）；**②** 安装包资产名与 `app_setup_asset_name` 的构造逐字一致、
走的是 `api.github.com`（不是不可达的 `browser_download_url`）、`SHA256SUMS.txt` 也在；
**③** 新版本不提示自己（严格大于的判据在真数据上成立）。

另：发布物本身的校验链也在真资产上跑了一遍 —— `gh release download v0.4.0` 取回安装包与
`SHA256SUMS.txt`，用系统 `certutil -hashfile … SHA256`（FR-39 走的就是它）实算得到
`826ffa6f7193e19b7f1c6b8f302cad82ae4a29dda7d54b170ae0bc51577398a8`，与发布页声明的**逐字符一致**。

### 7.3 离屏探针（`target/ui-probe-update/`，配方同 §2.4）

复用 §2.4 的一次性探针配方（新开一个 `target/` 下的独立包，`build.rs` 编译**真实的** `ui/app.slint`），量本轮新增的两处界面：

| # | 项 | 结果 |
|---|---|---|
| UI-1 | **徽标位置（第一版是错的）** | 占位条方案：徽标 diff bbox `x 118..230  y 8..37` —— 紧贴着「帮助」落在左上角，压在那团氛围光上。**根因**：菜单栏必须 `alignment: start`，而该对齐下**子项的 `horizontal-stretch` 不生效**，占位条宽度为 0 |
| UI-2 | 徽标位置（改正后） | 外层再套一层默认（stretch）布局、里层自己 `start` 对齐：徽标 diff bbox **`x 753..865  y 8..37`（113×30）** —— 右内边距 14px，正好收在 x=866（窗口 880 宽） |
| UI-3 | 弹框卡片几何 | 卡片底色相同的像素带 `x 230..421`（行 y=280）⇒ 左沿 **230** = `440 − 420/2`，与 `Bezel { width: 420px }` 一致；**未被窗口裁切** |
| UI-4 | 弹框内容不越界 | 各行文字簇最右端 **629**、强调按钮块 `x 492..629（138×42）`；卡片右沿 650、`core-padding` 20 ⇒ 内容全部落在内边距之内 |
| UI-5 | 按钮成行 | 按钮行（y 458..482）三个按钮同带，强调动作在最右（与其它弹框同一条规则） |
| UI-6 | 遮罩生效 | 卡片区之外 **7512/7512** 个采样点比"无弹框"那一帧更暗 |

### 7.4 待用户执行（真机端到端）

| # | 对应 | 步骤 | 通过判据 | 状态 |
|---|---|---|---|---|
| **V-28** | FR-38 | 在装着旧版（v0.2.1）的机器上启动 | 顶栏出现「有新版本 v…」徽标并自动弹一次；〔以后再说〕→ 本会话不再弹、重启又弹；〔忽略此版本〕→ 重启不再弹（徽标仍在）且 `state.json` 出现 `skipped_app_version`；**断网启动不弹任何错误框**，只有日志一行 | ⬜ |
| **V-29** | FR-39 | 更新框里点〔下载并安装〕 | 日志依次出现"正在下载 …"与"SHA256 校验通过：…"；**安装进度窗**出现（v0.5.0 起改为静默安装，不再是向导）、本程序退出；**`dsh web` 仍在监听**（判别性的一步：它若被停，说明退出走了 FR-21 那条路，用户的会话被误杀）；装完自动重启 → 显示新版本，`dsh web` 被认成"外部 dsh web" | 🟡 见 §10.8：下载 / 校验 / 静默安装 / 自动重启都在真机上跑通，且下载物与装完的 exe 与 Release **逐字节一致**；**只剩"`dsh web` 没被停"这一步没有取证**（那次本机没在跑 `dsh web`） |
| **V-30** | FR-39 失败路径 | 把 `SHA256SUMS.txt` 改坏后再点〔下载并安装〕 | **不得**启动安装程序；日志出现"SHA256 不匹配（期望 …，实得 …）"；`%TEMP%\dsh-manager-update\` 里那个安装包**已被删除** | ⬜ |

### 7.5 本轮未做

- **真机端到端（V-28~V-30）**：需要"比当前版本更新的 release"存在，因此只能在 v0.3.0 发布之后由用户执行。
- **静默安装档**：本轮按用户选择只做 B 档（下载 + 可见向导）。C 档（`/SILENT`）在"为所有用户安装"的副本上会卡 UAC，未实现。
- **下载进度百分比**：未做（见 ARCHITECTURE §4.9.7）。

---

## 8. 更新说明里的站内链接不再误报（2026-09-23，SRS v1.6 / ARCHITECTURE v1.9）

**用户报的现象**：更新 dsh（npm `0.1.7-alpha.1 → 0.1.7-alpha.2`）后，日志里反复出现
`打开网页失败：拒绝打开非 http(s) 连接: #en-v0.1.7-alpha.2`（截图里 10 条，`#en-…` 8 条 / `#cn-…` 2 条）。
⚠ 上面这句是**照截图/用户原话**抄的；代码产出的那一行是
`打开网页失败: 拒绝打开非 http(s) 链接: #en-v0.1.7-alpha.2`（半角 `: `，且是「**链接**」不是「连接」——
分隔符来自 `main.rs` 的 `format!("{context}失败: {message}")`，文案来自 `dsh::open_url` 的 `Err`）。

### 8.1 根因（先定位，再改）

| 环节 | 事实 | 证据 |
|---|---|---|
| 触发源 | 说明正文**第一行**就是语言导航行 `[中文](#cn-v0.1.7-alpha.2) \| [English](#en-v0.1.7-alpha.2)` | `GET /repos/deepseek-ai/deepseek-harness/releases/tags/dsh-v0.1.7-alpha.2` 的 `body` 首行（本次实测） |
| 放大到全部版本 | 20 个 dsh release **每个都有**这一行；正文链接分类：锚点 40 条、相对路径 6 条（`SAFETY.md` / `SAFETY.zh.md` / `BRAND_GUIDELINES*.md`）、真 https 8 条 | 同上，`releases?per_page=100` 全量正则统计（本次实测，评审者独立复测一致）。**锚点写法有三种**：`#cn-v…`/`#en-v…`（近期）、`#chinese`/`#english`、`#cn`/`#en`（更早）—— 所以判据必须是"**不是 http(s)**"这个通用前缀，而不是"以 `#cn-` 开头" |
| 派发 | `win.on_link_clicked` **无条件**把字符串变成 `Job::OpenUrl` | 代码：`src/main.rs`（修复前）—— 两处转发（`NoteText` → `NoteBlockView`，再 `NoteBlockView` → `MainWindow` 的 `root.link-clicked`；说明面板本身就在 `MainWindow` 里）都不判别 |
| 报错 | `dsh::open_url` 的白名单（Ruling 65）在 `spawn` 之前 `Err` → `UiMsg::Failed{context:"打开网页"}` → 日志一行 | 白名单是**正确**的：改的不能是它。全仓库只有这一处 `context: "打开网页"` |
| 一次点击 = 一条日志 | `link-clicked` 只在 Slint 的 `MouseEvent::Released{Left}` 落在链接上时发一次 | `i-slint-core-1.18.0/src/items/text.rs:311`（`Released` 分支）→ `:319`（`link_clicked.call`）；shared-parley 路径，该 feature 在默认集合里。该文件里 `key_event` / `focus_event` 都不发它、也没有 accessibility 发射路径 ⇒ **没有**键盘/无障碍的第二条生产者。截图里的十几条 = 用户反复点击（点了没反应会继续点），不是自动循环 |
| 不是事务触发的 | 更新事务走的是"重探环境 → `start_web(port)`"，那条路上**没有**任何 `Job::OpenUrl`；日志也不落盘（`state.json` 无日志字段），启动时不会重放 | 代码：`Job::TxDone` 分支（`src/main.rs`）；`Job::StartWeb` 只有两个发送点，`start_web` 不发 `OpenUrl` |

### 8.2 改法

- `dsh::is_web_url(&str) -> bool`：新的纯函数（前缀判据、大小写不敏感）。`open_url` 的白名单改用它 —— 判据从此只有一份。
- `main::dispatch_notes_link(url, &impl Fn(Job))`：新的纯函数，`on_link_clicked` 的**准入**（与既有的 `dispatch_plugin_op` 同款：派发走调用方给的 `send`）。非 http(s) 的正文链接（锚点 / 相对路径）**一个任务都不派发**：不打开、不报错、不写日志（面板里没有锚点目标 —— FR-27 已裁决不做中英切换，它们既打不开也不是"打开失败"）。
- **白名单与 `explorer.exe` 单参数的注入防线一字未动**（Ruling 65 仍是第二道，只是不再有人往它嘴里塞站内链接）；`open_url` 的 `Err` 文案也逐字未变（`git show HEAD:src/dsh.rs` 对照）。

### 8.3 自动化证据（本机实测）

| 项 | 结果 |
|---|---|
| `cargo test` | **138 passed / 0 failed**（本轮新增 4 条；`HEAD` 为 134 条：`#[test]` 计数 dsh 33→35、main 0→2，其余文件不变） |
| `open_url_refuses_non_web_targets`（dsh） | `#en-v0.1.7-alpha.2` / `#cn-…` / `SAFETY.md` / `file:///…calc.exe` / `javascript:` / 空串逐条 `Err`，且都发生在 `spawn` 之前 —— 钉住 Ruling 65 的**顺序**。判别性的一步 `file:///…calc.exe` 不会弹出计算器：跑完该测试后 `tasklist /FI "IMAGENAME eq CalculatorApp.exe"`（与 `calc.exe`）均为 *No tasks*（跑前跑后各查一次） |
| `notes_in_page_links_are_not_web_urls`（dsh） | 锚点与相对路径 → `false`；`github.com/…/compare\|blob`、`http://127.0.0.1:3080`、`HTTPS://…`（大小写混写）→ `true` —— 判据本身的表驱动 |
| `in_page_notes_links_dispatch_nothing`（main） | **判别性的一条**，且**覆盖接线**：给 `dispatch_notes_link` 一个真通道，断言 worker 收件箱里什么都没有。四个取自真实正文的取值：`#en-v0.1.7-alpha.2`、`#cn-v0.1.7-alpha.2`、`#english`（更早 release 的短锚点）、`SAFETY.md`。红-绿实测：把 `dispatch_notes_link` 的判别整个去掉（= 修复前的无条件派发，**不留下任何"未被调用的辅助函数"**）→ 该测试**失败**于第一条断言（`#en-v0.1.7-alpha.2 不该派发任何任务（用户报的刷屏就是它）`；删掉那三行后 `panicked at src\main.rs:2459`，行号会随删改前移，以文案为准）；改回后 138/138 全绿 |
| `real_notes_links_still_dispatch_verbatim`（main） | 反面：真链接仍派出 `Job::OpenUrl`，且 Url **逐字**透传。反向变异实测：把 `dispatch_notes_link` 改成"什么都不派发" → 该测试失败于 `真链接必须派发 OpenUrl，实得 Err(Empty)`。没有它，一个"什么都不派发"的实现也能骗过上面那条 —— 那等于把正文末尾的 Full Changelog 变成死链 |
| `cargo build` | **0 警告**（`touch src/*.rs` 后强制重编，`grep -c '^warning'` = 0；`HEAD` 的 134 条计数与"main.rs 第一个测试模块"同样在这轮核对） |
| 单测没有泄漏进产物 | `cargo build --release`（exit 0、0 警告）后：`grep -c "不该派发任何任务" target/release/dsh-manager.exe` → **0**、`grep -c "真链接必须派发 OpenUrl"` → **0**（测试专用文案不在产物里），对照 `grep -c "拒绝打开非 http(s) 链接"` → **1**（非测试代码在） |
| 路径唯一性（评审者独立复核） | `Job::OpenUrl` 的生产发送点只有三个：本函数、窗口「打开网页」、托盘「打开 DSH 网页」（后两个恒为 `http://127.0.0.1:<port>`）；`link-clicked` 全仓库只有一条 Slint 转发链 + 一处 Rust 注册，无 `invoke_*`、无拖放、无第二条 URL 回调 |

### 8.4 未做（诚实记录）

- **真机点击回放**：`link-clicked` 是 GUI 回调，本仓库按 §6.3 的边界不做 GUI 自动化；本轮**没有**真机点一遍语言导航行。判据 = 上表那四条单测（含接线那一跳）+ 路径唯一性。想亲眼确认的话：打开任一版本的更新说明，点第一行的〔English〕—— 日志**不应**新增任何行。
- **点击仍然"没反应"是有意的**：站内链接点了什么都不发生（不弹、不跳、不写日志）。Ruling 92 把"用户可见的静默失败"列为缺陷类，这里刻意选择静默 —— 依据是 FR-27 已裁决不做中英切换（面板里根本没有锚点目标），且报错本身正是用户报的 bug。要改成"跳转"属增量功能，见下条。
- **锚点跳转（点〔English〕滚到英文段）**：未做，与"不做中英切换"同一条裁决；要做需把 `<h3 id="…">` 的锚点带进 `NoteBlock`、在 Slint 侧换算滚动位置（`ScrollView.viewport-y` 需要各块 y 偏移，块高要等布局跑完），属增量功能。

---

## 9. 输出面板的正文可选中复制（2026-09-23，SRS v1.7 / ARCHITECTURE v1.10）

**用户要求**：输出（FR-28 的日志面板）里的文字可以选中复制。
**改法**：`ui/app.slint` 的 `LogLine` 里那个 `Text` 换成 `read-only: true` 的 `TextInput`
（Slint 的 `Text` 没有任何选区支持；`read-only` 的官方语义是"能选不能改"）。
**Rust 侧零改动、零新增依赖** —— `Ctrl+C` 的剪贴板写入由 Slint 自己做。

### 9.1 源码依据（不是猜的）

| 事实 | 出处 |
|---|---|
| 左键按下即 `GrabMouse` ⇒ 外层 `ListView`(Flickable) 抢不走拖动 | `i-slint-core-1.18.0/items/text.rs` 的 `input_event`（`MouseEvent::Pressed{Left}` 分支 `return InputEventResult::GrabMouse`） |
| `Ctrl+A` / `Ctrl+C` 在 `read_only` 下**照旧有效**（被挡的只有 Paste/Cut/Undo/Redo） | 同文件 `key_event`：`StandardShortcut::SelectAll` / `Copy` 无 `read_only` 条件；`Paste|Cut|Undo|Redo if !self.read_only()` |
| `read-only` 时插入光标不画 | 同文件 `show_cursor`：`if self.read_only() \|\| !self.has_focus() { hide }` |
| 剪贴板写的是"系统剪贴板"，且 Windows 上**没有**主选区剪贴板 | `Platform::set_clipboard_text` + `i-slint-backend-winit-1.18.0/clipboard.rs` 的 `select_clipboard`：`SelectionClipboard` → `SilentClipboardContext`（空实现） |
| `Text` 默认无障碍角色是 `text`，`TextInput` 默认是 `text-input` | `i-slint-common-1.18.0/enums.rs` 的 `AccessibleRole`（两条 doc 注释各写明"automatically applied"） |

### 9.2 离屏探针实测（`target/ui-probe-copy/`，⚠ 探针不入库，配方见 §9.5）

| 项 | 结果 |
|---|---|
| **行几何：改动前 vs 改动后** | **逐字段一致**：三行墨迹带 `y 393..404 / 413..424 / 433..444`（高 12，行距 20），左边缘 `x=30`，宽度 83/77/96，墨色 `#BCC5F7`（命令）/`#7FB4E4`（成功）/`#F07178`（错误）。⇒ 换 `TextInput` **没有动排版** |
| **长行（375 字）** | 改动前后同一条：`y 393..404  x 30..416 (w 387)  px 1720` ⇒ 长行仍是"按内容宽度画出去、被卡片裁掉"，`TextInput` **没有**引入内部横向滚动 |
| 拖选整行 + `Ctrl+C` | **PASS**，剪贴板 = `"probe-ok-two"` —— 即该行正文，`✓ ` 前缀已剥 |
| 只拖选、不按 `Ctrl+C`（判别性对照） | **PASS**，剪贴板为空 ⇒ 上一条那串字确实是 `Ctrl+C` 写进去的，不是探针/选区自己写的 |
| 点一下 + `Ctrl+A` + `Ctrl+C` | **PASS**，剪贴板 = `"probe-err-three"`（整行） |
| 只读：打字 `X` | **PASS**，打字前后两帧 diff **0 px** ⊂ 内容没被改 |
| 选区确实画出来了 | 拖动后 vs 三行基准帧：`893 px, bbox x 30..106 y 412..423`，底色 `#101019 → #353952`（`Tokens.accent-line`）—— 正是第二行的文字区 |
| **亮色主题（`copy light`）** | A~D 同样 **ALL PASS**；行几何与暗色同结构（同 y 带、同 20px 行距），墨色换成亮色侧（`#2E6C9E` / `#9C3F41`），选区底色 `#FCFCFD → #D4D7E5` ⇒ 选区在两个主题下都可见 |
| `cargo test` / `cargo build` | **138 passed / 0 failed**；`touch src/*.rs` 后强制重编 **0 警告** |

### 9.3 待用户执行（真机端到端）

| # | 步骤 | 通过判据 | 状态 |
|---|---|---|---|
| **V-31** | ① 按住左键横向拖过日志里的一行 → `Ctrl+C` → 粘贴到记事本；② 点一行后 `Ctrl+A` + `Ctrl+C` → 粘贴；③ 换一行**只拖不按** `Ctrl+C` → 粘贴；④ 在日志行上打字 | ①② 依次得到"剥掉前缀的那行正文"与"整行正文"；③ 剪贴板**不变**（仍是上一步那份）；④ 内容不变。另：拖选时日志区**不跟着滚动**（滚轮 / 滚动条仍可滚） | ⬜ |

### 9.4 未做（诚实记录）

- **真机端到端**：探针走的是自写 `Platform`（Windows 语义：只认 `DefaultClipboard`），它证明的是
  "Slint 这条链会把选中文字写进剪贴板"，**没有**真的往 Windows 剪贴板写、再粘到记事本。
  上面那张表（V-31）就是留给这一步的。
- **跨行选择 / 复制全部**：按 SRS FR-28 v1.7 的决定**不做**。
- **探针的两处自身缺陷（已修，记下来免得下一个人重踩）**：① 组合键必须**按住**修饰键
  （`KeyPressed(Control)` → `KeyPressed("c")` → `KeyReleased("c")` → `KeyReleased(Control)`）；
  写成 `key(Control)` 再 `key("c")` 会因为中间那次 `KeyReleased` 清掉控制位而永远按不出
  `Ctrl+C`（假阴性）；② 拖选起点必须落在 `TextInput` **内部**（它的左边缘 = 文字左边缘），
  落在左边 3px 就是点在控件外，拖动没人接。

### 9.5 探针配方（重建用）

```text
target/ui-probe-copy/
  Cargo.toml    # [package] edition="2021"、空 [workspace]；slint / slint-build 1.18
                # （features 同主程序：image-default-formats；renderer-software 本就是默认 feature）
  build.rs      # slint_build::compile("../../ui/app.slint")   ← 编译真实的 app.slint
  src/main.rs   # ① 自写 Platform：create_window_adapter 交出 MinimalSoftwareWindow
                #   (RepaintBufferType::NewBuffer)，set_clipboard_text **只记 DefaultClipboard**
                #   （照抄 winit 在 Windows 的语义，见 §9.1）
                # ② MainWindow::new() → global::<Tokens>().set_dark(true) → show()
                # ③ 帧 = request_redraw + draw_if_needed(render) 连渲两帧，回读 Vec<Rgb8Pixel>
                # ④ geometry：空日志帧 vs 三行日志帧做差集 → 每行的 y 带 / x 范围 / 墨色
                # ⑤ copy：在真帧上 dispatch_event(PointerPressed → PointerMoved×8 →
                #   PointerReleased) + Ctrl+A/Ctrl+C，读回探针记下的剪贴板
# 三个模式：
cargo run -- geometry   # 行几何（改前/改后各跑一次比对）
cargo run -- long       # 375 字长行的墨迹边界
cargo run -- copy       # 四个场景 A~D，全 PASS 退出码 0
cargo run -- copy light # 同上，亮色主题（选区在两个主题下都要看得见）
# 「改动前」的帧怎么拿：`git stash push -- ui/app.slint` → 跑 → `git stash pop`
```

---

## 10. 单实例 + 静默安装自动重启（2026-09-24，SRS v1.8 / ARCHITECTURE v1.11）

对应 **FR-40**（单实例）与 **FR-39 修订**（静默安装 + 装完自动重启）。两条都有**真机探针**，
脚本留在 `target/`（不进版本库，与 §7.2 的一次性测试同款做法）。

### 10.1 自动化证据

| # | 项 | 证据 |
|---|---|---|
| U-1 | 单测 | `cargo test` **142/142** 通过（v1.7 基线 138 + 本轮 4：`setup_args_are_exact` / `second_claim_sees_the_running_instance` / `window_title_matches_slint` / `quit_turn_is_granted_only_once`） |
| U-2 | 零警告 | `cargo build --release` **0 warning**；`cargo clippy --all-targets` 的警告数 **20 → 20**（与改动前逐条对比，新增代码零警告） |
| U-3 | 判据 + 交接一次钉住 | `second_claim_sees_the_running_instance`：同名互斥体第二次 claim 必须 `None`（否则两个实例并存），且弹窗事件必须真被点亮、取走后必须回到未点亮（自动重置 —— 否则 80 ms timer 会每个 tick 重弹窗口） |
| U-4 | 参数逐字 | `setup_args_are_exact`：`["/SILENT", "/NORESTART"]` 逐字断言 |
| U-5 | 跨语言契约 | `window_title_matches_slint`：`include_str!("../ui/app.slint")` 里必须含 `title: "DSH Manager";` |
| U-6 | 退出幂等 | `quit_turn_is_granted_only_once`：第一条退出放行、此后每条作废 |

### 10.2 探针 A：单实例（`target/fr40-probe.ps1`，13 项断言）

跑法：`pwsh -NoProfile -File target/fr40-probe.ps1`（脚本自己起/杀程序，退出码 0 = 全绿）。
探针用 `EnumWindows` + `GetWindowThreadProcessId` **按 pid** 找窗口（不是 `MainWindowHandle`
—— 那个对隐藏窗口返回 0，正好用来断言"确实藏起来了"）。**连跑 3 轮，13/13 全绿**：

| 步骤 | 断言 | 结果 |
|---|---|---|
| ① 启动第一个实例 | 进程数 = 1；标题为 `DSH Manager` 的顶层窗口出现且可见 | ✅ |
| ② 点 X（`WM_CLOSE`，`state.json` 里 `close_behavior=hide`） | 进程仍在（隐藏 ≠ 退出）；窗口 `IsWindowVisible` = 假 | ✅ |
| ③ **再次启动**（隐藏状态） | 仍只有 1 个进程；第二个进程**自己退出**（实测耗时 **0.08–0.10 s**）；窗口重新可见；**窗口在前台**（`GetForegroundWindow` = 它） | ✅ |
| ④ **再次启动**（最小化状态） | 仍只有 1 个进程；窗口已还原（`IsIconic` = 假）；窗口在前台；第三个进程自己退出 | ✅ |
| ⑤ 收尾 | 杀掉程序 | ✅ |

**★ 第 ④ 步是第一版实现失败的地方**（判别性的一步）：窗口**从隐藏到显示**全绿，**从最小化
还原**却只做到了"看得见、不在最前" —— `SetForegroundWindow` 被 Windows 前台锁拒绝，因为
真正调它的第一个实例是**后台进程**。修法是让**第二个实例**（用户这次点击拉起来的、持有
前台权利的那一方）在 `SetEvent` **之前**调一次 `AllowSetForegroundWindow(ASFW_ANY)`。
改完三轮全绿。这条没有别的办法能抓到 —— 单测测不到前台锁，只有真机 + 真实的双击时序能。

### 10.3 探针 B：静默安装 + 自动重启 + 装进已有目录（`target/fr39-probe.ps1`，12 项断言）

跑法：先 `cargo build --release`，再把安装包编译到**临时目录**（不碰 `dist/`：

```text
ISCC.exe --output-dir="C:\Mine\dsh-manager\target\fr39-dist" installer\dsh-manager.iss
```

—— ⚠ 别让这次编译覆盖 `dist/` 里那份已发布、已记过哈希的 v0.4.0 安装包）。

⚠ **构建时要带 `RC`**（本轮踩到的坑）：本机没装 Windows SDK，`build.rs` 靠 PATH 上的
`llvm-rc` 嵌图标与版本资源，而 PATH 上并没有它 —— 于是构建**静默**产出一个没有版本资源的
exe（`build.rs` 按设计只告警、不失败），接着 ISCC 读 `GetFileVersionString` 拿到空版本，
安装包连名字都会变成 `dsh-manager-v-windows-x64-setup.exe`。判别方法是一行命令：

```powershell
(Get-Item target\release\dsh-manager.exe).VersionInfo.FileVersion   # 必须是 0.4.0
```

本机可用的资源编译器在 Qt 的 llvm-mingw 里，用 `build.rs` 自己支持的 `RC` 变量指过去即可
（**不要**为它改 PATH —— 那个 bin 里有 `ld.lld.exe` 之类，改 PATH 有撞上链接器的风险）：

```bash
touch build.rs && RC="C:\Software\Qt\Tools\llvm-mingw2217_64\bin\llvm-rc.exe" cargo build --release
```

（`touch build.rs` 是必需的：`build.rs` 没有声明 `rerun-if-env-changed=RC`，不碰它就不会
重跑构建脚本。）

脚本先跑 `cp target/installed-backup.exe "C:\Software\DSH Manager\dsh-manager.exe"` 把**旧二进制**
放回去（否则"装上了"这件事不可判别），然后：

| 步骤 | 断言 | 结果 |
|---|---|---|
| 起点 | 装的是**另一份**二进制（哈希不同）；登记表安装位置 = `C:\Software\DSH Manager\` | ✅ |
| ① 按用户的方式把旧版跑起来 | 进程数 = 1 | ✅ |
| ② **静默安装**（`/SILENT /NORESTART`，与 `launch_setup` 逐字相同） | 退出码 **0**，耗时 **1.3–1.6 s** | ✅ |
| ③ **装完自己回来** | 新进程出现（pid 与旧的不同）；安装日志里有 `-- Run entry -- … Filename: C:\Software\DSH Manager\dsh-manager.exe` 与 `Run as: Original user`；主窗口已显示 | ✅ |
| ④ **装进已有目录** | exe 哈希 = 新构建；登记表安装位置**没变**；`%LOCALAPPDATA%\Programs` / `Program Files` 下**没有**多出一份；目录里仍是 `dsh-manager.exe` + `unins000.exe` | ✅ |

安装日志（`target/fr39-setup.log`）里判别性的三行：

```text
Installation process succeeded.
-- Run entry --
Run as: Original user
Filename: C:\Software\DSH Manager\dsh-manager.exe
Attempting to restart applications.
```

`-- Run entry --` 那三行就是"用户一下都没点、程序自己回来了"的证据：那条 `[Run]` 上挂的正是
`skipifnotsilent`，在 `/SILENT` 下**本该被跳过**的条目真的执行了。

### 10.4 ★ 本轮实测出来的一个真问题：Restart Manager 会把静默安装卡住 30 s

第一版探针是**失败**的，而且失败得很有价值：安装包 **180 s 没返回**。日志停在

```text
Shutting down applications using our files.      ← 12:00:32.606
Some applications could not be shut down.        ← 12:01:02.717（整整 30 s 后）
Message box (Abort/Retry/Ignore): Setup was unable to automatically close all applications…
```

根因：`CloseApplications=yes` 会用 Restart Manager **关停**占用待替换文件的程序（静默模式下
不给用户任何选择），而本程序**点 X 的行为是"隐藏"、不是退出** —— RM 发 `WM_CLOSE`、
窗口藏起来、进程还在，于是 RM 等到超时。这是**既有的**交互（v0.4.0 的可见向导同样会撞上，
只是那时用户能看到"请关闭这些程序"的页面并手动关掉），静默模式把它变成了"没人能回答"。

对**真实更新路径**无影响，理由是实测出来的两个时间：

| 量 | 实测 |
|---|---|
| Setup 从进程创建到 "Shutting down applications" | **0.40 s**（`12:00:32.208` → `12:00:32.606`） |
| 本程序从 spawn 安装程序到自己退出 | **0.15–0.24 s**（80 ms timer 交接 + 实测 `WM_CLOSE → 进程消失` 132/133/139/140/146 ms，中位数 **139 ms**） |

也就是说本程序总是**先走一步**。探针 B 也是照这个时序跑的（spawn 后 300 ms 杀掉旧实例，
比真实的 0.2 s **更晚**，即更严格）。

**顺带钉出一个真洞并修掉**：RM 关停时那个 `WM_CLOSE` 可能撞上已经在飞的 FR-39 退出。
没有闸门时，它会按用户的「关闭行为 = 彻底退出」走一趟 `make_quit(true)` —— 于是把用户正在
用的 `dsh web` 会话 `taskkill` 掉，正好推翻 FR-39 那句"装更新不停 `dsh web`"（V-29 的判别性
一步）。修法是 `AppState.quitting` + `take_quit_turn`：退出只走一次，先到的那条说了算（U-6）。

### 10.5 探针自身的坑（写给下一位验证者）

`Stop-Process` 杀掉外层 `SetupLdr` **不会**带走它解包出来的**内层 setup**（临时目录里的
`…setup.tmp`）。残留的内层进程一直占着 `/LOG` 那个文件，下一轮 Setup 建不出日志就会弹错误框
等人点 —— 表现为"莫名其妙卡住"。本轮踩过一次，排查方式是列进程：

```powershell
Get-CimInstance Win32_Process | Where-Object { $_.Name -match "setup|dsh" } |
  Select-Object ProcessId, ParentProcessId, Name
```

`fr39-probe.ps1` 的起点现在自己会清这一类残留。**这是测试环境的坑，不是产品行为**。

### 10.6 待用户执行（真机端到端）

| # | 对应 | 步骤 | 通过判据 | 状态 |
|---|---|---|---|---|
| **V-32** | FR-40 | 隐藏到托盘 / 最小化 / 正常开着，各再双击一次图标 | 始终只有一个 `dsh-manager.exe` 与一个托盘图标；窗口重新出现、还原、到前台 | ✅ §10.2 探针覆盖三种状态（连跑 3 轮）；§10.8 又在装好的发布版上复测了一次 |
| **V-33** | FR-39 修订 | 在装着旧版的机器上点〔下载并安装〕，中途一次都不点 | 出现的是**进度窗**而不是向导；程序退出后**自己回来**；安装位置不变、没有多装一份 | ✅ §10.3 覆盖安装包那一半；§10.8 覆盖"从更新框点下去"那一半（下载物与装完的 exe 都与 Release 逐字节一致） |

### 10.7 本轮**没有**跑的一项

更新框里的说明文案改长了一行（`ui/app.slint` 的那句"下载完成后会静默安装到当前目录…"）。
卡片是自适应高度（`Bezel { width: 420px }` —— 只钉了宽、没有固定高，`Text` 是 `word-wrap`），
所以预期只是卡片长高约一行；但**这一条是推断，不是实测**，与 §7.3 / §9 那两次离屏探针不同。
要补的话按 §9.5 的配方跑一次 `ui-probe-*`（它 `build.rs` 编译的是**真实的** `ui/app.slint`），
量卡片几何并与改动前对比即可。

### 10.8 发布后复验（2026-09-24，v0.5.0 已上线）

v0.5.0 上线后，本机（当时装着 0.4.0 的开发构建）**真的走了一遍完整更新流程** —— §10.6 里
那两项"要等一个比当前更新的 release"（V-29 / V-33 的"从更新框点下去"那一半）就此闭合：

| 证据 | 值 |
|---|---|
| FR-39 的落盘位置 | `%TEMP%\dsh-manager-update\dsh-manager-v0.5.0-windows-x64-setup.exe`（13:37:04） |
| 下载到的安装包 | SHA256 `84f1a7b8c8243e58cfb8c7e902fa4ee4d977f3ced56d67ece910dc91ccee7f01` —— 与 Release 的 `SHA256SUMS.txt` 里那一行**逐字符一致** |
| 装完之后的 exe | `C:\Software\DSH Manager\dsh-manager.exe`，FileVersion **0.5.0**，SHA256 `830f2e9596456e6f2d64d5bfc13443469eeabdfe3c42dc4109aad5374c48a5e2` = Release 里 `dsh-manager-v0.5.0-windows-x64.exe` 那一行 —— **逐字节就是发布物本身** |
| 登记表 | `HKCU\…\{8F3C1A64…}_is1` 的 `DisplayVersion` = 0.5.0，`InstallLocation` 仍是 `C:\Software\DSH Manager\`（**没有**另建目录） |
| 自动重启 | 更新后程序自己起来了（否则不会有 0.5.0 在跑）；另外在**装好的那份发布版**上复测了单实例：连开两次仍只有 1 个进程、第二个进程 1.11 s 内自己退出、已有窗口被激活 |
| 人工点击次数 | **1**（就是〔下载并安装〕那一下） |

⚠ **一处没能取证**：V-29 的判别性一步"装更新时 `dsh web` 仍在监听"。这次跑的时候本机没有在
跑 `dsh web`（`state.json` 的 `running_port` 是 null），所以那一条目前只有设计依据
（§4.9.4 的退出语义分叉）与 `quit_turn_is_granted_only_once` 这条单测，**没有真机证据**。
下次带着 `dsh web` 跑一次更新即可闭合。

⚠ **一条环境事实**（与探针脚本有关）：`gh release download` 走的是
`release-assets.githubusercontent.com`，在本机**不可达**（dial tcp 超时）；加
`HTTPS_PROXY=http://127.0.0.1:12450` 才通。这与 SRS §2.2.7 / GC-4 记的是同一件事，也正是
本程序下载走 `assets[].url`（api.github.com）而不是 `browser_download_url` 的原因 ——
而本次真机更新恰好证明了那条路是通的。

⚠ **一次没跑成的探针**：本轮还写了一个 `target/fr39-e2e.ps1` —— 用 UI Automation 的
`Invoke` 点〔下载并安装〕（全程只此一次人工操作），然后断言"本程序退出 → 自己回来 →
FileVersion / 哈希"。要跑它的时候，机器上**已经是 0.5.0**（就是上面那次真实更新的结果），
"起点必须是 0.4.0"这条判别性前提不成立，于是它按设计在第一步就停下、没有点任何按钮。
要复跑：先把 `target/installed-backup.exe`（v0.4.0 的发布物）拷回安装目录，
再 `pwsh -NoProfile -File target/fr39-e2e.ps1`。

顺带一条**探针脚本的坑**（与 §10.5 同类）：这个脚本在起点做的是"杀掉所有 dsh-manager"，
而单实例上线之后，**程序自己就不是随便能杀干净的了** —— 杀完要留出一点时间让互斥体释放，
否则紧接着启动的那次会把自己当成第二个实例、直接退出（表现为"应用起不来"）。

### 10.9 探针 C：装完清掉安装包（`target/fr39-clean.ps1`，5 项断言）

用户要求"安装完成后删除安装包"（v0.5.0 发布后提的）。真机现状正是动机：到 13:37 为止
`%TEMP%\dsh-manager-update\` 里已经攒了**两份** 8.3 MB 的安装包（v0.4.0 与 v0.5.0 各一份）。

跑法：`pwsh -NoProfile -File target/fr39-clean.ps1`（对着**新构建**的
`target\release\dsh-manager.exe` 跑 —— 装好的那份 0.5.0 里还没有这个改动）：

| 步骤 | 断言 | 结果 |
|---|---|---|
| ① 造残留 | 目录 + 假安装包 + 一个子目录 | ✅ |
| ② 启动 | **整个目录被删掉**；日志面板里出现 `已删除上次更新留下的安装包：C:\Users\xueyu\AppData\Local\Temp\dsh-manager-update` | ✅ |
| ③ 反面（没有残留时再启动一次） | 日志里**一个字都不提**清理（否则每次启动都多一行噪音）；启动日志照旧 | ✅ |

配套单测 `clean_dir_reports_only_when_it_removed_something` 钉住同一件事的两个方向：
不存在 → 不吭声；存在 → 连目录一起删掉并报一句（用**测试专属**目录名，绝不碰真的
`%TEMP%\dsh-manager-update\` —— 那里面可能正躺着用户等着安装的那份）。

⚠ 探针自己踩的一个坑：失败分支的说明串被 PowerShell **提前求值**（`Check` 的第四个参数是
实参），于是"朝已经删掉的目录 `Get-ChildItem -Recurse`"直接把脚本打断了 —— 报的还是一个
风马牛不相及的临时文件 `Access denied`。失败路径的文案要么写成常量，要么用脚本块延迟求值。

---

## 11. 通道勾选与"已是最新"的判据（2026-09-24，SRS v1.8 / ARCHITECTURE v1.11）

用户报的 bug：**"dsh 目标版本中出现了 0.1.7-rc.1 但当前版本显示的是 0.1.7-alpha.2 已是最新。"**
修法（用户裁决）：**保留通道概念，改成"检测哪些通道的更新"的勾选框，默认全勾**。

### 11.1 根因（先复现，再动手）

判据 `is_up_to_date()` 拿**已装版本所在通道内**的最新版比较（FR-8 的原始定义），而版本下拉列的是
**全目录**（不带通道过滤）。2026-09-24 的真实数据第一次让两者打架（registry 实测：
`alpha = 0.1.7-alpha.2`、`next = 0.1.7-rc.1`、`latest = 0.1.5-rc.3`；`0.1.7-rc.1` 发布更晚、
semver 更大）—— 本机 `dsh --version` 恰好就是 `0.1.7-alpha.2`，于是现场就能复现。

**真机 UIA 取证**（`target/repro-channel.ps1`，修之前）：

```text
[当前版本] [0.1.7-alpha.2] [已是最新]      ← 而下拉里列着 0.1.7-rc.1
```

为什么当年这么写也查清了：FR-8/GC-14 的动机是真的（npm 的 `latest` tag `0.1.5-rc.2` 比已装的
`0.1.6-alpha.2` **更旧**），但"按通道过滤"只是"防止把用户往下带"的一个**过度近似**；68a95bd
那次修复已经把判据改成"只认严格更新"（`>=`），防降级于是不再需要靠过滤保证 —— 过滤剩下的
唯一作用就是**把确实更新的版本藏起来**。

### 11.2 改法与自动化证据

| # | 项 | 证据 |
|---|---|---|
| U-1 | 单测 | `cargo test` **151/151**（v1.8 基线 143 + 本轮 8：`model::channels_*` 2 条、`pm::newest_among_*` 3 条（含 GC-14 防降级改写）、`config::update_channels_roundtrip_*` 1 条、`main::` 判据层 3 条） |
| U-2 | 零警告 | `cargo build --release` **0 warning**；`cargo clippy --all-targets` 警告数 **20 → 19**（少的那条是随 `latest_in` 一起删掉的显式生命周期） |
| U-3 | ★ 判别性（判据层） | `next_stage_of_the_same_line_counts_as_an_update`：已装 `0.1.7-alpha.2` + 目录含 `0.1.7-rc.1`，**全勾**必须 `!is_up_to_date()`；`older_only_catalog_still_reads_as_up_to_date`：目录只有更旧的版本时仍判"已是最新"（GC-14 不变量没被改坏） |
| U-4 | 勾选真的起作用 | `channel_selection_gates_the_verdict`：只勾 alpha → 判"已是最新"（= v0.4.0 的老行为，现在是显式选择）；一个都不勾 → 判"已是最新"而不是空白或乱报 |
| U-5 | 纯函数 | `pm::newest_among`：全勾取目录最大、按勾选过滤、`0.1.10-rc.1 > 0.1.5-rc.2` 的 semver 序（不是字符串序）、`Channel::Other` 永远在范围内 |
| U-6 | 持久化的坑 | `config::update_channels_roundtrip_and_absent_is_none`：**缺失 = 没记过、空数组 = 明确一个都不勾**，两者必须分得开（否则取消勾选之后重启会被改回全勾）；非数组按没记过；数组里的非字符串项丢掉 |

### 11.3 真机探针（`target/fr8-probe.ps1`，8 项断言，全绿）

对着新构建的 `target\release\dsh-manager.exe` 跑，本机已装 `0.1.7-alpha.2`：

| 步骤 | 断言 | 结果 |
|---|---|---|
| ① 起点 | `state.json` 里没有 `update_channels`（= 没记过 = 全勾） | ✅ |
| ② **缺省全勾** | 徽标 = **可更新**（这就是用户报的那个 bug 被修掉的地方） | ✅ |
| ③ **取消勾选 RC + 稳定版** | 设置面板里正好 3 个勾选框（UIA 的 CheckBox 角色，位置 `372,591 / 500,591 / 613,591`）；`state.json` 落成 `"update_channels": ["alpha"]`；徽标回到**已是最新** | ✅ |
| ④ **重新勾回 RC** | 落成 `["alpha","rc"]`；徽标又变回**可更新** | ✅ |
| ⑤ 截图 | `target/fr8-settings.png`（设置面板新增的「更新通道」一节） | ✅ |

截图里还有一处**顺带的自证**：背景的操作卡上「目标版本」下拉现在默认选中 **`0.1.7-rc.1`** ——
v0.4.0 那一刻它默认选中的是已装的那个 `0.1.7-alpha.2`（也就是"最新版"，而徽标同时说"已是最新"）。

### 11.4 本轮**没有**做的

- **没跑离屏 UI 探针**：新增的一节设置面板是靠真机截图 + UIA 几何确认的（3 个勾选框的实际矩形），
  没有按 §9.5 的配方量像素。卡片是自适应高度（`Bezel` 只钉了宽），多出的一节是"Eyebrow + 一行
  勾选框 + 一段小字"，截图里**未越界、未被裁切**。
- **`Channel::Other` 没有勾选框**（设计如此）：它永远参与检测。真要暴露给用户需要先有 dsh 发布
  `beta` 之类版本的真实需求。
