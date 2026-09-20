# DSH Manager 验证记录

对应 SRS v1.2 §8.2 / §8.3 的验证清单。日期：**2026-09-20**。

被验证的代码：`feat/implementation` @ **`d8114ad`**（Task 19 的收尾提交）+ Task 20 的工作树改动
（`src/main.rs` / `src/model.rs` / `src/pm.rs` / `src/txn.rs` / `src/dsh.rs` / `src/config.rs`，
见本次提交信息）。构建：`cargo build` **0 警告**，`cargo test` **0 警告 / 70 passed**（`cargo clean -p`
后强制全量重编译）。

**证据来源图例**（每行都标明来源，不混用）：

| 标记 | 含义 |
|---|---|
| **T20 实测** | 本次 Task 20 会话中在**本机实测**，用 UI Automation 读回 + `netstat` + `state.json` 观测 |
| **T19 实测** | Task 19 报告记录的实测（提交 `d8114ad`，同一份已评审代码；本次未重复执行） |
| **T18 实测** | Task 18 报告记录的实测（提交 `8f00bb4`，`SetWinEventHook` 控制台窗口对照） |
| **单测** | `cargo test` 中的具名单元测试（本次 70 项全绿） |

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
| **V-14** 换版本事务（同 PM） | **未执行** —— 原因见下 | 需要真的执行 `npm install -g @deepseek-ai/dsh@0.1.6-alpha.1`：那会**降级用户正在使用的真实 dsh 安装**（本机唯一的 npm 那份，也是本会话所依赖的 CLI），事务中断就会把用户环境留在半成品状态。**替代证据（单测）**：`txn::tests::same_pm_version_change_commits_and_skips_uninstall`（FR-13：同 PM 跳过 S3）、`model::tests::pm_command_table_is_exact`（钉住每个 PM 的确切 argv）、`txn::tests::cross_pm_migration_installs_then_uninstalls`（装/卸顺序）。**未观测**：GUI 上一次真实事务的端到端运行。 |
| **V-15** 迁移事务（npm → pnpm） | **未执行** —— 这是简报 Step 2，**已由控制器裁定不做** | 原因：`Job::Transact` 先按 TR-1 停掉 `dsh web`（本机就是**承载本次验证的 live 会话**，pid 13432），随后事务会 `npm uninstall` 掉 `@deepseek-ai/dsh` —— 半途失败会同时毁掉用户的环境与本次会话。**替代证据**：事务引擎语义由 **20 个单元测试**钉住（TR-4 / TR-5 / TR-6 / TR-11 及 V-17 / V-19 / V-20 / V-21），且"真实事务"代码路径已在 Task 18/19 通过 dry-run 后端（`txn::FakeBackend`）走过一遍，**零环境变更**。 |
| **V-16** 事务前置检查 | **未执行（GUI 级）** —— 本机**无法构造**该场景 | 三条拒绝路径由单测逐条覆盖：`txn::tests::precheck_rejects_pm_bin_not_on_path`（TR-3）、`precheck_rejects_unavailable_pm`（TR-2）、`precheck_rejects_unsafe_version`（NFR-6），并由 `rejected_target_produces_no_side_effects` 断言**零副作用**。**为什么 GUI 级构造不出来**：TR-3 比较的是"该 PM 的 `bin -g` 输出目录是否在 PATH 上"，而 PM 的 shim 恰好就住在那个目录里（`C:\Users\xueyu\AppData\Local\pnpm\bin\pnpm.cmd`）—— 把该目录移出 PATH 后，`probe_env` 连这个 PM 都探不到，根本走不到 `precheck`。要造这个场景只能改真实 PATH 或让它真跑一次事务，两者都不在允许范围内。 |
| **V-17** 事务失败补偿 | **未执行（GUI 级）** —— 需要一次真实失败事务 | 单测覆盖补偿全部关键分支：`txn::tests::v17_s3_failure_rolls_back_to_origin`（状态回到 `(PM_old, V0)`）、`c2a_cleanup_failure_is_reported_as_degraded_with_cause`、`c3_detects_incomplete_recovery_and_degrades`、`degraded_outcome_carries_runnable_manual_commands`、`tr11_compensation_probes_before_acting`。**未观测**：真实 PM 上的回滚。 |
| **V-18** 事务期间 UI | **未执行** —— 需要一次真实事务在跑 | 未取得观测证据。代码侧：`busy` 由 `project()` 投影为 `enabled: !root.busy`（`ui/app.slint:137` 的 `安装` 按钮、`:132` 版本下拉），日志经 `VecModel` 在 UI 线程追加（`LOG_CAP` 裁剪），排空靠 80 ms timer —— 但"按钮禁用、日志实时滚动、界面不卡死"这三条**本轮没有实测**。 |
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
| **V-20** TR-6：先装后卸 | **PASS**（单测） | `txn::tests::v20_s1_failure_touches_nothing`：构造 S1 安装失败，断言**任何 uninstall 都没有被执行**（npm 那份分毫未动）。顺序本身由 `txn::tests::cross_pm_migration_installs_then_uninstalls` 断言（先装后卸）。SRS 要求的"确认 dsh 仍完全可用"是真实环境断言，本轮未执行（同 V-14/V-15 的理由）。 |
| **V-21** TR-5：补偿前探测 | **PASS**（单测） | `txn::tests::v21_origin_broken_does_not_blindly_uninstall_target`：PM_old 已损坏 + S3 失败时**不会**盲目卸载 PM_new；`txn::tests::tr11_compensation_probes_before_acting` 另钉住"补偿动作前必须先探测"。 |
| **V-22** FR-25：双实例状态同步 | **部分执行 / 未完成** | **窗口 → 托盘**方向有观测：T19 的 V-10 在窗口驱动状态变化后读托盘菜单项 `IsEnabled`（运行中 ↔ 已停止两侧都读到了正确的启用关系）。**托盘 → 窗口**方向本轮**未执行**：锁屏会话下无法用 UIA 识别我们自己那个弹出菜单里的菜单项（详见「验证方法说明」第 4 条），因此没法可靠地选中 `启动 dsh web`。本轮实际观测到的托盘→UI 通路只有**可见性**那一条：点 `隐藏到托盘` 后窗口消失，posted `WM_TRAYICON`+`WM_LBUTTONUP` 后窗口重新可见。代码侧：`project()` 把同一份状态**同时**推给 `win` 与 `tray`（FR-25 存在的理由就是把两次赋值放在同一个函数里）。**待解锁桌面复验。** |

