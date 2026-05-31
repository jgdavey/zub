use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct Config {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Directory holding the program's command executables. A relative path is
    /// resolved against `root`; an absolute path is used as-is. Defaults to
    /// `libexec` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub libexec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

use std::fmt;
use std::fs;
use std::path::Path;

/// An error loading a config file.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read (missing, unreadable, …).
    Read(std::io::Error),
    /// The file's contents were not valid YAML for a `Config`.
    Parse(yaml_serde::Error),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Read(e) => write!(f, "{e}"),
            LoadError::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Load a config from an explicit file path. Distinguishes a read failure
/// (missing/unreadable file) from a YAML parse error so callers can report the
/// underlying cause.
pub fn load(path: &Path) -> Result<Config, LoadError> {
    let contents = fs::read_to_string(path).map_err(LoadError::Read)?;
    yaml_serde::from_str::<Config>(&contents).map_err(LoadError::Parse)
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
    fn loads_config_with_libexec() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nlibexec: src/cmds\n").unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.libexec.as_deref(), Some("src/cmds"));
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
    fn missing_config_is_read_error() {
        let dir = tempdir().unwrap();
        assert!(matches!(
            load(&dir.path().join("nope.yml")),
            Err(LoadError::Read(_))
        ));
    }

    #[test]
    fn malformed_yaml_is_parse_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        // `name` is required, and this is not even a mapping.
        fs::write(&path, ":\n  - not\n - valid: yaml\n").unwrap();
        let err = load(&path).unwrap_err();
        assert!(matches!(err, LoadError::Parse(_)));
        // The message carries the underlying parser detail (e.g. line/column).
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn omits_none_fields_when_serialized() {
        let cfg = Config {
            name: "rush".into(),
            root: None,
            libexec: None,
            version: None,
            description: None,
        };
        let yaml = yaml_serde::to_string(&cfg).unwrap();
        assert!(yaml.contains("name: rush"));
        assert!(!yaml.contains("root"));
        assert!(!yaml.contains("version"));
    }
}
