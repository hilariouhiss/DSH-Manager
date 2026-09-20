//! 版本拉取、更新说明与 `dsh web` 进程监督。

use std::collections::BTreeMap;
use std::time::Duration;

use crate::model::*;
use crate::pm;

pub const REGISTRY_URL: &str = "https://registry.npmjs.org/@deepseek-ai/dsh";
pub const GITHUB_REPO: &str = "deepseek-ai/deepseek-harness";
/// GC-5：只允许 registry.npmjs.org 与 api.github.com。github.com 实测不可达。
pub const USER_AGENT: &str = "dsh-manager";

/// NFR-3：全局超时上限，超时后进入失败路径而非无限等待。
pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// 纯函数，可用 fixture 单元测试。
pub fn parse_catalog(body: &str) -> Result<Catalog, String> {
    let v: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("registry 响应解析失败: {e}"))?;

    let mut versions: Vec<Version> = Vec::new();
    if let Some(obj) = v.get("versions").and_then(|x| x.as_object()) {
        for key in obj.keys() {
            if let Ok(ver) = key.parse::<Version>() {
                versions.push(ver);
            }
        }
    }

    let mut tags: BTreeMap<String, Version> = BTreeMap::new();
    if let Some(obj) = v.get("dist-tags").and_then(|x| x.as_object()) {
        for (key, val) in obj {
            if let Some(s) = val.as_str() {
                if let Ok(ver) = s.parse::<Version>() {
                    tags.insert(key.clone(), ver);
                }
            }
        }
    }

    Ok(Catalog { versions: pm::sorted_desc(versions), tags })
}

/// FR-6。精简 packument 请求头可显著减小响应体积 —— 本需求只需要
/// `versions` 的键集合与 `dist-tags`，不需要每个版本的完整元数据。
pub fn fetch_catalog() -> Result<Catalog, String> {
    let body = agent()
        .get(REGISTRY_URL)
        .header("Accept", "application/vnd.npm.install-v1+json")
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("registry 请求失败: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("registry 响应读取失败: {e}"))?;
    parse_catalog(&body)
}

/// 删除裸 HTML 标签，**保留标签内的文本**。
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}

/// 识别 ATX 标题，返回井号之后的原文（含前导空格）。空标题返回 None。
fn heading_body(s: &str) -> Option<&str> {
    let hashes = s.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    // '#' 是 ASCII，字节索引等于字符数，安全
    let rest = &s[hashes..];
    if !rest.starts_with(' ') || rest.trim().is_empty() {
        return None;
    }
    Some(rest)
}

/// 这一行是否是 HTML 标题（`<h1>` ~ `<h6>`）？
///
/// 必须单独识别：DSH 的 release notes **同时**使用两种标题写法 ——
/// 语言段用 `<h3 id="cn-...">新增功能</h3>`，小节用 `### 体验优化`。
/// 若只处理 ATX 一种，HTML 那批（恰恰是层级最高的段标题）会以纯文本出现，
/// 与小节标题的粗体**视觉不一致** —— 那正是 FR-27 要消除的"垃圾文本"问题。
fn is_html_heading(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    (1..=6).any(|n| lower.contains(&format!("<h{n}")))
}

/// FR-27 的强制预处理。
///
/// Slint 的 `StyledText` 官方 Currently Unsupported 列表包含 **Headings** 与
/// **Other HTML tags**。而 DSH 的 release notes 通篇是 `### 新增功能` 这类 ATX
/// 标题，且混有 `<h3 id="...">` 裸 HTML。不预处理就会原样显示成垃圾文本。
///
/// 只做两件事：剥离 HTML 标签、标题降级为粗体。
/// 其余语法（粗体 / 斜体 / 行内代码 / **链接** / 列表）由 StyledText 原生支持，
/// **不做干预** —— 尤其不要破坏链接。
pub fn preprocess_notes(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 32);
    for line in md.lines() {
        let stripped = strip_html(line);
        // 两种标题来源都要认：ATX（`### x`）与 HTML（`<h3>x</h3>`）。
        // 先判 HTML —— 它在 strip_html 之后就认不出来了。
        let heading = if is_html_heading(line) {
            Some(stripped.trim())
        } else {
            heading_body(stripped.trim_start()).map(|r| r.trim())
        };
        match heading {
            Some(text) if !text.is_empty() => {
                out.push_str("**");
                out.push_str(text);
                out.push_str("**\n");
            }
            _ => {
                out.push_str(&stripped);
                out.push('\n');
            }
        }
    }
    out
}