---

## 未通过项与处理

**没有失败项。** 以下是**未执行**项及其处置，逐条列出以便追溯（不留空、不跳过）：

| 编号 | 现象 | 处理 |
|---|---|---|
| V-14 换版本事务 | 未执行：真实 `npm install -g` 会降级用户正在使用的 dsh 安装（本机唯一那份，也是本会话依赖的 CLI） | 由单测钉住（`same_pm_version_change_commits_and_skips_uninstall`、`pm_command_table_is_exact`）；**真实事务的端到端运行仍未被任何人验证过**，属已知缺口 |
| V-15 迁移事务 | 未执行：控制器裁定（TR-1 会先停掉承载本次会话的 `dsh web`，随后 `npm uninstall` 掉 dsh 本身） | 20 个事务单测 + Task 18/19 的 dry-run 后端；`docs/VERIFICATION.md` 即本条记录 |
| V-16 前置检查 | 未执行（GUI 级）：本机 PM shim 与它的 `bin -g` 目录是同一个目录，移出 PATH 后连 PM 都探不到，构造不出该场景 | 三条拒绝路径 + 零副作用由 4 个单测覆盖 |
| V-17 失败补偿 | 未执行：需要一次真实的 S3 失败（= 动用户的安装） | 5 个补偿单测覆盖回滚/降级/手动命令 |
| V-18 事务期间 UI | 未执行：需要一次真实事务在跑 | 无观测证据；仅代码路径（`busy` → 按钮禁用、`VecModel` 日志、80 ms 排空）。**这是本轮最明显的验证缺口** |
| V-22 双实例状态同步 | 部分执行：托盘 → 窗口（状态）方向未执行（锁屏桌面无法识别弹出菜单项） | 窗口 → 托盘方向由 T19 的 V-10 覆盖；托盘 → UI 的可见性通路本轮实测；**待解锁桌面复验** |
| V-10 托盘菜单状态 | 本轮未重复执行（T19 已 PASS） | 本轮试图复读菜单项时 UIA 返回 0 个 `MenuItem`，方法学记录见下 |

---

## 已知限制与环境假设

1. **代理只认环境变量**：`ureq` 的默认 feature 集**不做系统代理发现**，只读 `HTTP_PROXY` / `HTTPS_PROXY`；
   GC-2 禁止为此引入读注册表 / WinINET 的依赖。因此两个出网函数（`dsh::fetch_catalog` / `dsh::fetch_notes`）
   的错误串尾部都挂了 `PROXY_HINT`（"若本机仅允许通过系统代理出网，请设置 HTTPS_PROXY 后重启本程序"）。
   本机实测就撞上过这个组合：直连 `api.github.com` 得 HTTP 403（出口 IP 配额），而系统代理返回 200 ——
   即**只有本程序会失败**，所以失败信息必须自己解释原因。
2. **托盘的 `Starting` 态不渲染**：`ui/app.slint` 没有 `web-starting` 属性，`web-running` 在 `Starting`
   期间仍为 false → 从点击到就绪（正常约 2 s，超时路径最长 20 s）**"启动"按钮一直可点**。守卫
   （`start_pending` + 状态检查）会拒绝重复请求并写一条日志/状态栏（"已在启动或运行中，忽略重复的启动请求"），
   所以不会造出第二个子进程 —— 但用户看到的是"点得动、被拒绝"，不是按钮变灰。
3. **被采用的外部实例没有存活监控**：孤儿恢复认领的（以及预检发现的外部）实例只能靠用户点 `停止` 或它自己退出；
   `dsh::spawn_web` 的三个监督线程只属于**本程序自己启动**的实例，外部实例没有对应的 `WebExited` 通知，
   它的状态因此可能长期停留在 `运行中`（点 `停止` 时由 `tasklist`/`netstat` 现场兜底）。
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
