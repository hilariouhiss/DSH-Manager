# DSH 管理器

Windows 桌面端（Tauri 2 + 原生 TypeScript）管理全局安装的 `@deepseek-ai/dsh`：

- 查看已安装版本、npm 通道标签与全部历史版本，安装任意版本（升级 / 回滚 / 切通道）
- 启动、停止、重启 `dsh web`（端口可配，可选自动打开浏览器）
- 识别并停止非本应用启动的 DSH 实例（停止前核验进程命令行，避免误杀）
- 日志面板查看 DSH 运行输出与 npm 安装输出

## 开发

```bash
pnpm install
pnpm tauri dev
```

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
