use std::env;
use std::path::{Path, PathBuf};

use crate::config::Config;

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
    local_root_in(&cwd, name)
}

/// Build an `Identity` from a config file path and its loaded config.
/// `root` comes from the config's `root` field when set & non-empty, otherwise
/// from the config file's parent directory. `libexec` comes from the config's
/// `libexec` field (a relative path resolved against `root`, an absolute path
/// used as-is), defaulting to `<root>/libexec`. The config path is canonicalized.
pub fn resolve(config_path: &Path, config: &Config) -> Option<Identity> {
    let canon = config_path.canonicalize().ok()?;
    let root = match config.root.as_deref() {
        Some(r) if !r.is_empty() => PathBuf::from(r),
        _ => canon.parent()?.to_path_buf(),
    };
    let libexec = libexec_dir(&root, config.libexec.as_deref());
    Some(Identity {
        name: config.name.clone(),
        root,
        libexec,
        local_root: local_root(&config.name),
        config_path: canon,
    })
}

/// Resolve the program's libexec directory from the configured value: a
/// relative path is joined onto `root`, an absolute path is used as-is, and an
/// unset/empty value defaults to `<root>/libexec`.
fn libexec_dir(root: &Path, configured: Option<&str>) -> PathBuf {
    match configured {
        Some(p) if !p.is_empty() => {
            let p = PathBuf::from(p);
            if p.is_absolute() {
                p
            } else {
                root.join(p)
            }
        }
        _ => root.join("libexec"),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    /// The directory holding the program's command executables. See
    /// [`libexec_dir`]; defaults to `<root>/libexec`.
    pub libexec: PathBuf,
    pub local_root: Option<PathBuf>,
    pub config_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn libexec_defaults_to_root_libexec() {
        assert_eq!(
            libexec_dir(Path::new("/opt/rush"), None),
            PathBuf::from("/opt/rush/libexec")
        );
    }

    #[test]
    fn libexec_relative_is_joined_to_root() {
        assert_eq!(
            libexec_dir(Path::new("/opt/rush"), Some("src/cmds")),
            PathBuf::from("/opt/rush/src/cmds")
        );
    }

    #[test]
    fn libexec_absolute_is_used_as_is() {
        assert_eq!(
            libexec_dir(Path::new("/opt/rush"), Some("/usr/lib/rush/cmds")),
            PathBuf::from("/usr/lib/rush/cmds")
        );
    }

    #[test]
    fn libexec_empty_value_defaults() {
        assert_eq!(
            libexec_dir(Path::new("/opt/rush"), Some("")),
            PathBuf::from("/opt/rush/libexec")
        );
    }

    #[test]
    fn resolve_reads_libexec_from_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nlibexec: src/cmds\n").unwrap();
        let cfg = crate::config::load(&path).unwrap();
        let id = resolve(&path, &cfg).unwrap();
        assert_eq!(
            id.libexec,
            dir.path().canonicalize().unwrap().join("src/cmds")
        );
    }
}
