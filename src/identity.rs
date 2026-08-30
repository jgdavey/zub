use std::env;
use std::path::{Path, PathBuf};

use crate::config::Config;

/// The default command roots when `command_roots` is unset: the program's
/// `<root>/libexec` (the base layer) overlaid by the local `.<name>/libexec`,
/// discovered by walking up from the current directory.
const DEFAULT_COMMAND_ROOTS: [&str; 2] = ["$ZUB_ROOT/libexec", "$ZUB_LOCAL_ROOT/libexec"];

/// The pseudo-variable naming the discovered local root — the `.<name>`
/// directory itself, so the `.<name>` part is never respelled in a template. A
/// template using it is working-directory-local, and is dropped entirely when
/// the walk finds nothing.
const LOCAL_ROOT_VAR: &str = "$ZUB_LOCAL_ROOT";

/// One discovered command-source directory and whether it is working-directory
/// local (its template referenced `$PWD` or `$ZUB_LOCAL_ROOT`; such commands
/// are flagged `(local)`).
#[derive(Debug, Clone, PartialEq)]
pub struct CommandRoot {
    pub path: PathBuf,
    pub is_local: bool,
}

/// Build an `Identity` from a config file path and its loaded config.
/// `root` comes from the config's `root` field when set & non-empty, otherwise
/// from the config file's parent directory. `command_roots` comes from the
/// config's `command_roots` field (defaulting to [`DEFAULT_COMMAND_ROOTS`]),
/// with each entry's `$ZUB_ROOT`/`$ZUB_INSTANCE`/`$PWD`/`$ZUB_LOCAL_ROOT`
/// pseudo-variables expanded. The config path is canonicalized.
pub fn resolve(config_path: &Path, config: &Config) -> Option<Identity> {
    let canon = config_path.canonicalize().ok()?;
    let root = match config.root.as_deref() {
        Some(r) if !r.is_empty() => PathBuf::from(r),
        _ => canon.parent()?.to_path_buf(),
    };
    let cwd = env::current_dir().unwrap_or_default();
    let templates = effective_templates(config.command_roots.as_deref());
    // Only walk the tree when some template actually asks for the local root.
    let local_root = templates
        .iter()
        .any(|t| t.contains(LOCAL_ROOT_VAR))
        .then(|| find_local_root(&cwd, &format!(".{}", config.name)))
        .flatten();
    let command_roots = command_roots(&templates, &root, &config.name, &cwd, local_root.as_deref());
    Some(Identity {
        name: config.name.clone(),
        root,
        command_roots,
        local_root,
        config_path: canon,
        version: config.version.clone(),
        description: config.description.clone(),
    })
}

/// Resolve command-root templates into absolute `CommandRoot`s, expanding
/// pseudo-variables. A template is flagged `is_local` when it references `$PWD`
/// or `$ZUB_LOCAL_ROOT`; one referencing an unresolved `$ZUB_LOCAL_ROOT` is
/// dropped.
fn command_roots(
    templates: &[String],
    root: &Path,
    name: &str,
    cwd: &Path,
    local_root: Option<&Path>,
) -> Vec<CommandRoot> {
    templates
        .iter()
        .filter_map(|template| {
            let expanded = expand_pseudo_vars(template, root, name, cwd, local_root)?;
            // A still-relative entry (e.g. a bare `cmds`) resolves against root.
            let path = if expanded.is_relative() {
                root.join(expanded)
            } else {
                expanded
            };
            Some(CommandRoot {
                path,
                is_local: is_local(template),
            })
        })
        .collect()
}

/// Whether a template is working-directory-local: it is anchored at the current
/// directory or at the local root discovered by walking up from it.
fn is_local(template: &str) -> bool {
    template.contains("$PWD") || template.contains(LOCAL_ROOT_VAR)
}

