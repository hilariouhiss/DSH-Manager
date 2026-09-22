//! 第三方插件管理：profile 盘点 + registry 查最新版 + `dsh plugin` 命令转发。
//!
//! **本项目不写 profile 的任何文件**（spec §1）：只读 `<profile>/package.json` 与
//! `<profile>/node_modules/<包>/package.json`；一切变更经
//! `dsh plugin --profile <name> …` 转发 —— 由 dsh 自己持 profile 写锁，并在退出码 0
//! 后 reconcile `dsh.profile.bundles`（spec F2/F3）。绕过它直接调 `pnpm.cmd` 会让
//! 卸载后的 bundle 列表留下悬挂项。
//!
//! 与 `config.rs` 同款边界：**不含 UI 逻辑，也不含"何时该拉取"的决策** ——
//! 那是 `main.rs` 的 worker 的事。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use semver::Version;

use crate::model::{PluginOp, PluginRow, SHIM_NAMES};
use crate::pm::{self, CmdOut};

/// 本程序管的 profile。`dsh web` 等价于 `dsh --profile web`，本程序也只管它。
pub const PROFILE: &str = "web";

/// `%DSH_HOME%\profiles\web`；`DSH_HOME` 缺失时回落 `%USERPROFILE%\.dsh\profiles\web`。
///
/// 这两个变量覆盖了 dsh 自己的解析结果：Node 的 `os.homedir()` 在 Windows 上就是
/// `USERPROFILE`，而 `DSH_HOME` 优先于它。两者都拿不到 → `None`
/// （界面显示"未找到 profile"并禁用全部插件控件）。
///
/// 拆成"接收两个 Option"的纯函数是为了可单测 —— 与 `config::load_from` 同款做法。
pub fn profile_dir_from(dsh_home: Option<OsString>, userprofile: Option<OsString>) -> Option<PathBuf> {
    let base = match dsh_home {
        Some(h) if !h.is_empty() => PathBuf::from(h),
        _ => PathBuf::from(userprofile?).join(".dsh"),
    };
    Some(base.join("profiles").join(PROFILE))
}

/// 从真实环境变量解析 profile 目录。
pub fn profile_dir() -> Option<PathBuf> {
    profile_dir_from(std::env::var_os("DSH_HOME"), std::env::var_os("USERPROFILE"))
}

/// 规格白名单（**信任边界**，spec §7）。允许 `名称` 或 `名称@版本`：
///
/// - 名称 = `pkg` 或 `@scope/pkg`，字符集 `[A-Za-z0-9._~-]`
/// - 版本段 = 空或 `[A-Za-z0-9.+-]`（`^1.0.0` / `~1.0.0` 这类范围**刻意不允许** ——
///   界面上显示的"最新版"是具体版本，装一个范围会让显示与实际情况分叉）
///
/// 这一步挡住的是 `file:` / `link:` / `git+` / `github:` / 绝对与相对路径 / 空白 / 引号。
/// 两条理由都不是洁癖：① 它们会让 pnpm 去**本机路径或远端 git** 取代码并执行其构建脚本；
/// ② SRS CON-4 只允许 registry.npmjs.org 与 api.github.com 两个**主机**。
///
/// ⚠ 切分必须用 `rfind('@')` **且下标 > 0**：scoped 包名以 `@` 开头，
/// `rsplit_once('@')` 会把 `@scope/name` 切成 `("", "scope/name")` 从而误判为"没有名称"。
pub fn valid_spec(spec: &str) -> bool {
    let (name, version) = match spec.rfind('@') {
        Some(i) if i > 0 => (&spec[..i], &spec[i + 1..]),
        _ => (spec, ""),
    };
    let name_ok = match name.strip_prefix('@') {
        // scoped：必须恰好一段 `/`，且两段都是合法 ident
        Some(rest) => match rest.split_once('/') {
            Some((scope, pkg)) => is_ident(scope) && is_ident(pkg),
            None => false,
        },
        None => is_ident(name),
    };
    name_ok
        && (version.is_empty()
            || version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-')))
}

/// npm 名称的字符集。`/` 不在其中 —— 它能且仅能在 scope 与包名之间那一个位置出现。
fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '~' | '-'))
}

