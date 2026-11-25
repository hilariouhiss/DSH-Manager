# DSH 管理器

Windows 桌面端（Tauri 2 + 原生 TypeScript）管理全局安装的 `@deepseek-ai/dsh`：

- 查看已安装版本、npm 通道标签与全部历史版本，安装任意版本（升级 / 回滚 / 切通道）
- 启动、停止、重启 `dsh web`（端口可配，可选自动打开浏览器）
- 识别并停止非本应用启动的 DSH 实例（停止前核验进程命令行，避免误杀）
- 日志面板查看 DSH 运行输出与 npm 安装输出
- 关窗与托盘：默认关窗即弹确认，退出时先停止本应用启动的 DSH；可在设置里改为「关窗后保持后台运行」，托盘提供显示主界面 / 启动 / 停止 / 退出
- 设置：开机自启（写入登录项并带 `--hidden`，登录后静默进托盘）
- 单实例：已在运行时再次双击只唤起已有窗口，不会出现第二个托盘图标

## 开发

```bash
pnpm install
pnpm tauri dev
```

> 换图标后要 `pnpm tauri icon src-tauri/app-icon.svg` 重新生成，再 `touch src-tauri/build.rs` 才重新嵌入 exe（cargo 不会因为 png/ico 变动就重跑 build script）。

## 打包

```bash
pnpm tauri build --no-bundle   # 只要 exe：src-tauri/target/release/dsh-manager.exe
pnpm tauri build               # 另出 NSIS/MSI 安装包
```

## 说明

- 只管理 `dsh web`；`desktop` profile 由 Electron 客户端独占，不在范围内。
- 安装新版本前必须先停止 DSH（Windows 下运行中的文件会被锁定），界面会提示并代劳。
- 日志位置：`%LOCALAPPDATA%\com.hiss.dsh-manager\logs\web.log`、`npm.log`。
- 环境要求：Node.js 24.22.3+（24 线）或 26.13.1+，以及已 `npm i -g @deepseek-ai/dsh`。
