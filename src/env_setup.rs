use crate::identity::Identity;
use std::env;

pub const CONFIG: &str = "ZUB_CONFIG";
pub const INSTANCE: &str = "ZUB_INSTANCE";
pub const ROOT: &str = "ZUB_ROOT";
pub const LOCAL_ROOT: &str = "ZUB_LOCAL_ROOT";

/// Export the program's identity to every child process. We deliberately do
/// *not* add libexec to `PATH`: subcommands are reachable only through `zub`
/// (or the generated wrapper), never as standalone executables on `PATH`, so
/// they can't shadow built-ins, system commands, or each other. A subcommand
/// that needs a sibling re-enters via `zub` — `ZUB_CONFIG` is set for exactly
/// that.
pub fn apply(id: &Identity) {
    env::set_var(CONFIG, &id.config_path);
    env::set_var(ROOT, &id.root);
    env::set_var(INSTANCE, &id.name);
    // Removed rather than left alone when there is no local root: otherwise a
    // subcommand re-entering `zub` from a different directory would inherit a
    // stale value from the outer invocation.
    match &id.local_root {
        Some(dir) => env::set_var(LOCAL_ROOT, dir),
        None => env::remove_var(LOCAL_ROOT),
    }
}
