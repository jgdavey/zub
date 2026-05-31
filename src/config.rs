use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct Config {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

use std::fs;
use std::path::Path;

/// Load a config from an explicit file path. Returns `None` when the file is
/// missing or cannot be parsed.
pub fn load(path: &Path) -> Option<Config> {
    if let Ok(contents) = fs::read_to_string(path) {
        if let Ok(cfg) = yaml_serde::from_str::<Config>(&contents) {
            return Some(cfg);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_config_with_optional_root() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nroot: /opt/rush\nversion: 0.2.0\n").unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.name, "rush");
        assert_eq!(cfg.root.as_deref(), Some("/opt/rush"));
        assert_eq!(cfg.version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn loads_config_without_root() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: tool\n").unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.name, "tool");
        assert_eq!(cfg.root, None);
    }

    #[test]
    fn ignores_unknown_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nfuture_key: 1\n").unwrap();
        assert_eq!(load(&path).unwrap().name, "rush");
    }

    #[test]
    fn missing_config_returns_none() {
        let dir = tempdir().unwrap();
        assert!(load(&dir.path().join("nope.yml")).is_none());
    }

    #[test]
    fn omits_none_fields_when_serialized() {
        let cfg = Config {
            name: "rush".into(),
            root: None,
            version: None,
            description: None,
        };
        let yaml = yaml_serde::to_string(&cfg).unwrap();
        assert!(yaml.contains("name: rush"));
        assert!(!yaml.contains("root"));
        assert!(!yaml.contains("version"));
    }
}
