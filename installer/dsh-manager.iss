; ═══════════════════════════════════════════════════════════════════════════
;  DSH Manager 安装脚本（Inno Setup 6）
;
;  编译：  "C:\Software\Inno Setup 6\ISCC.exe" installer\dsh-manager.iss
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
OutputBaseFilename=dsh-manager-{#AppVersion}-setup
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
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
