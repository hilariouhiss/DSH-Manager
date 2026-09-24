//! 包管理器探测（FR-1 ~ FR-5）、通道判定（FR-7 / FR-8）与命令执行 helper。

use std::path::{Path, PathBuf};

use crate::model::*;

/// FR-7：由 semver 的 pre 字段推导发布通道。
pub fn channel_of(v: &Version) -> Channel {
    if v.pre.is_empty() {
        return Channel::Stable;
    }
    let s = v.pre.as_str();
    if s.starts_with("alpha") {
        Channel::Alpha
    } else if s.starts_with("rc") {
        Channel::Rc
    } else {
        Channel::Other
    }
}

/// FR-8 v1.8 修订 / GC-14：在**勾选的通道**里求最新。
///
/// ⚠ 绝不使用 npm 的 `latest` tag —— 它可能比已安装版本更旧（本机实测：
/// `latest` = `0.1.5-rc.2`，而当时已装的 `0.1.6-alpha.2` 比它新）。"防降级"由调用方
/// 那条 `已装 >= 最新` 承担：本函数只回答"在勾选范围内，目录里最大的是谁"。
///
/// ⚠ 比较必须是 **semver 比较**（`max()`），不是字符串、也不是发布顺序：
/// `0.1.10-rc.1 > 0.1.9-rc.1`，而字符串比较会给出反的答案。
pub fn newest_among<'a>(versions: &'a [Version], sel: &Channels) -> Option<&'a Version> {
    versions.iter().filter(|v| sel.accepts(channel_of(v))).max()
}

/// 版本列表降序（最新在前）。FR-10 要求。
pub fn sorted_desc(mut versions: Vec<Version>) -> Vec<Version> {
    versions.sort_by(|a, b| b.cmp(a));
    versions
}

/// 在目录中查找 dsh 的可执行 shim。
pub fn shim_in(dir: &Path) -> Option<PathBuf> {
    SHIM_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|candidate| candidate.is_file())
}

/// 路径归一化比较：Windows 大小写不敏感，且需容忍尾部分隔符。
pub fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

