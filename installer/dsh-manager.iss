; ═══════════════════════════════════════════════════════════════════════════
;  DSH Manager 安装脚本（Inno Setup 6 / 7 —— 同一份脚本两边都编过）
;
;  编译：  ISCC.exe installer\dsh-manager.iss
;          （开发机上是 Inno Setup 7：C:\Software\Inno Setup 7\ISCC.exe；CI 用 choco 装的 6）
;  前置：  cargo build --release
;
;  ⚠ 版本号**不在这里手写**。它取自 target\release\dsh-manager.exe 的
;  FILE_VERSION 字符串资源，而那份资源由 build.rs 从 CARGO_PKG_VERSION 注入。
;  于是 Cargo.toml → exe → 安装包 三者**不可能漂移**，没有需要手工同步的地方；
;  代价是必须先构建、后编译安装包（下面用 #error 把这条前置条件说清楚）。
;
;  ⚠ 安装包**不碰** state.json 与 notes-cache.json —— 那两个在 %APPDATA% 下，
;  不在安装目录里。所以升级/卸载都不会毁掉用户的端口、主题、代理与说明缓存。
; ═══════════════════════════════════════════════════════════════════════════

#define AppName     "DSH Manager"
#define AppExeName  "dsh-manager.exe"
#define RepoRoot    AddBackslash(SourcePath) + "..\"
#define ReleaseExe  RepoRoot + "target\release\" + AppExeName
#define IconFile    RepoRoot + "assets\logo.ico"

#if !FileExists(ReleaseExe)
  #error 找不到 target\release\dsh-manager.exe —— 请先执行 cargo build --release 再编译本安装包
#endif

; 取字符串资源（"0.1.0"），不是四段数字版本 —— 后者是 "0.1.0.0"，与 tag 对不上
#define AppVersion      GetFileVersionString(ReleaseExe)
#define AppVersionQuad  GetVersionNumbersString(ReleaseExe)

[Setup]
; ⚠ AppId 是**升级与卸载的识别键**，一旦发布就不得再改 —— 改了会被当成另一个程序，
; 旧版本不会被覆盖，而是并存出两个卸载项。
AppId={{8F3C1A64-9B2E-4E7A-9C4D-1F5A6B7C8D9E}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
VersionInfoVersion={#AppVersionQuad}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
; 安装目录**让用户可改**（按需求）。缺省落在用户目录下，所以默认全程无 UAC。
DisableDirPage=no
DisableProgramGroupPage=yes
; 缺省"仅为当前用户"安装：本程序是单用户本机工具（SRS §2.3），不该为一个
; 免安装式的小工具弹 UAC。但**允许**在向导里改成"为所有用户安装" —— 选了才提权，
; 这是把安装目录改到 Program Files / C:\Software 这类受保护路径的唯一前提。
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline dialog
; 本程序是 64 位 msvc 目标（rustc host = x86_64-pc-windows-msvc）
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#RepoRoot}dist
; ⚠ 发布物名字是**英文 + 说明平台**：本安装包只装 x64（见上面 ArchitecturesAllowed），
; 而 `v{#AppVersion}` 里的 `v` 与 git tag 逐字一致 —— 更新器（FR-38/FR-39）就是按
; 这个名字去 Release 里找资产的，改名等于让老版本的自动更新失效。
OutputBaseFilename=dsh-manager-v{#AppVersion}-windows-x64-setup
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\{#AppExeName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; 覆盖正在运行的 exe 时，让 Inno 自己去处理"文件被占用"（本程序是托盘常驻，
; 很可能正开着）。它会把占用者列出来并请用户关闭，而不是甩一个写失败。
CloseApplications=yes

[Languages]
; ⚠ 只有英文 —— Inno 6 自带语言里**没有简体中文**（Languages\ 下 30 个文件，
; 无 ChineseSimplified.isl）。要中文向导需另取一份非官方翻译 .isl 放进来，
; 那是一份第三方文件，故未擅自加入。
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#ReleaseExe}"; DestDir: "{app}"; Flags: ignoreversion
; 图标已由 build.rs 嵌进 exe 本体，因此不必再单独安装 logo.ico ——
; 快捷方式直接用 {app}\dsh-manager.exe 取图标。

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
; ⚠ 两条**互斥**的条目，合起来才是"人工安装照旧、自动更新全程不用点"：
;   · 第 1 条（postinstall skipifsilent）—— 向导完成页上那个"完成后启动"勾选框，
;     人工安装走这条；
;   · 第 2 条（skipifnotsilent）—— 只在**静默**安装时跑，也就是 FR-39 的自动更新：
;     用户在更新框里已经点过〔下载并安装〕，不该再被要求点一次"完成"才能看到
;     程序回来（本次修订要消灭的正是这一下）。
;
; ⚠ 第 2 条**不能**靠"把第 1 条的 skipifsilent 去掉"来实现：postinstall 的语义是
;   "在完成页上放一个勾选框"，而静默模式根本没有完成页 —— 它到底跑不跑，Inno 的
;   文档没有写死。`skipifnotsilent` 是文档明写的判据（帮助原文："Instructs Setup to
;   skip this entry if Setup is not running (very) silent"），不依赖未言明的行为。
;
; ⚠ 重启**只能**由这里做：本程序在启动安装程序后必须退出（要替换的正是它自己的
;   exe），所以"装完再把程序拉起来"这件事只有还活着的安装程序能完成。
;   `runasoriginaluser`：即便这次安装走 UAC 提了权，拉起来的也是**非提权**的原用户
;   进程 —— 否则托盘、`%APPDATA%` 下的 state.json 全会变成管理员所有。
; ⚠ 不会因此拉起两个进程：`RestartApplications` 缺省 yes 只重启"被 Setup 关掉的"
;   程序，而本程序是自己退出的，且没调 `RegisterApplicationRestart`（帮助原文：
;   "the application needs to be using the Windows RegisterApplicationRestart API"）。
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\{#AppExeName}"; Flags: nowait skipifnotsilent runasoriginaluser