/// The command-root templates actually in force: the configured list when
/// present and non-empty, else [`DEFAULT_COMMAND_ROOTS`].
fn effective_templates(configured: Option<&[String]>) -> Vec<String> {
    match configured {
        Some(list) if !list.is_empty() => list.to_vec(),
        _ => DEFAULT_COMMAND_ROOTS
            .iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

/// Walk up from `start` (inclusive) looking for the first directory that
/// contains `marker` as a directory, and return **that marker directory** — it
/// is the local counterpart of `root`, holding `libexec`/`share` just as the
/// program root does. `None` when the filesystem root is reached without a
/// match.
fn find_local_root(start: &Path, marker: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(marker))
        .find(|candidate| candidate.is_dir())
}

/// Expand the supported pseudo-variables in a command-root template:
/// `$ZUB_ROOT` -> `root`, `$ZUB_INSTANCE` -> the program name, `$PWD` -> the
/// current working directory, `$ZUB_LOCAL_ROOT` -> the discovered local root.
/// Returns `None` when the template needs a local root that wasn't found, so
/// the caller drops the entry rather than expanding it against an empty path.
fn expand_pseudo_vars(
    template: &str,
    root: &Path,
    name: &str,
    cwd: &Path,
    local_root: Option<&Path>,
) -> Option<PathBuf> {
    let expanded = if template.contains(LOCAL_ROOT_VAR) {
        template.replace(LOCAL_ROOT_VAR, &local_root?.to_string_lossy())
    } else {
        template.to_string()
    };
    let expanded = expanded
        .replace("$ZUB_ROOT", &root.to_string_lossy())
        .replace("$ZUB_INSTANCE", name)
        .replace("$PWD", &cwd.to_string_lossy());
    Some(PathBuf::from(expanded))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    /// Directories to collect commands from, lowest-precedence first (a later
    /// root overrides an earlier one on a name collision).
    pub command_roots: Vec<CommandRoot>,
    /// The `.<name>` directory of the nearest ancestor of the current directory
    /// (inclusive) that has one, when any template asked for it and the walk
    /// found one. Exported to subcommands as `ZUB_LOCAL_ROOT`.
    pub local_root: Option<PathBuf>,
    pub config_path: PathBuf,
    /// The program's version, from the config's `version` field (for the help
    /// header). `None` when unset.
    pub version: Option<String>,
    /// The program's one-line description, from the config's `description` field
    /// (for the help header). `None` when unset.
    pub description: Option<String>,
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

/// Test helper: an `Identity` for `name` rooted at `root`, with the default
/// single command root (`<root>/libexec`) and config at `<root>/zub.yml`.
/// Shared across the crate's test modules so they don't each re-spell it;
/// mutate the returned fields for cases that need different roots.
#[cfg(test)]
pub(crate) fn fixture(name: &str, root: impl AsRef<Path>) -> Identity {
    let root = root.as_ref().to_path_buf();
    Identity {
        command_roots: vec![CommandRoot {
            path: root.join("libexec"),
            is_local: false,
        }],
        config_path: root.join("zub.yml"),
        name: name.to_string(),
        root,
        local_root: None,
        version: None,
        description: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn find_local_root_finds_marker_in_starting_dir() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".rush")).unwrap();
        assert_eq!(
            find_local_root(dir.path(), ".rush"),
            Some(dir.path().join(".rush"))
        );
    }

    #[test]
    fn find_local_root_finds_marker_in_ancestor() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".rush")).unwrap();
        let deep = dir.path().join("a/b");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            find_local_root(&deep, ".rush"),
            Some(dir.path().join(".rush"))
        );
    }

    #[test]
    fn find_local_root_prefers_the_nearest_marker() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join(".rush")).unwrap();
        let nearer = dir.path().join("a");
        fs::create_dir_all(nearer.join(".rush")).unwrap();
        let deep = nearer.join("b");
        fs::create_dir(&deep).unwrap();
        assert_eq!(find_local_root(&deep, ".rush"), Some(nearer.join(".rush")));
    }

    #[test]
    fn find_local_root_ignores_a_marker_that_is_a_file() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".rush"), "not a dir").unwrap();
        assert_eq!(find_local_root(dir.path(), ".rush"), None);
    }

    #[test]
    fn find_local_root_returns_none_when_no_marker_exists() {
        let dir = tempdir().unwrap();
        let deep = dir.path().join("a/b");
        fs::create_dir_all(&deep).unwrap();
        assert_eq!(
            find_local_root(&deep, ".zub-marker-that-does-not-exist"),
            None
        );
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

    /// The defaults, as `command_roots` sees them after template selection.
    fn defaults() -> Vec<String> {
        effective_templates(None)
    }

    #[test]
    fn expand_pseudo_vars_substitutes_all_four() {
        let expand = |template| {
            expand_pseudo_vars(
                template,
                Path::new("/opt/rush"),
                "rush",
                Path::new("/work/a/b"),
                Some(Path::new("/work/.rush")),
            )
        };
        assert_eq!(
            expand("$ZUB_ROOT/libexec"),
            Some(PathBuf::from("/opt/rush/libexec"))
        );
        assert_eq!(
            expand("$PWD/.$ZUB_INSTANCE/libexec"),
            Some(PathBuf::from("/work/a/b/.rush/libexec"))
        );
        assert_eq!(
            expand("$ZUB_LOCAL_ROOT/libexec"),
            Some(PathBuf::from("/work/.rush/libexec"))
        );
    }

    #[test]
    fn expand_pseudo_vars_drops_local_root_template_when_unresolved() {
        assert_eq!(
            expand_pseudo_vars(
                "$ZUB_LOCAL_ROOT/libexec",
                Path::new("/opt/rush"),
                "rush",
                Path::new("/work"),
                None,
            ),
            None
        );
    }

    #[test]
    fn command_roots_default_reproduces_root_then_local() {
        let roots = command_roots(
            &defaults(),
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work/a/b"),
            Some(Path::new("/work/.rush")),
        );
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
    fn command_roots_drops_local_entry_when_local_root_unresolved() {
        let roots = command_roots(
            &defaults(),
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work"),
            None,
        );
        assert_eq!(
            roots,
            vec![CommandRoot {
                path: PathBuf::from("/opt/rush/libexec"),
                is_local: false,
            }]
        );
    }

    #[test]
    fn command_roots_keeps_pwd_entries_when_local_root_unresolved() {
        let configured = vec!["$PWD/.$ZUB_INSTANCE/libexec".to_string()];
        let roots = command_roots(
            &configured,
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work/a/b"),
            None,
        );
        assert_eq!(
            roots,
            vec![CommandRoot {
                path: PathBuf::from("/work/a/b/.rush/libexec"),
                is_local: true,
            }]
        );
    }

    #[test]
    fn command_roots_uses_configured_list() {
        let configured = vec!["$ZUB_ROOT/libexec".to_string(), "/abs/cmds".to_string()];
        let roots = command_roots(
            &configured,
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work"),
            None,
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
            &configured,
            Path::new("/opt/rush"),
            "rush",
            Path::new("/work"),
            None,
        );
        assert_eq!(roots[0].path, PathBuf::from("/opt/rush/cmds"));
    }

    #[test]
    fn effective_templates_empty_list_falls_back_to_default() {
        assert_eq!(effective_templates(Some(&[])), defaults());
        assert_eq!(defaults().len(), 2);
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
            local_root: None,
            version: None,
            description: None,
        };
        assert_eq!(id.new_command_dir(), PathBuf::from("/opt/rush/cmds"));
    }

    #[test]
    fn resolve_carries_version_and_description() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(
            &path,
            "name: rush\nversion: 1.2.3\ndescription: do things\n",
        )
        .unwrap();
        let cfg = crate::config::load(&path).unwrap();
        let id = resolve(&path, &cfg).unwrap();
        assert_eq!(id.version.as_deref(), Some("1.2.3"));
        assert_eq!(id.description.as_deref(), Some("do things"));
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
