use std::env;
use std::path::{Path, PathBuf};

use crate::config::Config;

fn shout(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Name of the env var holding the program root, e.g. `rush` -> `_RUSH_ROOT`.
pub fn env_var_name(name: &str) -> String {
    format!("_{}_ROOT", shout(name))
}

/// Name of the env var holding the local-sub root, e.g. `_RUSH_LOCAL_ROOT`.
pub fn env_var_name_local(name: &str) -> String {
    format!("_{}_LOCAL_ROOT", shout(name))
}

/// The local-sub root for a working directory: `<cwd>/.<name>` when
/// `<cwd>/.<name>/libexec` exists.
pub fn local_root_in(cwd: &Path, name: &str) -> Option<PathBuf> {
    let dot_sub = cwd.join(format!(".{}", name));
    if dot_sub.join("libexec").is_dir() {
        Some(dot_sub)
    } else {
        None
    }
}

/// Convenience wrapper using the current working directory.
pub fn local_root(name: &str) -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    local_root_in(&cwd, &name)
}

/// Build an `Identity` from a config file path and its loaded config.
/// `root` comes from the config's `root` field when set & non-empty, otherwise
/// from the config file's parent directory. The config path is canonicalized.
pub fn resolve(config_path: &Path, config: &Config) -> Option<Identity> {
    let canon = config_path.canonicalize().ok()?;
    let root = match config.root.as_deref() {
        Some(r) if !r.is_empty() => PathBuf::from(r),
        _ => canon.parent()?.to_path_buf(),
    };
    Some(Identity {
        name: config.name.clone(),
        root,
        local_root: local_root(&config.name),
        config_path: canon,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    pub local_root: Option<PathBuf>,
    pub config_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_env_var_name_uppercases_and_substitutes() {
        assert_eq!(env_var_name("rush"), "_RUSH_ROOT");
        assert_eq!(env_var_name("my-tool"), "_MY_TOOL_ROOT");
    }

    #[test]
    fn local_env_var_name() {
        assert_eq!(env_var_name_local("rush"), "_RUSH_LOCAL_ROOT");
    }

    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn local_root_detected_when_dot_sub_libexec_exists() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".sub").join("libexec")).unwrap();
        assert_eq!(
            local_root_in(dir.path(), "sub"),
            Some(dir.path().join(".sub"))
        );
    }

    #[test]
    fn local_root_absent_otherwise() {
        let dir = tempdir().unwrap();
        assert_eq!(local_root_in(dir.path(), "x"), None);
    }

    #[test]
    fn resolve_uses_root_field_when_set() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nroot: /opt/rush\n").unwrap();
        let cfg = crate::config::load(&path).unwrap();
        let id = resolve(&path, &cfg).unwrap();
        assert_eq!(id.name, "rush");
        assert_eq!(id.root, PathBuf::from("/opt/rush"));
        assert_eq!(id.config_path, path.canonicalize().unwrap());
    }

    #[test]
    fn resolve_falls_back_to_config_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: walker\n").unwrap();
        let cfg = crate::config::load(&path).unwrap();
        let id = resolve(&path, &cfg).unwrap();
        assert_eq!(id.root, dir.path().canonicalize().unwrap());
    }
}
