use std::path::{Path, PathBuf};

/// Derive the program name from the invoked path's final component.
pub fn name_from_argv0(argv0: &str) -> String {
    Path::new(argv0)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| argv0.to_string())
}

fn shout(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
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

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    pub local_root: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_from_plain_argv0() {
        assert_eq!(name_from_argv0("rush"), "rush");
    }

    #[test]
    fn name_from_path_argv0() {
        assert_eq!(name_from_argv0("/home/me/.rush/bin/rush"), "rush");
    }

    #[test]
    fn root_env_var_name_uppercases_and_substitutes() {
        assert_eq!(env_var_name("rush"), "_RUSH_ROOT");
        assert_eq!(env_var_name("my-tool"), "_MY_TOOL_ROOT");
    }

    #[test]
    fn local_env_var_name() {
        assert_eq!(env_var_name_local("rush"), "_RUSH_LOCAL_ROOT");
    }
}