/// 从 packument 里取 `dist-tags.latest`。
///
/// ⚠ 这里用 `latest` tag **是对的，不要照抄 FR-8 的禁令**：FR-8 禁止把 `latest` 当"最新"
/// 是针对 **dsh 自身** —— 它有 alpha / rc / stable 三条通道，且 `latest` 实测比已装的
/// alpha 版更旧（SRS §2.2.5）。**第三方插件没有通道概念**，`dist-tags.latest` 就是它
/// 语义正确的最新版。
///
/// 三种失败（没有该字段 / JSON 非法 / 版本号非法）一律 `None` 而**不是 Err**：
/// 单个包查不到只该让那一行显示"—"，不该牵连其他行（spec §4.3 的失败路径）。
pub fn parse_latest(body: &str) -> Option<Version> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.get("dist-tags")?.get("latest")?.as_str()?.parse().ok()
}

/// 查一个包的 registry 最新版。任何失败 → `None`（调用方渲染成"—"）。
///
/// scoped 包的 `/` 转义成 `%2F`：两种写法 registry 都接受，转义是为了不让任何中间层
/// 对包名里的斜杠做路径处理。走 `dsh::agent()`，因此**自动带上系统代理**（FR-33）。
pub fn fetch_latest(agent: &ureq::Agent, name: &str) -> Option<Version> {
    let url = format!("https://registry.npmjs.org/{}", name.replace('/', "%2F"));
    let body = agent
        .get(&url)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .header("User-Agent", crate::dsh::USER_AGENT)
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    parse_latest(&body)
}

/// 并行补齐每行的 `latest`：每包一个线程（`std::thread::scope`），共用一个 Agent clone。
///
/// 依据：本机单次 registry 请求实测 4.1 s，6 个包串行 ≈ 25 s（spec §4.3 / NFR-12）。
/// 线程 panic 只让那一行变 `None`（`join().unwrap_or(None)`），不影响其他行。
pub fn fill_latest(rows: &mut [PluginRow]) {
    if rows.is_empty() {
        return;
    }
    let agent = crate::dsh::agent();
    let latest: Vec<Option<Version>> = std::thread::scope(|scope| {
        let handles: Vec<_> = rows
            .iter()
            .map(|row| {
                let agent = agent.clone();
                let name = row.name.clone();
                scope.spawn(move || fetch_latest(&agent, &name))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
    });
    for (row, l) in rows.iter_mut().zip(latest) {
        row.latest = l;
    }
}

/// 盘点 profile 里已装的第三方插件。
///
/// **列表来源是 `dependencies`，不是 `node_modules`**（spec F6）：实测 `node_modules`
/// 里有不在 `dependencies` 的传递依赖（`@hilariouhiss/dsh-skill-kit`），列进来就与用户
/// 在 `dsh plugin --profile web list` 里看到的对不上。
///
/// **不做任何包名过滤** —— profile 的 `dependencies` 本来就只有第三方插件；
/// `@deepseek-ai/*` 那三个 bundle 只在 `dsh.profile.bundles` 里，不在依赖里。
/// 顺手加一条 `@deepseek-ai` 过滤，等于把一条实测事实换成一条猜测的启发式。
///
/// 顺序由 `serde_json::Map` 决定（未启用 `preserve_order` 时是 BTreeMap = 字典序），
/// 与 pnpm 写回 package.json 的顺序一致。
pub fn read_installed(dir: &Path) -> Result<Vec<PluginRow>, String> {
    let manifest = dir.join("package.json");
    let text = std::fs::read_to_string(&manifest)
        .map_err(|e| format!("读取 {} 失败：{e}", manifest.display()))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("解析 {} 失败：{e}", manifest.display()))?;

    let mut rows = Vec::new();
    let deps = json.get("dependencies").and_then(|d| d.as_object());
    for (name, spec) in deps.into_iter().flatten() {
        rows.push(PluginRow {
            name: name.clone(),
            spec: spec.as_str().unwrap_or_default().to_string(),
            installed: installed_version(dir, name),
            latest: None, // 由 fill_latest 补齐
        });
    }
    Ok(rows)
}

/// 已装版本：`<profile>/node_modules/<包>/package.json` 的 `version`。
/// 读不到（包没装上 / 文件损坏 / 版本号非法）→ `None`，界面显示"未安装"。
fn installed_version(dir: &Path, name: &str) -> Option<Version> {
    let path = dir.join("node_modules").join(name).join("package.json");
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("version")?.as_str()?.parse().ok()
}

/// 汇总成"一次 add 多规格"的更新操作；没有可更新项 → `None`（界面上〔全部更新〕禁用）。
pub fn update_all_op(rows: &[PluginRow]) -> Option<PluginOp> {
    let items: Vec<(String, Version)> = rows
        .iter()
        .filter(|r| r.updatable())
        .filter_map(|r| r.latest.clone().map(|v| (r.name.clone(), v)))
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(PluginOp::UpdateAll(items))
    }
}

