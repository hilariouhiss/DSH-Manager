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
}