/// 从 GitHub release 响应中取 `body`。纯函数，可单测。
pub fn parse_release_body(json: &str) -> Result<String, NotesError> {
    let v: serde_json::Value =
        serde_json::from_str(json).map_err(|e| NotesError::Net(e.to_string()))?;
    match v.get("body").and_then(|b| b.as_str()) {
        Some(b) if !b.trim().is_empty() => Ok(b.to_string()),
        // body 为 null 或空 —— 与"该版本无 release"对用户是同一件事
        _ => Err(NotesError::Missing),
    }
}

/// FR-26。404 是**正常情况**：npm 有 22 个版本，GitHub 只有 18 个 release，
/// 有 6 个 npm 版本根本没有更新说明（SRS §2.2.6）。
pub fn fetch_notes(version: &Version) -> Result<String, NotesError> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/tags/dsh-v{version}");
    let mut resp = match agent()
        .get(&url)
        .header("User-Agent", USER_AGENT) // GitHub API 对无 UA 的请求返回 403
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        // ureq 3 默认把 4xx/5xx 变成 Err(StatusCode)
        Err(ureq::Error::StatusCode(404)) => return Err(NotesError::Missing),
        Err(e) => return Err(NotesError::Net(e.to_string())),
    };
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| NotesError::Net(e.to_string()))?;
    parse_release_body(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    /// 取自 registry 真实响应的精简形态
    const FIXTURE: &str = r#"{
      "name": "@deepseek-ai/dsh",
      "dist-tags": { "latest": "0.1.5-rc.2", "next": "0.1.5-rc.2", "alpha": "0.1.6-alpha.2" },
      "versions": {
        "0.1.5-rc.2": { "name": "@deepseek-ai/dsh", "version": "0.1.5-rc.2" },
        "0.1.6-alpha.1": { "name": "@deepseek-ai/dsh", "version": "0.1.6-alpha.1" },
        "0.1.6-alpha.2": { "name": "@deepseek-ai/dsh", "version": "0.1.6-alpha.2" }
      }
    }"#;

    #[test]
    fn parse_catalog_extracts_versions_descending() {
        let c = parse_catalog(FIXTURE).unwrap();
        assert_eq!(c.versions.len(), 3);
        assert_eq!(c.versions[0], v("0.1.6-alpha.2"), "最新应在最前");
        assert_eq!(c.versions[2], v("0.1.5-rc.2"));
    }

    #[test]
    fn parse_catalog_extracts_dist_tags() {
        let c = parse_catalog(FIXTURE).unwrap();
        assert_eq!(c.tags.get("latest"), Some(&v("0.1.5-rc.2")));
        assert_eq!(c.tags.get("alpha"), Some(&v("0.1.6-alpha.2")));
    }

    /// GC-14 的端到端回归：从真实形状的响应出发，
    /// 走完 parse → channel_of → latest_in，必须得到 0.1.6-alpha.2 而非 latest tag。
    #[test]
    fn gc14_latest_tag_must_not_be_used_as_newest() {
        let c = parse_catalog(FIXTURE).unwrap();
        let installed = v("0.1.6-alpha.2");
        let ch = pm::channel_of(&installed);
        let newest = pm::latest_in(&c.versions, ch).unwrap();
        assert_eq!(*newest, installed, "已是最新");
        assert_ne!(
            newest,
            c.tags.get("latest").unwrap(),
            "若这里相等，说明实现误用了 latest tag"
        );
    }

    #[test]
    fn parse_catalog_tolerates_missing_fields() {
        let c = parse_catalog("{}").unwrap();
        assert!(c.versions.is_empty());
        assert!(c.tags.is_empty());
    }

    #[test]
    fn parse_catalog_rejects_invalid_json() {
        assert!(parse_catalog("not json").is_err());
    }

    #[test]
    fn parse_catalog_skips_unparseable_version_keys() {
        let c = parse_catalog(r#"{"versions":{"not-a-version":{},"1.2.3":{}}}"#).unwrap();
        assert_eq!(c.versions, vec![v("1.2.3")]);
    }

    #[test]
    fn strip_html_removes_tags_but_keeps_inner_text() {
        // DSH 的 release notes 正文混有裸 HTML，如 <h3 id="cn-...">新增功能</h3>
        assert_eq!(strip_html(r#"<h3 id="cn-v0.1.6-alpha.2">新增功能</h3>"#), "新增功能");
        assert_eq!(strip_html("无标签"), "无标签");
        assert_eq!(strip_html("<b>粗</b>体"), "粗体");
    }

    #[test]
    fn heading_body_detects_atx_headings() {
        assert_eq!(heading_body("### 新增功能"), Some(" 新增功能"));
        assert_eq!(heading_body("# 一级"), Some(" 一级"));
        assert_eq!(heading_body("###### 六级"), Some(" 六级"));
        // 非标题
        assert_eq!(heading_body("普通文本"), None);
        assert_eq!(heading_body("####### 七个井号不是标题"), None);
        assert_eq!(heading_body("#没空格不是标题"), None);
        assert_eq!(heading_body("### "), None, "空标题按普通文本处理");
    }

    /// FR-27 的核心：StyledText 不支持标题与 HTML 标签。
    /// 不预处理的话，release notes 会显示成 `### 新增功能` 和
    /// `<h3 id="...">` 这样的垃圾文本。
    #[test]
    fn preprocess_converts_headings_to_bold_and_strips_html() {
        let input = "<h3 id=\"cn-x\">新增功能</h3>\n\n### 体验优化\n\n- 某条目\n";
        let out = preprocess_notes(input);
        assert!(!out.contains('<'), "HTML 标签应被剥离，得到 {out:?}");
        assert!(!out.contains("###"), "ATX 标题应被转换，得到 {out:?}");
        assert!(out.contains("**新增功能**"), "得到 {out:?}");
        assert!(out.contains("**体验优化**"), "得到 {out:?}");
        assert!(out.contains("- 某条目"), "列表语法应保持原样交给 StyledText");
    }

    #[test]
    fn preprocess_preserves_bilingual_nav_and_links() {
        // StyledText 原生支持链接，不该破坏它们
        let input = "[中文](#cn-x) | [English](#en-x)\n";
        let out = preprocess_notes(input);
        assert!(out.contains("[中文](#cn-x)"), "链接应原样保留，得到 {out:?}");
    }

    #[test]
    fn preprocess_handles_empty_input() {
        assert_eq!(preprocess_notes(""), "");
    }

    #[test]
    fn parse_release_body_extracts_body_field() {
        // 不能写成 r#"..."#：JSON 里 `"## 标题` 紧跟的 "# 会提前终止 raw string
        // （Rust 2024 又禁用了 r##"）。原文与断言均保持 brief 给定的形状。
        let json = concat!(
            r#"{"tag_name":"dsh-v0.1.6-alpha.2","body":""#,
            "## 标题\\n内容",
            r#""}"#
        );
        assert_eq!(parse_release_body(json).unwrap(), "## 标题\n内容");
    }

    #[test]
    fn parse_release_body_treats_null_body_as_missing() {
        // 有些 release 的 body 是 null
        let json = r#"{"tag_name":"dsh-v0.0.1-rc.1","body":null}"#;
        assert!(matches!(parse_release_body(json), Err(NotesError::Missing)));
    }
}
