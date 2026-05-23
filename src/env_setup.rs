use crate::identity;
use crate::identity::Identity;
use std::env;

/// Build the environment variables to export, given the current `PATH`.
/// Pure (no process mutation) so it can be unit-tested.
pub fn build_env(id: &Identity, current_path: &str) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    vars.push(("ORIG_PATH".to_string(), current_path.to_string()));

    let mut parts: Vec<String> = Vec::new();
    if let Some(local) = &id.local_root {
        parts.push(local.join("libexec").to_string_lossy().into_owned());
    }
    parts.push(id.root.join("libexec").to_string_lossy().into_owned());
    parts.push(id.root.join("bin").to_string_lossy().into_owned());
    if !current_path.is_empty() {
        parts.push(current_path.to_string());
    }
    vars.push(("PATH".to_string(), parts.join(":")));

    vars.push((
        identity::env_var_name(&id.name),
        id.root.to_string_lossy().into_owned(),
    ));
    let local_val = id
        .local_root
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    vars.push((identity::env_var_name_local(&id.name), local_val));

    vars
}

/// Compute the environment from the live `PATH` and apply it to this process,
/// so every child (built-in subprocess or `exec`'d command) inherits it.
pub fn apply(id: &Identity) {
    let current_path = env::var("PATH").unwrap_or_default();
    for (key, value) in build_env(id, &current_path) {
        env::set_var(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn lookup<'a>(vars: &'a [(String, String)], key: &str) -> Option<&'a str> {
        vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn exports_root_and_path_with_libexec_prepended() {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/opt/rush"),
            local_root: None,
        };
        let vars = build_env(&id, "/usr/bin:/bin");
        assert_eq!(lookup(&vars, "_RUSH_ROOT"), Some("/opt/rush"));
        assert_eq!(lookup(&vars, "ORIG_PATH"), Some("/usr/bin:/bin"));
        assert_eq!(
            lookup(&vars, "PATH"),
            Some("/opt/rush/libexec:/opt/rush/bin:/usr/bin:/bin")
        );
        assert_eq!(lookup(&vars, "_RUSH_LOCAL_ROOT"), Some(""));
    }

    #[test]
    fn local_libexec_comes_first() {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/opt/rush"),
            local_root: Some(PathBuf::from("/work/.rush")),
        };
        let vars = build_env(&id, "/bin");
        assert_eq!(
            lookup(&vars, "PATH"),
            Some("/work/.rush/libexec:/opt/rush/libexec:/opt/rush/bin:/bin")
        );
        assert_eq!(lookup(&vars, "_RUSH_LOCAL_ROOT"), Some("/work/.rush"));
    }
}
