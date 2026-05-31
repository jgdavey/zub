use std::env;
use std::path::{Path, PathBuf};

use crate::config::Config;

/// The default command roots when `command_roots` is unset, reproducing the
/// historical local/root behavior: the program's `<root>/libexec` (the base
/// layer) overlaid by a per-directory `$PWD/.<name>/libexec`.
const DEFAULT_COMMAND_ROOTS: [&str; 2] = ["$ZUB_ROOT/libexec", "$PWD/.$ZUB_INSTANCE/libexec"];

/// One discovered command-source directory and whether it is working-directory
/// local (its template referenced `$PWD`; such commands are flagged `(local)`).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandRoot {
    pub path: PathBuf,
    pub is_local: bool,
}

/// Build an `Identity` from a config file path and its loaded config.
/// `root` comes from the config's `root` field when set & non-empty, otherwise
/// from the config file's parent directory. `command_roots` comes from the
/// config's `command_roots` field (defaulting to [`DEFAULT_COMMAND_ROOTS`]),
/// with each entry's `$ZUB_ROOT`/`$ZUB_INSTANCE`/`$PWD` pseudo-variables
/// expanded. The config path is canonicalized.
pub fn resolve(config_path: &Path, config: &Config) -> Option<Identity> {
    let canon = config_path.canonicalize().ok()?;
    let root = match config.root.as_deref() {
        Some(r) if !r.is_empty() => PathBuf::from(r),
        _ => canon.parent()?.to_path_buf(),
    };
    let cwd = env::current_dir().unwrap_or_default();
    let command_roots = command_roots(config.command_roots.as_deref(), &root, &config.name, &cwd);
    Some(Identity {
        name: config.name.clone(),
        root,
        command_roots,
        config_path: canon,
    })
}

/// Resolve the configured command-root templates (or the defaults) into
/// absolute `CommandRoot`s, expanding pseudo-variables. A template is flagged
/// `is_local` when it references `$PWD`.
fn command_roots(
    configured: Option<&[String]>,
    root: &Path,
    name: &str,
    cwd: &Path,
) -> Vec<CommandRoot> {
    let defaults: Vec<String> = DEFAULT_COMMAND_ROOTS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let templates = match configured {
        Some(list) if !list.is_empty() => list,
        _ => &defaults,
    };
    templates
        .iter()
        .map(|template| {
            let expanded = expand_pseudo_vars(template, root, name, cwd);
            // A still-relative entry (e.g. a bare `cmds`) resolves against root.
            let path = if expanded.is_relative() {
                root.join(expanded)
            } else {
                expanded
            };
            CommandRoot {
                path,
                is_local: template.contains("$PWD"),
            }
        })
        .collect()
}

/// Expand the supported pseudo-variables in a command-root template:
/// `$ZUB_ROOT` -> `root`, `$ZUB_INSTANCE` -> the program name, `$PWD` -> the
/// current working directory.
fn expand_pseudo_vars(template: &str, root: &Path, name: &str, cwd: &Path) -> PathBuf {
    let expanded = template
        .replace("$ZUB_ROOT", &root.to_string_lossy())
        .replace("$ZUB_INSTANCE", name)
        .replace("$PWD", &cwd.to_string_lossy());
    PathBuf::from(expanded)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    /// Directories to collect commands from, lowest-precedence first (a later
    /// root overrides an earlier one on a name collision).
    pub command_roots: Vec<CommandRoot>,
    pub config_path: PathBuf,
}

impl Identity {
    /// The directory where `new` creates a (non-local) command: the first
    /// non-local command root, else the first root, else `<root>/libexec`.
    pub fn new_command_dir(&self) -> PathBuf {
        self.command_roots
            .iter()
            .find(|r| !r.is_local)
            .or_else(|| self.command_roots.first())
            .map(|r| r.path.clone())
            .unwrap_or_else(|| self.root.join("libexec"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

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
    fn expand_pseudo_vars_substitutes_all_three() {
        assert_eq!(
            expand_pseudo_vars(
                "$ZUB_ROOT/libexec",
                Path::new("/opt/rush"),
                "rush",
                Path::new("/work")
            ),
            PathBuf::from("/opt/rush/libexec")
        );
        assert_eq!(
            expand_pseudo_vars(
                "$PWD/.$ZUB_INSTANCE/libexec",
                Path::new("/opt/rush"),
                "rush",
                Path::new("/work")
            ),
            PathBuf::from("/work/.rush/libexec")
        );
    }

    #[test]
    fn command_roots_default_reproduces_root_then_local() {
        let roots = command_roots(None, Path::new("/opt/rush"), "rush", Path::new("/work"));
        assert_eq!(
            roots,
            vec![
                CommandRoot {
                    path: PathBuf::from("/opt/rush/libexec"),
                    is_local: false,
                },
                CommandRoot {
                    path: PathBuf::from("/work/.rush/libexec"),
                    is_local: true,
                },
            ]
        );
    }

    #[test]
    fn command_roots_uses_configured_list() {
        let configured = vec!["$ZUB_ROOT/libexec".to_string(), "/abs/cmds".to_string()];
        let roots = command_roots(
            Some(&configured),
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work"),
        );
        assert_eq!(roots.len(), 2);
        assert_eq!(roots[0].path, PathBuf::from("/opt/rush/libexec"));
        assert!(!roots[0].is_local);
        assert_eq!(roots[1].path, PathBuf::from("/abs/cmds"));
        assert!(!roots[1].is_local);
    }

    #[test]
    fn command_roots_relative_entry_resolves_against_root() {
        let configured = vec!["cmds".to_string()];
        let roots = command_roots(
            Some(&configured),
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work"),
        );
        assert_eq!(roots[0].path, PathBuf::from("/opt/rush/cmds"));
    }

    #[test]
    fn command_roots_empty_list_falls_back_to_default() {
        let roots = command_roots(
            Some(&[]),
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work"),
        );
        assert_eq!(roots.len(), 2);
    }

    #[test]
    fn new_command_dir_picks_first_non_local() {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/opt/rush"),
            command_roots: vec![
                CommandRoot {
                    path: PathBuf::from("/work/.rush/libexec"),
                    is_local: true,
                },
                CommandRoot {
                    path: PathBuf::from("/opt/rush/cmds"),
                    is_local: false,
                },
            ],
            config_path: PathBuf::new(),
        };
        assert_eq!(id.new_command_dir(), PathBuf::from("/opt/rush/cmds"));
    }

    #[test]
    fn resolve_reads_command_roots_from_config() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\ncommand_roots:\n  - $ZUB_ROOT/cmds\n").unwrap();
        let cfg = crate::config::load(&path).unwrap();
        let id = resolve(&path, &cfg).unwrap();
        assert_eq!(id.command_roots.len(), 1);
        assert_eq!(
            id.command_roots[0].path,
            dir.path().canonicalize().unwrap().join("cmds")
        );
    }
}