/// FR-3 第 1~2 步：按 PATH 顺序找出第一个含 dsh shim 的完整路径。
///
/// **顺序即语义** —— 先命中的就是用户敲 `dsh` 时真正执行的那个。
/// 自己扫 PATH 而不用 `where.exe`：无外部进程依赖，且可用假 `exists`
/// 闭包做单元测试（SRS §8.4 要求）。
pub fn find_dsh_on_path(
    path_dirs: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    for dir in path_dirs {
        for name in SHIM_NAMES {
            let candidate = dir.join(name);
            if exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// FR-3 第 2 步：由 dsh shim 所在目录判断 owner PM。
pub fn owner_of(shim: &Path, bins: &[PmInfo]) -> Option<Pm> {
    let parent = shim.parent()?;
    bins.iter()
        .find(|info| same_dir(&info.bin_dir, parent))
        .map(|info| info.kind)
}

use std::process::{Command, Stdio};

/// CREATE_NO_WINDOW —— GC-8。缺了它每次调外部命令都会闪一个黑窗口。
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug)]
pub struct CmdOut {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 执行外部命令。**参数以数组传递，绝不拼接 shell 字符串**（GC-9）。
///
/// 返回 `Err` 只表示"命令没能跑起来"（可执行文件不存在、权限不足等）。
/// **退出码非零是 `Ok`** —— 这个区分是必需的：SRS TR-11 要求补偿流程
/// 能识别"对不存在的包执行卸载"这类非零退出，若把它当成执行失败，
/// 就会误判为补偿失败并错误报告 Degraded。
pub fn run_cmd(exe: &str, args: &[String]) -> Result<CmdOut, String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;

    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd
        .output()
        .map_err(|e| format!("无法执行 {exe}: {e}"))?;
    Ok(CmdOut {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// 解析 PATH 变量值，**丢弃空段**。纯函数，可单测。
///
/// ⚠ 为什么必须丢空段：`std::env::split_paths` 对 `PATH` 里的空条目
/// （`;;`、或以 `;` 开头/结尾）会产出一个空 `PathBuf`。于是
/// `空目录.join("dsh.cmd")` 得到**相对路径** `"dsh.cmd"` —— 它相对于当前
/// 工作目录解析，一旦 CWD 下恰好有同名文件就会被 `is_file()` 判为真，并因
/// `find_dsh_on_path` 的提前 return 而**遮蔽后面真实的 PATH 命中**。随后
/// `owner_of` 拿到 `parent() == Some("")` 返回 `None`，owner 判定直接失败。
///
/// 抽成独立函数是为了可测：`path_dirs()` 直接读环境变量，在并行测试里
/// 改 PATH 既不可靠也会干扰其他用例。见 `parse_path_var_drops_empty_segments`。
pub fn parse_path_var(v: &std::ffi::OsStr) -> Vec<PathBuf> {
    std::env::split_paths(v)
        .filter(|p| !p.as_os_str().is_empty())
        .collect()
}

/// 当前进程的 PATH 目录序列，保持顺序（顺序即语义，见 `find_dsh_on_path`）。
pub fn path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|v| parse_path_var(&v))
        .unwrap_or_default()
}

/// FR-1 + FR-2：探测单个 PM。未安装返回 None。
///
/// ⚠ `--version` 这一跑**不是为了版本号** —— `PmInfo` 里已经没有 `version` 字段了
/// （Task 20 删除：全仓零读取）。它在这里的作用是 TR-2 的可用性判据：
/// 命令跑不起来或退出码非零，即视为该 PM 不可用。
pub fn probe_pm(pm: Pm) -> Option<PmInfo> {
    let ver = run_cmd(pm.exe(), &["--version".to_string()]).ok()?;
    if ver.code != 0 {
        return None;
    }
    // ⚠ 必须先把 Vec<String> 绑到变量再取引用：直接写
    // `run_cmd(pm.exe(), &args.iter().map(..).collect())` 会因 collect() 的
    // 目标类型无法穿过引用推断而报 E0277（已实测 2 处）。
    let args: Vec<String> = pm.bin_dir_args().iter().map(|s| s.to_string()).collect();
    let dir = run_cmd(pm.exe(), &args).ok()?;
    if dir.code != 0 {
        return None;
    }
    Some(PmInfo { kind: pm, bin_dir: PathBuf::from(dir.stdout.trim()) })
}

/// FR-4 + TR-4：直接执行**指定目录下**的 dsh shim 读版本，**不经 PATH**。
///
/// TR-4 的强制要求：迁移验证绝不能用 PATH 解析。本机实测 PATH 中
/// pnpm\bin 排在 npm 之前，`pnpm → npm` 迁移时装完 npm 那份后
/// `where dsh` 仍指向 pnpm 的旧文件，验证会【假通过】。
pub fn read_dsh_version_at(dir: &Path) -> Option<Version> {
    version_from_shim(&shim_in(dir)?)
}

/// 执行指定 shim 读版本，**带退出码检查**。
///
/// ⚠ 抽成共享函数是为了**从结构上消除两个读取器的不对称**：
/// 早先 `read_dsh_version_at` 检查退出码而 `read_dsh_version_on_path` 不检查，
/// 于是一个"stdout 可解析但退出码非零"的 shim 会被 S4 接受、被 dir 读取器拒绝 ——
/// 两者对**同一次安装**给出相反结论，S4 假通过。
/// 只要守卫只写一份，这种不对称就不可能再出现。
///
/// 另一个好处：它是**可直接单测**的（传入任意 shim 路径），
/// 不需要改动进程的 PATH —— 那是并行测试里不可靠的做法。
pub fn version_from_shim(shim: &Path) -> Option<Version> {
    let out = run_cmd(&shim.to_string_lossy(), &["--version".to_string()]).ok()?;
    if out.code != 0 {
        return None;
    }
    out.stdout.trim().parse().ok()
}

/// 经 PATH 解析后读版本。**仅用于事务的 S4 最终验证**。
pub fn read_dsh_version_on_path() -> Option<(Pm, Version)> {
    let shim = find_dsh_on_path(&path_dirs(), &|p| p.is_file())?;
    let bins: Vec<PmInfo> = Pm::ALL.iter().filter_map(|pm| probe_pm(*pm)).collect();
    let owner = owner_of(&shim, &bins)?;
    // 与 read_dsh_version_at 共用同一个带退出码检查的实现
    Some((owner, version_from_shim(&shim)?))
}

/// FR-1 ~ FR-5：完整环境探测。
///
/// ⚠ 探测分两段，**不能合并**：正常路径（PATH 上有 dsh shim）与退化路径
/// （FR-3 第 3 步：问各 PM 的全局包列表）是互斥的 —— 只在 `find_dsh_on_path`
/// 一无所获时才查列表。理由是成本与语义：查一次列表要起一个 PM 子进程
/// （`npm ls -g --depth=0` 本机实测约 **1.5 秒** —— node 启动 + 解析全局树），
/// 而 PATH 命中时列表答案**无关紧要** ——
/// PATH 顺序才是"用户敲 dsh 时真正执行哪一个"的语义（见 `find_dsh_on_path`）。
pub fn probe_env() -> PmEnv {
    let available: Vec<PmInfo> = Pm::ALL.iter().filter_map(|pm| probe_pm(*pm)).collect();
    let dsh_path = find_dsh_on_path(&path_dirs(), &|p| p.is_file());
    let mut owner = dsh_path.as_deref().and_then(|s| owner_of(s, &available));
    let mut installed = read_dsh_version_on_path().map(|(_, v)| v);

    // FR-3 第 3 步：`dsh` 不在 PATH 上（无 shim）时，退化为依次查询各 PM 的
    // 全局包列表，**第一个**列出 `@deepseek-ai/dsh` 的即为 owner PM。
    //
    // ⚠ `dsh_path` 此时**保持 `None`** —— 它确实不在 PATH 上，不能因为"包列表里有"
    // 就伪造一个路径。代价是 TR-3 随后会拒绝事务（`BinNotOnPath`）—— 那是**正确的**：
    // 装完之后用户敲 `dsh` 仍然找不到它（SRS TR-3 存在的全部理由）。
    if dsh_path.is_none() {
        for info in &available {
            if let Some(v) = version_in_global_list(info.kind) {
                installed = Some(v);
                owner = Some(info.kind);
                break;
            }
        }
    }

    // FR-4 注：版本号在这里**不解析列表正文**——正常路径走 `dsh --version`
    // （`read_dsh_version_on_path` → `version_from_shim`）；只有退化路径
    // 才从列表里取（见上），因为此时 `dsh --version` 根本无从执行。
    //
    // ⚠ `available` **不进返回值**（FR-11 修订后它没有任何外部读者了，理由见
    // `model::PmEnv` 的注释）——它是本函数的局部变量，供上面的 owner 判定使用。
    PmEnv { owner, installed, dsh_path }
}

/// 包名。只在本模块的退化路径里用；`model::Pm::{install,uninstall}_args` 里
/// 的同一字面量由 `pm_command_table_is_exact` 钉住，两边不会各自漂移。
const PKG: &str = "@deepseek-ai/dsh";
/// 短名。少数 PM 的列表只印短名（不带 scope）。
const PKG_SHORT: &str = "dsh";

/// 这个空白分隔字段是否"点名"了 dsh 包？
///
/// ⚠ 必须是**整字段**匹配（`== PKG` / `== "dsh"` / 以 `PKG@`、`dsh@` 开头），
/// 不能用 `contains`：列表第一行是安装目录（`C:\Users\x\AppData\Roaming\npm`），
/// 用户名或路径里出现 `dsh` 三个字母是完全正常的，`contains` 会把路径当包名。
fn names_dsh(field: &str) -> bool {
    [PKG, PKG_SHORT]
        .iter()
        .any(|p| field == *p || field.strip_prefix(p).is_some_and(|r| r.starts_with('@')))
}

/// FR-3 第 3 步：从某个 PM 的**全局包列表输出**里取出 dsh 的版本。
///
/// **纯函数**，可用 fixture 单测（本机实测的四种输出形状见测试）。
///
/// 刻意**不为四个 PM 各写一个格式解析器**：四种输出的差异只有"包名与版本号怎么分隔"
/// 这一件事，其余（树状前缀 `+--` / `├──` / `└─`、`Legend:` / 路径行、`info` / `warning` 行）
/// 都是**独立的空白分隔字段**，按字段扫描时天然被跳过。于是只需要认两种形态：
///
/// - **同字段**：`@deepseek-ai/dsh@1.2.3`（npm / pnpm / bun / yarn 的树状行都长这样）
/// - **相邻字段**：`@deepseek-ai/dsh 1.2.3`（pnpm 的原生列表格式）
///
/// 版本号一律交给 `semver::Version::parse` 判定 —— 手写版本号规则是 AS-4 明确
/// 列为假设的东西，绝不能重写一遍。
///
/// ⚠ 形参 `pm` 是签名的一部分（与 `version_in_global_list` 对称），但**刻意不用**：
/// 一旦按 PM 分支，"四种格式"的知识就又回到了 per-PM 解析器里，而这里要的恰恰相反。
pub fn parse_global_list(_pm: Pm, output: &str) -> Option<Version> {
    for line in output.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        for (i, f) in fields.iter().enumerate() {
            // 形态 A：包名与版本在同一个字段
            if let Some((name, ver)) = f.rsplit_once('@') {
                if names_dsh(name) {
                    if let Ok(v) = ver.parse::<Version>() {
                        return Some(v);
                    }
                }
            }
            // 形态 B：包名与版本是两个相邻字段
            if names_dsh(f) {
                if let Some(v) = fields.get(i + 1).and_then(|t| t.parse::<Version>().ok()) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// FR-3 第 3 步：跑某个 PM 的全局包列表命令并解析。
///
/// `pm.exe()` 是 shim 全名（GC-7）、参数以数组传递（GC-9）、
/// `CREATE_NO_WINDOW` 在 `run_cmd` 内部（GC-8）—— 这里不需要重复任何一条。
pub fn version_in_global_list(pm: Pm) -> Option<Version> {
    // ⚠ 与 probe_pm / version_from_shim 相反：**不检查退出码**。
    // `npm ls -g --depth=0` 在有 peer 依赖告警时会以非零码退出，而 stdout 里的
    // 列表是完整的 —— 按退出码一刀切会让这个退化路径在最需要它的机器上永远为空。
    let args: Vec<String> = pm.list_args().iter().map(|s| s.to_string()).collect();
    let out = run_cmd(pm.exe(), &args).ok()?;
    parse_global_list(pm, &out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn channel_of_classifies_correctly() {
        assert_eq!(channel_of(&v("0.1.6-alpha.2")), Channel::Alpha);
        assert_eq!(channel_of(&v("0.1.5-rc.2")), Channel::Rc);
        assert_eq!(channel_of(&v("0.1.0")), Channel::Stable);
        assert_eq!(channel_of(&v("0.1.0-beta.1")), Channel::Other);
    }

    /// ★ v1.8 修的那个 bug 的回归测试：同一条线的下一个阶段必须算"更新"。
    ///
    /// 本机真实数据（2026-09-24 实测 registry）：已装 `0.1.7-alpha.2`，目录里还有
    /// `0.1.7-rc.1`（发布得更晚，semver 上 rc > alpha）。旧实现只在本通道里找最新
    /// ⇒ 得到 `0.1.7-alpha.2` ⇒ 界面说"已是最新"，而版本下拉里明明躺着 `0.1.7-rc.1`。
    /// 缺省（全勾）必须得到 `0.1.7-rc.1`。
    #[test]
    fn newest_among_all_channels_finds_the_next_stage() {
        let all = vec![v("0.1.7-alpha.1"), v("0.1.7-alpha.2"), v("0.1.7-rc.1")];
        assert_eq!(newest_among(&all, &Channels::ALL), Some(&v("0.1.7-rc.1")));

        // 只勾 alpha = v0.4.0 的旧行为，现在是**用户自己选的**：
        let only_alpha = Channels { alpha: true, rc: false, stable: false };
        assert_eq!(newest_among(&all, &only_alpha), Some(&v("0.1.7-alpha.2")));

        // 一个都不勾：勾选范围内没有候选（`Channel::Other` 除外，这里没有）
        let none = Channels { alpha: false, rc: false, stable: false };
        assert_eq!(newest_among(&all, &none), None);
    }

    /// ★ GC-14 的核心回归测试（判据从"按通道过滤"换成"在勾选范围内取最大"之后，
    /// 这一条必须仍然成立）。
    /// npm 的 latest tag 是 0.1.5-rc.2，比已装的 0.1.6-alpha.2 更旧 ——
    /// 求"最新"绝不能落在那个更旧的版本上。
    #[test]
    fn newest_among_never_downgrades() {
        let all = vec![v("0.1.5-rc.2"), v("0.1.6-alpha.1"), v("0.1.6-alpha.2")];
        // 全勾：最大的是 0.1.6-alpha.2（= 已装版本）⇒ 调用方那条 `>=` 判"已是最新"
        assert_eq!(newest_among(&all, &Channels::ALL), Some(&v("0.1.6-alpha.2")));
        assert_ne!(
            newest_among(&all, &Channels::ALL),
            Some(&v("0.1.5-rc.2")),
            "落在 latest tag 上就是把用户往下带"
        );
        // 只勾 rc：只有 0.1.5-rc.2 可选 —— 调用方必须靠 `>=` 把它判成"已是最新"，
        // 而不是"可更新到更旧的版本"
        let only_rc = Channels { alpha: false, rc: true, stable: false };
        assert_eq!(newest_among(&all, &only_rc), Some(&v("0.1.5-rc.2")));
    }

    #[test]
    fn newest_among_ordering_is_semver_not_string() {
        // 字符串序会把 "0.1.10-rc.1" 排在 "0.1.5-rc.2" 之前；semver 不会
        let all = vec![v("0.1.5-rc.2"), v("0.1.10-rc.1")];
        let only_rc = Channels { alpha: false, rc: true, stable: false };
        assert_eq!(newest_among(&all, &only_rc), Some(&v("0.1.10-rc.1")));
    }

    /// 预发布版排在正式版之前（同版本号），且**勾选范围之外的一律看不见**。
    #[test]
    fn prerelease_sorts_before_release_of_same_version() {
        // 0.1.6-alpha.2 < 0.1.6 —— 语义正确性
        let all = vec![v("0.1.6-alpha.2"), v("0.1.6")];
        let only_stable = Channels { alpha: false, rc: false, stable: true };
        let only_alpha = Channels { alpha: true, rc: false, stable: false };
        assert_eq!(newest_among(&all, &only_stable), Some(&v("0.1.6")));
        assert_eq!(newest_among(&all, &only_alpha), Some(&v("0.1.6-alpha.2")));
        // 全勾时两者都在范围内，正式版更大
        assert_eq!(newest_among(&all, &Channels::ALL), Some(&v("0.1.6")));
    }

    /// `Channel::Other`（beta 之类）永远参与 —— 见 `Channels::accepts` 的说明。
    #[test]
    fn newest_among_always_counts_other() {
        let all = vec![v("0.1.0-beta.1"), v("0.1.0-alpha.1")];
        let none = Channels { alpha: false, rc: false, stable: false };
        assert_eq!(newest_among(&all, &none), Some(&v("0.1.0-beta.1")));
    }

    #[test]
    fn sorted_desc_puts_newest_first() {
        let s = sorted_desc(vec![v("0.1.5-rc.2"), v("0.1.6-alpha.2"), v("0.1.0")]);
        assert_eq!(s[0], v("0.1.6-alpha.2"));
        assert_eq!(s[2], v("0.1.0"));
    }

    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    /// 造一个假文件系统：给定存在的路径集合。
    fn exists_fn(set: HashSet<PathBuf>) -> impl Fn(&Path) -> bool {
        move |p: &Path| set.contains(p)
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn find_dsh_takes_the_first_hit_in_path_order() {
        // ★ 关键语义：PATH 顺序决定谁生效。
        // 本机实测 PATH 中 pnpm\bin 排在 npm 之前，若两者都有 dsh，pnpm 的会赢。
        let dirs = vec![p("C:/first"), p("C:/second")];
        let set: HashSet<PathBuf> = [p("C:/first/dsh.cmd"), p("C:/second/dsh.cmd")]
            .into_iter()
            .collect();
        assert_eq!(
            find_dsh_on_path(&dirs, &exists_fn(set)),
            Some(p("C:/first/dsh.cmd"))
        );
    }

    #[test]
    fn find_dsh_recognizes_exe_shim_for_bun() {
        // SHIM_NAMES 必须同时含 .exe 与 .cmd：bun 在 Windows 生成 .exe shim
        let dirs = vec![p("C:/bun/bin")];
        let set: HashSet<PathBuf> = [p("C:/bun/bin/dsh.exe")].into_iter().collect();
        assert_eq!(
            find_dsh_on_path(&dirs, &exists_fn(set)),
            Some(p("C:/bun/bin/dsh.exe"))
        );
    }

    #[test]
    fn find_dsh_returns_none_when_absent() {
        let dirs = vec![p("C:/a"), p("C:/b")];
        assert_eq!(find_dsh_on_path(&dirs, &exists_fn(HashSet::new())), None);
    }

    #[test]
    fn owner_of_matches_by_parent_directory() {
        let bins = vec![
            PmInfo { kind: Pm::Npm, bin_dir: p("C:/Users/x/AppData/Roaming/npm") },
            PmInfo { kind: Pm::Pnpm, bin_dir: p("C:/Users/x/AppData/Local/pnpm/bin") },
        ];
        assert_eq!(
            owner_of(&p("C:/Users/x/AppData/Roaming/npm/dsh.cmd"), &bins),
            Some(Pm::Npm)
        );
        assert_eq!(
            owner_of(&p("C:/Users/x/AppData/Local/pnpm/bin/dsh.exe"), &bins),
            Some(Pm::Pnpm)
        );
    }

    #[test]
    fn owner_of_is_case_insensitive_on_windows() {
        // Windows 路径大小写不敏感，必须归一化比较
        let bins = vec![PmInfo {
            kind: Pm::Npm,
            bin_dir: p("C:/Users/X/AppData/Roaming/NPM"),
        }];
        assert_eq!(
            owner_of(&p("c:/users/x/appdata/roaming/npm/dsh.cmd"), &bins),
            Some(Pm::Npm)
        );
    }

    #[test]
    fn owner_of_tolerates_trailing_separator() {
        let bins = vec![PmInfo {
            kind: Pm::Npm,
            bin_dir: p("C:/npm/"),
        }];
        assert_eq!(owner_of(&p("C:/npm/dsh.cmd"), &bins), Some(Pm::Npm));
    }

    #[test]
    fn owner_of_returns_none_for_unknown_location() {
        let bins = vec![PmInfo { kind: Pm::Npm, bin_dir: p("C:/npm") }];
        assert_eq!(owner_of(&p("D:/elsewhere/dsh.cmd"), &bins), None);
    }

    #[test]
    fn path_dirs_is_nonempty_on_windows() {
        let dirs = path_dirs();
        assert!(!dirs.is_empty(), "PATH 不应为空");
        assert!(dirs.iter().all(|d| !d.as_os_str().is_empty()));
    }

    /// 空段必须被丢弃 —— 完整理由见 `parse_path_var` 的文档注释。
    #[test]
    fn parse_path_var_drops_empty_segments() {
        let v = std::ffi::OsString::from("C:/a;;C:/b;");
        assert_eq!(parse_path_var(&v), vec![p("C:/a"), p("C:/b")]);

        // 全空 → 全丢，不得留下一个空目录
        let only_seps = std::ffi::OsString::from(";;");
        assert!(parse_path_var(&only_seps).is_empty());

        // 单个正常项应原样保留（顺序也保留）
        let one = std::ffi::OsString::from("C:/only");
        assert_eq!(parse_path_var(&one), vec![p("C:/only")]);
    }

    #[test]
    fn run_cmd_reports_nonzero_exit_without_erroring() {
        // 退出码非零是【正常结果】而不是 Err —— 事务补偿依赖这个区分
        // （SRS TR-11：对不存在的包执行卸载会非零退出，不该当成执行失败）
        let out = run_cmd("cmd.exe", &["/c".into(), "exit 3".into()]);
        assert!(out.is_ok(), "非零退出不应返回 Err");
        assert_eq!(out.unwrap().code, 3);
    }

    #[test]
    fn run_cmd_errors_only_when_exe_missing() {
        let out = run_cmd("definitely-not-a-real-exe-xyz.exe", &[]);
        assert!(out.is_err(), "可执行文件不存在才应是 Err");
    }

    #[test]
    fn run_cmd_captures_stdout() {
        let out = run_cmd("cmd.exe", &["/c".into(), "echo hello".into()]).unwrap();
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("hello"));
    }

    #[test]
    fn read_dsh_version_at_missing_dir_is_none() {
        assert_eq!(read_dsh_version_at(&p("C:/definitely/not/here")), None);
    }

    /// ★ TR-4 的**判别性**测试（上面那条对 TR-4 没有牙齿）。
    ///
    /// `read_dsh_version_at_missing_dir_is_none` 在 `shim_in(dir)?` 就提前返回了，
    /// 命令根本没跑 —— 所以一个"经 PATH 解析"的错误实现照样能通过它（只要 PATH
    /// 上没有 dsh）。它断言的内容没错，但**测不到 TR-4**。
    ///
    /// 本测试在临时目录里放一个真实可执行的 `dsh.cmd`（内容是 `@echo 9.9.9-tr4probe`）：
    /// - 正确实现（执行该目录下的 shim）→ 拿到 `9.9.9-tr4probe` ✓
    /// - 错误实现（忽略 `dir`、走 PATH 解析）→ 拿到 **PATH 上那份真实 dsh 的版本**
    ///   （本机实测为 `0.1.6-alpha.2`）✗
    ///
    /// ⚠ 判别依据是**版本值**，不是"返回 None"。早先这里写的是"该目录不在 PATH，
    /// 所以错误实现返回 None" —— 那是**实测证伪的**：本机 dsh 确实在 PATH 上，
    /// 一个走 PATH 的实现会正常返回 `0.1.6-alpha.2`，断言同样失败，但失败方式不同。
    /// 真正让本测试有牙齿的是夹具里的 `9.9.9-tr4probe` 这个不可能撞上的版本号 ——
    /// 它让"读了哪个目录"变成可观测的差别。断言消息里两种失败都已点明。
    ///
    /// 顺带覆盖 `.cmd` shim 的真实 spawn 路径 —— 既有的 run_cmd 测试用的都是 cmd.exe。
    #[test]
    fn tr4_read_dsh_version_at_executes_shim_in_that_directory() {
        let dir = std::env::temp_dir().join(format!("dsh-mgr-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 一个最小但真实可执行的 .cmd shim
        std::fs::write(dir.join("dsh.cmd"), "@echo 9.9.9-tr4probe\r\n").unwrap();

        let got = read_dsh_version_at(&dir);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            got,
            Some(v("9.9.9-tr4probe")),
            "必须执行【该目录下】的 shim。返回 None 或【真实 dsh 的版本】都说明实现走了 PATH 解析（TR-4 违规）"
        );
    }

    /// ★ 退出码守卫的**判别性**测试 —— Finding 1 的回归保护。
    ///
    /// 这个 shim **打印一个完全合法的版本号，然后以非零码退出** —— 正是
    /// `read_dsh_version_on_path` 早先会误接受的那种形状。共享函数
    /// `version_from_shim` 必须拒绝它。
    ///
    /// 之所以能直接单测而不必改进程 PATH：`version_from_shim` 接收 shim 路径
    /// 作为参数。若把它写成内部直接调 `find_dsh_on_path`，这条测试就写不出来 ——
    /// 那正是这个抽象存在的第二个理由。
    #[test]
    fn version_from_shim_rejects_nonzero_exit() {
        let dir = std::env::temp_dir().join(format!("dsh-mgr-badshim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shim = dir.join("dsh.cmd");
        // 先打印合法版本号，再以非零码退出
        std::fs::write(&shim, "@echo 9.9.9-tr4probe
@exit /b 3
").unwrap();

        let got = version_from_shim(&shim);
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            got, None,
            "stdout 可解析但退出码非零的 shim 必须被拒绝；返回 Some 说明退出码守卫被删掉了"
        );
    }

    // ═══ FR-3 第 3 步：全局包列表解析（I-3）═══
    //
    // 下面四条 fixture 都是**真实输出形状**，捕获自本机：
    //   npm  `npm ls -g --depth=0`          （实测，2026-09-20，路径已替换为占位符）
    //   pnpm `pnpm list -g --depth=0`       （实测，2026-09-20）
    //   bun / yarn                          （本机未安装，取自各自官方输出的公开形状）
    // 只有 dsh 那一行是按同样格式**加**进 pnpm/bun/yarn 的 fixture 的 ——
    // 本机 pnpm 的全局列表里没有 dsh（dsh 装在 npm 上）。

    /// 本机实测的 npm 形状。注意树状前缀是 **ASCII** 的 `+--` / `` `-- ``
    /// （`npm ls` 在输出被重定向时不用 Unicode 制表符）。
    const NPM_LIST: &str = "\
C:\\Users\\<user>\\AppData\\Roaming\\npm
+-- @colbymchenry/codegraph@1.6.0
+-- @deepseek-ai/dsh@0.1.6-alpha.2
+-- @earendil-works/pi-coding-agent@0.85.1
+-- npm@12.0.2
";

    /// 本机实测的 pnpm 形状：**Unicode** 树状前缀 + `Legend:` 行 + 路径行（含 `(PRIVATE)`）。
    const PNPM_LIST: &str = "\
Legend: production dependency, optional only, dev only

C:\\Users\\<user>\\AppData\\Local\\pnpm\\global\\v11 (PRIVATE)
│
│   dependencies:
├── @pnpm/exe@11.24.0
├── @deepseek-ai/dsh@0.1.6-alpha.2
└── pnpm@12.3.4
";

    /// pnpm 的原生列表格式：包名与版本是**两个字段**（无 `@` 连接）。
    const PNPM_TWO_FIELD: &str = "\
C:\\Users\\<user>\\AppData\\Local\\pnpm\\global\\v11 (PRIVATE)
dependencies:
@deepseek-ai/dsh 0.1.6-alpha.2
";

    /// bun `pm ls -g`：路径行带 `node_modules (N)` 后缀，条目是 `├── pkg@ver`。
    const BUN_LIST: &str = "\
C:\\Users\\<user>\\.bun\\install\\global node_modules (2)
├── @deepseek-ai/dsh@0.1.6-alpha.2
└── typescript@5.5.4
";

    /// yarn v1 `global list`：`info` 噪声行 + `└─ pkg@ver`。
    const YARN_LIST: &str = "\
yarn global v1.22.22
info \"fsevents@2.3.2\" has binaries, but yarn does not support binaries for fsevents yet
warning @deepseek-ai/dsh@0.1.6-alpha.2 has unmet peer dependency zod@3.22.4
└─ @deepseek-ai/dsh@0.1.6-alpha.2
Done in 0.31s.
";

    #[test]
    fn parse_global_list_reads_npm_ascii_tree() {
        assert_eq!(
            parse_global_list(Pm::Npm, NPM_LIST),
            Some(v("0.1.6-alpha.2")),
            "npm 的 `+-- pkg@ver` 行"
        );
    }

    #[test]
    fn parse_global_list_reads_pnpm_unicode_tree() {
        assert_eq!(parse_global_list(Pm::Pnpm, PNPM_LIST), Some(v("0.1.6-alpha.2")));
    }

    #[test]
    fn parse_global_list_reads_pnpm_two_field_form() {
        // 包名与版本分开两个字段 —— 这是"扫描相邻字段"那条规则存在的唯一理由
        assert_eq!(
            parse_global_list(Pm::Pnpm, PNPM_TWO_FIELD),
            Some(v("0.1.6-alpha.2"))
        );
    }

    #[test]
    fn parse_global_list_reads_bun_tree() {
        assert_eq!(parse_global_list(Pm::Bun, BUN_LIST), Some(v("0.1.6-alpha.2")));
    }

    #[test]
    fn parse_global_list_reads_yarn_v1_list() {
        assert_eq!(
            parse_global_list(Pm::Yarn, YARN_LIST),
            Some(v("0.1.6-alpha.2")),
            "yarn 的 warning 行与真正的 `└─` 行都提到 dsh，取到的必须是同一个版本"
        );
    }

    /// 没有 dsh 的列表必须返回 `None` —— 否则"哪个 PM 是 owner"会由噪声决定。
    #[test]
    fn parse_global_list_returns_none_when_package_absent() {
        let no_dsh = "\
C:\\Users\\<user>\\AppData\\Roaming\\npm
+-- @colbymchenry/codegraph@1.6.0
+-- npm@12.0.2
";
        assert_eq!(parse_global_list(Pm::Npm, no_dsh), None);
        assert_eq!(parse_global_list(Pm::Npm, ""), None);
    }

    /// ★ 判别性：包名必须**整字段**匹配。
    ///
    /// 列表第一行是安装目录。用户名或路径里带 `dsh` 完全正常（本机就有 `dsh-manager`
    /// 这个目录），一个用 `contains("dsh")` 的实现会把路径行当成包名，再往下找到
    /// **别的包的版本**（这里埋了 `9.9.9-bogus`）当成 dsh 的版本 —— 那会让退回路径
    /// 报出一个根本不存在的版本，甚至把 owner 判给错误的 PM。
    #[test]
    fn parse_global_list_requires_whole_field_match() {
        let trap = "\
C:\\Work\\Projects\\Tools\\dsh-manager\\target\\fakebin
+-- some-other-pkg@9.9.9-bogus
";
        assert_eq!(
            parse_global_list(Pm::Npm, trap),
            None,
            "路径里的 `dsh` 与别的包的版本号都不能被当成 dsh 的安装记录"
        );

        // 反过来，短名（不带 scope）的列表行必须**能**认出来
        assert_eq!(
            parse_global_list(Pm::Npm, "dependencies:\ndsh 0.1.6-alpha.2\n"),
            Some(v("0.1.6-alpha.2"))
        );
        assert_eq!(
            parse_global_list(Pm::Npm, "+-- dsh@0.1.6-alpha.2\n"),
            Some(v("0.1.6-alpha.2"))
        );
    }

    /// `list_args` 的**每个** PM 都必须真的能跑（参数表写错就起不来）。
    ///
    /// 只对**本机确实装了**的 PM 断言（bun / yarn 未安装，见 VERIFICATION 的已知限制）：
    /// 命令跑不起来（`Err`）不算失败 —— 那是"该 PM 不存在"，与参数表无关；
    /// 但**跑起来了却连 stdout 都是空**就说明参数表错了。
    #[test]
    fn list_args_run_for_installed_pms() {
        for pm in Pm::ALL {
            if probe_pm(pm).is_none() {
                continue; // 该 PM 未安装 —— 与本条无关
            }
            let args: Vec<String> = pm.list_args().iter().map(|s| s.to_string()).collect();
            let out = run_cmd(pm.exe(), &args).expect("已探测到可用的 PM，列表命令必须能跑起来");
            assert!(
                !out.stdout.trim().is_empty(),
                "{pm:?} 的 list_args {:?} 打不出任何 stdout —— 参数表写错了",
                pm.list_args()
            );
        }
    }
}
