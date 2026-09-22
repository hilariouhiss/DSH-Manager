//! 构建脚本：编译 `.slint`，并给 Windows exe 嵌上图标与版本信息。

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    slint_build::compile("ui/app.slint").expect("Slint 编译失败");
    embed_icon_and_version();
}

/// 把 `RT_GROUP_ICON` 与 `VS_VERSION_INFO` 嵌进 PE。
///
/// **为什么需要这一步**：Slint 只是在**运行期**把 `assets/logo.png` 设成窗口/托盘
/// 图标 —— 那是个 HICON，不是 PE 资源。少了这一段，资源管理器与任务管理器里的
/// `dsh-manager.exe` 是空白默认图标，快捷方式也没有图标可用（安装包同样拿不到）。
///
/// ⚠ **不新增依赖、不破 GC-2**：`winresource` / `embed-resource` 这类 crate 本身也只是
/// 去调 `rc.exe` / `windres`，并不能凭空变出一个资源编译器来。这里自己找、自己调，
/// 反而更短，且少一个编译单元。
///
/// ⚠ **找不到资源编译器时只告警、不失败**：本机压根没装 Windows SDK（没有 `rc.exe`），
/// 靠 PATH 上的 `llvm-rc`（LLVM 的 MSVC 兼容实现；已实测 MSVC 的 `link.exe` 能直接吃它
/// 产出的 `.res`，`VS_VERSION_INFO` 确实进了 PE）。换一台机器可能是别的组合，
/// 而图标是**装饰**、不是构建的前提 —— 不该因为缺个装饰就让整个项目构建不了。
fn embed_icon_and_version() {
    // 交叉编译到非 Windows 时不做这件事
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = Path::new("assets/logo.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", icon.display());
    if !icon.is_file() {
        println!("cargo:warning=assets/logo.ico 不存在，跳过图标与版本信息嵌入");
        return;
    }

    let Some(rc) = find_resource_compiler() else {
        println!(
            "cargo:warning=找不到资源编译器（PATH 上既无 rc.exe 也无 llvm-rc）——\
             跳过图标与版本信息嵌入，exe 在资源管理器里会是默认图标"
        );
        return;
    };

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 必须存在"));
    let rc_file = out_dir.join("dsh-manager.rc");
    let res_file = out_dir.join("dsh-manager.res");

    // ⚠ 用 CARGO_MANIFEST_DIR 拼**绝对路径**，且把反斜杠换成正斜杠：
    // 资源编译器的工作目录不是 crate 根，相对路径找不到文件；而 .rc 里的
    // 反斜杠是转义字符，直接写 `C:\Mine\...` 会被当成 `\M` 之类的转义序列。
    let icon_abs = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(icon)
        .to_string_lossy()
        .replace('\\', "/");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION 必须存在");

    if let Err(e) = std::fs::write(&rc_file, resource_script(&icon_abs, &version)) {
        println!("cargo:warning=写 .rc 失败，跳过图标与版本信息嵌入：{e}");
        return;
    }

    // `/fo` 是 rc.exe 与 llvm-rc 共有的输出开关
    match Command::new(&rc).arg("/fo").arg(&res_file).arg(&rc_file).output() {
        Ok(o) if o.status.success() && res_file.is_file() => {
            // ⚠ 只给 bin 目标，不给 test —— 测试可执行文件不需要这份资源，
            // 少一次链接输入就少一点测试构建时间。
            println!("cargo:rustc-link-arg-bins={}", res_file.display());
        }
        Ok(o) => println!(
            "cargo:warning=资源编译失败（{}），跳过：{}{}",
            rc.display(),
            String::from_utf8_lossy(&o.stderr).trim(),
            String::from_utf8_lossy(&o.stdout).trim()
        ),
        Err(e) => println!("cargo:warning=无法执行 {}，跳过：{e}", rc.display()),
    }
}

/// 在 PATH 上找资源编译器。`RC` 环境变量优先（显式指定胜过猜测）。
///
/// 先整条 PATH 找 `rc.exe`（MSVC 的规范实现），再回头找 `llvm-rc.exe` ——
/// 而不是"逐个目录先 rc 后 llvm-rc"，因为后者可能在一个不相关的目录里
/// 先撞上 llvm-rc，从而错过后面某个目录里更合适的 rc.exe。
fn find_resource_compiler() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("RC") {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for name in ["rc.exe", "llvm-rc.exe"] {
        for dir in &dirs {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// 生成 `.rc` 内容。
///
/// 版本号由 `CARGO_PKG_VERSION` 注入 —— 于是它**不可能**与 `Cargo.toml` 漂移，
/// 不用手工同步（这也是安装包那边 `GetFileVersion` 能反过来读到正确版本的前提）。
fn resource_script(icon_abs: &str, version: &str) -> String {
    // FILEVERSION 要 4 段数字，而 CARGO_PKG_VERSION 可能是 `0.1.0-alpha.2`。
    // 取数字前缀，缺的段补 0。
    let base = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts = base.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let quad = format!(
        "{},{},{},{}",
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0)
    );

    // 1 ICON：`1` 是规范做法（资源管理器取第一个图标组）
    format!(
        r#"1 ICON "{icon_abs}"

1 VERSIONINFO
FILEVERSION {quad}
PRODUCTVERSION {quad}
FILEOS 0x40004L
FILETYPE 0x1L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "FileDescription", "DSH Manager\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "InternalName", "dsh-manager\0"
      VALUE "OriginalFilename", "dsh-manager.exe\0"
      VALUE "ProductName", "DSH Manager\0"
      VALUE "ProductVersion", "{version}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}