/// 执行一次 `dsh plugin …`。
///
/// 按 `SHIM_NAMES` 顺序尝试（GC-7）：npm / pnpm / yarn 生成 `dsh.cmd`，
/// 而 **bun 生成 `dsh.exe`** —— 写死 `.cmd` 会让 bun 用户的操作直接失败。
///
/// ⚠ 必须走 `pm::run_cmd`：它带 `CREATE_NO_WINDOW`（GC-8），自己 `Command::new`
/// 会在每次操作时闪一个黑窗口。退出码非零**不是** `Err`（与 `pm::run_cmd` 同语义）——
/// 由调用方判定成败，这样 pnpm 的原文才能进日志。
pub fn run_plugin(args: &[String]) -> Result<CmdOut, String> {
    run_plugin_with(&SHIM_NAMES, args)
}

/// `run_plugin` 的可测内核：按给定 shim 名单顺序尝试，全失败才 `Err`。
///
/// **为什么要有这道缝**：真实的 `dsh plugin` 依赖 pnpm，而本项目的沙箱里 pnpm
/// 跑不起来（`pnpm --version` 直接报 untrusted mount point，见 VERIFICATION 的环境限制）。
/// 但"参数是否**逐字**送到进程"与"退出码非零不被当成执行失败"这两条可以用临时目录里的
/// 假 shim 验 —— 与 `pm::read_dsh_version_at` 存在的理由同款（TR-4 的判别性测试靠它）。
pub fn run_plugin_with(shims: &[&str], args: &[String]) -> Result<CmdOut, String> {
    let mut last = String::new();
    for shim in shims {
        match pm::run_cmd(shim, args) {
            Ok(out) => return Ok(out),
            Err(e) => last = format!("{shim}: {e}"),
        }
    }
    Err(format!("无法执行 dsh —— 已尝试 {}：{last}", shims.join(" / ")))
}

