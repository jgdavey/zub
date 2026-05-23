use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
pub struct Config {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

use std::fs;
use std::path::Path;

/// Load `sub.yml` (or `sub.yaml`) from `root`. Returns `None` when neither
/// exists or the file cannot be parsed.
pub fn load(root: &Path) -> Option<Config> {
    for filename in ["sub.yml", "sub.yaml"] {
        let path = root.join(filename);
        if let Ok(contents) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_yaml::from_str::<Config>(&contents) {
                return Some(cfg);
            }
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
    fn loads_sub_yml() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("sub.yml"), "name: rush\nversion: 0.2.0\n").unwrap();
        let cfg = load(dir.path()).unwrap();
        assert_eq!(cfg.name, "rush");
        assert_eq!(cfg.version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn accepts_yaml_extension() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("sub.yaml"), "name: tool\n").unwrap();
        assert_eq!(load(dir.path()).unwrap().name, "tool");
    }

    #[test]
    fn ignores_unknown_keys() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("sub.yml"), "name: rush\nfuture_key: 1\n").unwrap();
        assert_eq!(load(dir.path()).unwrap().name, "rush");
    }

    #[test]
    fn missing_config_returns_none() {
        let dir = tempdir().unwrap();
        assert!(load(dir.path()).is_none());
    }
}
