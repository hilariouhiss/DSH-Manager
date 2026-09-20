# DSH Manager 验证记录

对应 SRS v1.2 §8.2 / §8.3 的验证清单。日期：**2026-09-20**。

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

1. **代理只认环境变量**：`ureq` 的默认 feature 集**不做系统代理发现**，只读 `HTTP_PROXY` / `HTTPS_PROXY`；
   GC-2 禁止为此引入读注册表 / WinINET 的依赖。因此两个出网函数（`dsh::fetch_catalog` / `dsh::fetch_notes`）
   的错误串尾部都挂了 `PROXY_HINT`（"若本机仅允许通过系统代理出网，请设置 HTTPS_PROXY 后重启本程序"）。
   本机实测就撞上过这个组合：直连 `api.github.com` 得 HTTP 403（出口 IP 配额），而系统代理返回 200 ——
   即**只有本程序会失败**，所以失败信息必须自己解释原因。
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