/// worker 的唯一入口：定位 profile → 盘点 → 并行补最新版。
pub fn fetch_all() -> Result<Vec<PluginRow>, String> {
    let dir = profile_dir().ok_or_else(|| {
        "无法定位 profile：DSH_HOME 与 USERPROFILE 都不可用".to_string()
    })?;
    let mut rows = read_installed(&dir)?;
    fill_latest(&mut rows);
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    /// 造一个假 profile：写 package.json，按需写 node_modules 下各包的 package.json。
    /// 返回目录；调用方负责删。
    fn fake_profile(tag: &str, manifest: &str, installed: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dsh-mgr-plugin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("package.json"), manifest).unwrap();
        for (name, version) in installed {
            let d = dir.join("node_modules").join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("package.json"), format!(r#"{{"version":"{version}"}}"#)).unwrap();
        }
        dir
    }

    #[test]
    fn valid_spec_accepts_registry_specs() {
        for s in [
            "@hilariouhiss/dsh-gitbash",
            "@hilariouhiss/dsh-gitbash@1.0.1",
            "left-pad",
            "left-pad@1.2.3-rc.1",
            "@scope/pkg@latest",
            "a@1.0.0+build.7",
        ] {
            assert!(valid_spec(s), "{s} 应通过");
        }
    }

    /// ★ 信任边界（spec §7）：非 registry 来源一律拒绝。
    #[test]
    fn valid_spec_rejects_non_registry_sources() {
        for s in [
            "file:../x",
            "link:./x",
            "git+https://h/r.git",
            "github:u/r",
            "https://registry.npmjs.org/x",
            "../x",
            r"C:\x",
            "a/b",
            "@scope/",
            "@scope",
            "",
            " ",
            "a b",
            "a\"b",
            "@scope/p@1@2",  // 第二个 @ 落进名称里 → 名称含 @ → 非法
            "@scope/p@^1.0.0",
        ] {
            assert!(!valid_spec(s), "{s} 应被拒绝");
        }
    }

    #[test]
    fn parse_latest_reads_dist_tag_latest() {
        let body = r#"{"dist-tags":{"latest":"1.3.0","beta":"2.0.0-beta.1"},"versions":{}}"#;
        assert_eq!(parse_latest(body), Some(v("1.3.0")));
        assert_eq!(parse_latest(r#"{"versions":{}}"#), None, "无 dist-tags 应 None 而非 Err");
        assert_eq!(parse_latest("not json"), None);
        assert_eq!(parse_latest(r#"{"dist-tags":{"latest":"nope"}}"#), None, "非法版本号");
    }

    #[test]
    fn profile_dir_prefers_dsh_home_then_userprofile() {
        let d = |s: &str| Some(OsString::from(s));
        assert_eq!(
            profile_dir_from(d(r"C:\h"), d(r"C:\u")),
            Some(PathBuf::from(r"C:\h").join("profiles").join("web"))
        );
        assert_eq!(
            profile_dir_from(None, d(r"C:\u")),
            Some(PathBuf::from(r"C:\u").join(".dsh").join("profiles").join("web"))
        );
        assert_eq!(profile_dir_from(None, None), None);
        // 空串按"没设"处理（变量存在但为空是 Windows 上常见形状）
        assert_eq!(profile_dir_from(d(""), None), None);
    }

    /// ★ spec F6 的回归钉子：`node_modules` 里**不在 `dependencies`** 的包不得出现在结果里；
    /// 已装版本读不到时是 `None` 而不是某个默认值。
    #[test]
    fn read_installed_lists_only_dependencies_and_tolerates_missing_version() {
        let dir = fake_profile(
            "read",
            r#"{"dependencies":{"@s/b":"^1.0.0","@s/a":"1.2.1"}}"#,
            &[("@s/a", "1.2.1"), ("@s/transitive", "9.9.9")],
        );
        let rows = read_installed(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            vec!["@s/a", "@s/b"],
            "只列 dependencies（字典序）；@s/transitive 只在 node_modules 里，不得出现"
        );
        assert_eq!(rows[0].spec, "1.2.1", "规格原样保留");
        assert_eq!(rows[0].installed, Some(v("1.2.1")));
        assert_eq!(rows[1].installed, None, "node_modules 里没有 → None");
        assert!(rows.iter().all(|r| r.latest.is_none()), "latest 由 fill_latest 补，这里必须是 None");
    }

    #[test]
    fn read_installed_reports_unreadable_profile() {
        assert!(read_installed(Path::new(r"C:\definitely\not\here")).is_err());
    }

    /// ★ 假 shim 冒烟：参数必须**逐字**送到进程，且退出码非零是**结果**不是 `Err`。
    ///
    /// 这两条是"命令表写对了"之外唯一还能自动化的部分 —— 真实 `dsh plugin` 要 pnpm，
    /// 而本项目沙箱里 pnpm 起不来（见 VERIFICATION 的环境限制，端到端由用户执行）。
    #[test]
    fn run_plugin_forwards_args_verbatim_and_reports_exit_code() {
        let dir = std::env::temp_dir().join(format!("dsh-mgr-plugin-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("dsh-probe.cmd");
        // 回显收到的参数，然后以 3 退出（pnpm 失败时就是这个形状）
        std::fs::write(&shim, "@echo off\r\necho ARGS:%*\r\nexit /b 3\r\n").unwrap();

        let args = PluginOp::Remove("@s/p".into()).args("web");
        let got = run_plugin_with(&[shim.to_str().unwrap()], &args);
        let _ = std::fs::remove_dir_all(&dir);

        let out = got.expect("shim 存在 → 必须 Ok（非零退出码不是 Err）");
        assert_eq!(out.code, 3, "退出码必须原样带回来");
        assert!(
            out.stdout.contains("ARGS:plugin --profile web remove @s/p"),
            "参数必须逐字到达进程（顺序与拼写都算），实际输出：{}",
            out.stdout
        );
    }

    #[test]
    fn run_plugin_errors_only_when_no_shim_starts() {
        let args = PluginOp::Install("@s/p".into()).args("web");
        let err = run_plugin_with(&["definitely-not-a-real-shim.exe"], &args)
            .expect_err("一个 shim 都起不来才该 Err");
        assert!(err.contains("definitely-not-a-real-shim.exe"), "错误里要点明试过谁：{err}");
    }

    #[test]
    fn update_all_op_collects_only_updatable_rows() {
        let row = |name: &str, i: Option<&str>, l: Option<&str>| PluginRow {
            name: name.into(),
            spec: "1.0.0".into(),
            installed: i.map(v),
            latest: l.map(v),
        };
        let rows = vec![
            row("@s/old", Some("1.0.0"), Some("1.1.0")),
            row("@s/new", Some("1.1.0"), Some("1.1.0")),
            row("@s/unknown", Some("1.0.0"), None),
            row("@s/missing", None, Some("2.0.0")),
        ];
        assert_eq!(
            update_all_op(&rows),
            Some(PluginOp::UpdateAll(vec![("@s/old".into(), v("1.1.0"))])),
            "只收严格可更新的那一行"
        );
        assert_eq!(update_all_op(&rows[1..]), None, "没有可更新项 → None（按钮禁用）");
        assert_eq!(update_all_op(&[]), None);
    }
}
