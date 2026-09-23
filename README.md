<img src="assets/logo.png" alt="DSH Manager" width="96">

# DSH Manager

[![CI](https://github.com/hilariouhiss/DSH-Manager/actions/workflows/ci.yml/badge.svg)](https://github.com/hilariouhiss/DSH-Manager/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/hilariouhiss/DSH-Manager)](https://github.com/hilariouhiss/DSH-Manager/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6)](#系统要求)

把 [DeepSeek Harness](https://www.npmjs.com/package/@deepseek-ai/dsh)（`dsh` CLI）的**版本管理**与 **web 服务启停**从命令行搬到系统托盘 —— 常驻托盘、没有黑窗口。

## 解决的三个痛点

| 痛点              | 手工操作                                               | DSH Manager                                          |
| ----------------- | ------------------------------------------------------ | ---------------------------------------------------- |
| **P1** 版本不透明 | 敲 `dsh --version`，再 `npm view` 对比才知道有没有更新 | 启动即拉取版本目录，显示已装版本、通道最新与更新说明 |
| **P2** 升降级不便 | 记全 `npm install -g @deepseek-ai/dsh@0.1.7-alpha.1`   | 下拉选版本 →〔安装〕；事务化执行 + 失败补偿          |
| **P3** 黑窗口常驻 | `dsh web` 占一个前台终端，关掉即服务停止               | 后台启动、托盘托管，端口占用先探测再动手             |

## 功能

- **版本管理**：扫描 npm / pnpm / bun / yarn，按 PATH 解析顺序判定**真正在生效**的那一份 `dsh`（owner PM），据此安装/换版本。同版本重装会先弹确认。
- **`dsh web` 生命周期**：端口可改（缺省 3080），启动前先探测端口占用（本程序 / 外部进程两种提示），显示运行状态与访问地址，可启动 / 停止 / 打开网页。
- **插件管理**：列出 web profile 的第三方插件（规格 / 已装 / 最新 / 可更新），支持安装、单个与全部更新、卸载。**一切变更经 `dsh plugin --profile web …` 转发**，本程序不写 profile 的任何文件。
- **更新说明**：选中版本即拉取 DSH 对应 release 的说明并渲染 markdown；拉不到时降级为"该版本无更新说明"（npm 上的版本并非都有 release）。
- **日志面板**：所有命令原文与子进程实时输出（上限 2000 行），`npm install` 那几十秒不再是无反馈的等待。
- **托盘常驻**：关闭窗口 ≠ 退出（首次关闭会问一次，答案可在设置里改）。托盘菜单随状态启用/禁用。
- **设置**：关闭行为（隐藏至托盘 / 彻底退出 / 每次询问）、主题（浅色 / 深色 / 跟随系统）、网络代理（使用系统代理 / 直连）。

## 界面

```
┌─ DSH Manager ───────────────────────────────────────────────────────────────┐
│  文件   帮助                                                                 │
├──────────────────────────────────────┬───────────────────────────────────────┤
│ 包管理器 npm                         │ 插件   6 个 · 1 个可更新              │
│ 当前版本 0.2.0 ﹝已是最新﹞          │ 〔@scope/name@1.2.3〕 〔安装〕        │
│ 目标版本 〔 0.2.0       ▾ 〕         │                                       │
│──────────────────────────────────────│───────────────────────────────────────│
│                     〔安装〕〔刷新〕 │ @hilariouhiss/dsh-codegraph           │
│                                      │ ^1.1.0→1.2.0    〔更新〕〔卸载〕      │
│──────────────────────────────────────│                                       │
│ ● 运行中  http://127.0.0.1:3080      │ @hilariouhiss/dsh-colgrep             │
│ 端口〔3080〕〔启动〕〔停止〕〔打开〕 │ 1.2.1  已是最新    〔卸载〕           │
│──────────────────────────────────────│───────────────────────────────────────│
│ 输出                                 │              〔全部更新〕〔刷新〕     │
│ > npm.cmd install -g                 │                                       │
│ >   @deepseek-ai/dsh@0.2.0           │                                       │
│ …（上限 2000 行）                    │ 更新说明 · v0.2.0                     │
│                                      │ 新增功能                              │
│                                      │ · 支持第三方插件管理…                 │
├──────────────────────────────────────┴───────────────────────────────────────┤
│ 就绪                                                                         │
└──────────────────────────────────────────────────────────────────────────────┘
```

左栏是“换 `dsh` 版本 + 跑 `dsh web`”的操作区与日志，右栏是“插件 + 更新说明”的只读区；菜单栏为 文件（设置 / 退出）与 帮助（关于 DSH Manager）。

## 安装

从 [Releases](https://github.com/hilariouhiss/DSH-Manager/releases) 下载 `dsh-manager-v<版本>-windows-x64-setup.exe` 双击安装（Inno Setup 6）：

- 缺省**仅为当前用户**安装到 `%LOCALAPPDATA%\Programs\DSH Manager`，全程不弹 UAC；向导里可以改成"为所有用户安装"并自定义目录。
- 卸载项与桌面快捷方式（可选）齐备；安装包**不碰** `%APPDATA%\dsh-manager\` 下的配置，升级与卸载都不会毁掉你的端口、主题与代理设置。
- 同一 Release 里还有免安装的 `dsh-manager-v<版本>-windows-x64.exe`（单文件、拷走就能跑）与 `SHA256SUMS.txt`。

### 系统要求

- Windows 10 / 11 x64。
- 至少装一个包管理器（npm / pnpm / bun / yarn）—— 本程序通过它们管理 `dsh`。
- 拉取版本列表与更新说明需要能访问 `registry.npmjs.org` 与 `api.github.com`（支持 Windows 系统代理，可在设置里关掉）。

## 数据文件

| 路径                                                   | 内容                                                       |
| ------------------------------------------------------ | ---------------------------------------------------------- |
| `%APPDATA%\dsh-manager\state.json`                     | 端口、关闭行为、主题、代理开关（原子写入，损坏时回退缺省） |
| `%APPDATA%\dsh-manager\notes-cache.json`               | 更新说明缓存（与配置文件分开：改端口不必重写几 KB 的正文） |
| `%DSH_HOME%\profiles\web`（缺省 `%USERPROFILE%\.dsh`） | dsh 自己的 web profile —— 插件列表的**只读**来源           |

本程序不读取、不存储、不传输任何凭据，也不监听任何端口。

## 从源码构建

前置：Rust ≥ 1.92（Slint 1.18 的下限；本项目用 edition 2024）。

```powershell
cargo build --release          # → target\release\dsh-manager.exe
cargo test                     # 单元测试
```

图标与版本信息在构建期由 `build.rs` 嵌进 PE：它找 `rc.exe`（Windows SDK）或 `llvm-rc.exe`（LLVM），可用 `RC` 环境变量显式指定；**找不到只告警、不失败** —— 但那样 exe 里就没有版本资源，安装包会拿不到版本号。

打安装包（需 [Inno Setup 6](https://jrsoftware.org/isinfo.php)）：

```powershell
& "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer\dsh-manager.iss
# → dist\dsh-manager-v<版本>-windows-x64-setup.exe
```

**版本号只写一次**，在 `Cargo.toml`：`build.rs` 把它注入 exe 的 `VS_VERSION_INFO`，`.iss` 再用 `GetFileVersionString()` 从 exe 读回来 ⇒ `Cargo.toml` → exe → 安装包三者不可能漂移。

## 发布新版本

改 `Cargo.toml` 的 `version` 并提交，然后打 **注解 tag** 推上去：

```powershell
# 注解 tag（-a）的说明会直接成为 Release 正文 —— 按 changelog 写；轻量 tag 会退回用提交说明
git tag -a v0.3.0 -m "DSH Manager v0.3.0 —— 这次发了什么"
git push origin main --tags
```

[`.github/workflows/release.yml`](.github/workflows/release.yml) 随即在 `windows-latest` 上自动完成：

1. `cargo test --locked`；
2. 校验 **tag 与 `Cargo.toml` 版本一致**（不一致立即失败，不浪费一次构建）；
3. `cargo build --locked --release`，并校验 exe 的 `FileVersion` 与 tag 一致（防"没嵌上版本资源"）；
4. 编译安装包，生成 `SHA256SUMS.txt`；
5. 把免安装 exe 复制成 `dsh-manager-v<版本>-windows-x64.exe`，连同安装包与 `SHA256SUMS.txt` 一起附到 `gh release create` 建出的 Release 上（正文取注解 tag 的说明；含 `-` 的 tag 自动标为 prerelease）。

日常 CI（[`ci.yml`](.github/workflows/ci.yml)）在 push 到 `main` 与 PR 上跑 `cargo test --locked`。

## 文档

| 文件                                         | 内容                                                       |
| -------------------------------------------- | ---------------------------------------------------------- |
| [docs/SRS.md](docs/SRS.md)                   | 软件需求规范 —— 实现、验收与变更的唯一依据                 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构设计与关键模块详细设计（并发模型、事务引擎、IPC 契约） |
| [docs/VERIFICATION.md](docs/VERIFICATION.md) | 验证记录（含被实测证伪的设计前提）                         |
| [docs/RULINGS.md](docs/RULINGS.md)           | 实现期间的裁决记录                                         |

## 许可

本仓库尚未声明开源许可证。第三方署名义务已履行：Slint 的 `AboutSlint` 组件显示在"关于 DSH Manager"对话框中（Slint 免版税许可 v2.0）。
