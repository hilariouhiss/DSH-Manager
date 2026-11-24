//! npm 输出解析与全局安装目录读取。

use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Default, serde::Serialize)]
pub struct Versions {
    pub tags: BTreeMap<String, String>,
    pub versions: Vec<String>,
}

/// 安装目标只允许版本号或 dist-tag 名，杜绝命令行注入。
pub fn validate_spec(spec: &str) -> bool {
    !spec.is_empty()
        && spec.len() <= 64
        && spec
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
}

fn absorb(
    obj: &Map<String, Value>,
    tags: &mut BTreeMap<String, String>,
    versions: &mut Vec<String>,
) {
    if let Some(Value::Array(items)) = obj.get("versions") {
        for item in items {
            if let Some(value) = item.as_str() {
                versions.push(value.to_string());
            }
        }
    }
    if let Some(Value::Object(map)) = obj.get("dist-tags") {
        for (name, value) in map {
            if let Some(value) = value.as_str() {
                tags.insert(name.clone(), value.to_string());
            }
        }
    }
}

/// npm view 多字段的 --json 输出可能是数组包对象，也可能直接是对象，两种都接受。
pub fn parse_versions(text: &str) -> Result<Versions, String> {
    let value: Value = serde_json::from_str(text.trim())
        .map_err(|error| format!("解析 npm 输出失败: {error}"))?;
    let mut result = Versions::default();
    match value {
        Value::Array(items) => {
            for item in items {
                if let Value::Object(obj) = item {
                    absorb(&obj, &mut result.tags, &mut result.versions);
                }
            }
        }
        Value::Object(obj) => absorb(&obj, &mut result.tags, &mut result.versions),
        _ => return Err("npm 输出格式无法识别".to_string()),
    }
    let mut seen = std::collections::BTreeSet::new();
    result.versions.retain(|version| seen.insert(version.clone()));
    if result.versions.is_empty() && result.tags.is_empty() {
        return Err("npm 未返回任何版本信息".to_string());
    }
    Ok(result)
}

pub fn read_installed_version(npm_root: &Path) -> Option<String> {
    let manifest = npm_root.join("@deepseek-ai").join("dsh").join("package.json");
    let text = std::fs::read_to_string(manifest).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value.get("version")?.as_str().map(|version| version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_array_shape() {
        let text = r#"[{"versions":["1.0.0","1.1.0"],"dist-tags":{"latest":"1.1.0"}}]"#;
        let parsed = parse_versions(text).unwrap();
        assert_eq!(parsed.versions, vec!["1.0.0", "1.1.0"]);
        assert_eq!(parsed.tags.get("latest").unwrap(), "1.1.0");
    }

    #[test]
    fn parses_split_array_shape() {
        let text = r#"[{"versions":["2.0.0"]},{"dist-tags":{"alpha":"2.0.0"}}]"#;
        let parsed = parse_versions(text).unwrap();
        assert_eq!(parsed.tags.get("alpha").unwrap(), "2.0.0");
    }

    #[test]
    fn parses_object_shape() {
        let text = r#"{"versions":["3.0.0"],"dist-tags":{"latest":"3.0.0"}}"#;
        let parsed = parse_versions(text).unwrap();
        assert_eq!(parsed.versions, vec!["3.0.0"]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_versions("not json").is_err());
        assert!(parse_versions("[]").is_err());
    }

    #[test]
    fn validates_spec() {
        assert!(validate_spec("0.2.3-alpha.2"));
        assert!(validate_spec("latest"));
        assert!(!validate_spec(""));
        assert!(!validate_spec("1.0.0; rm -rf /"));
        assert!(!validate_spec("$(whoami)"));
        assert!(!validate_spec("a b"));
    }
}
