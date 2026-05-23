use crate::builtins::all_command_names;
use crate::builtins::Context;
use std::collections::BTreeSet;

/// Build the command-name list honoring the `--sh` / `--no-sh` filters.
/// The leading `sh-` prefix is stripped from displayed names.
pub fn collect(args: &[String], ctx: &Context) -> Vec<String> {
    let mode = args.first().map(String::as_str);
    let mut out = BTreeSet::new();
    for name in all_command_names(ctx) {
        let is_sh = name.starts_with("sh-");
        match mode {
            Some("--sh") if is_sh => {
                out.insert(name.strip_prefix("sh-").unwrap_or(&name).to_string());
            }
            Some("--sh") => {}
            Some("--no-sh") if is_sh => {}
            _ => {
                out.insert(name.strip_prefix("sh-").unwrap_or(&name).to_string());
            }
        }
    }
    out.into_iter().collect()
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        println!("--sh");
        println!("--no-sh");
        return 0;
    }
    for name in collect(args, ctx) {
        println!("{name}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::CommandInfo;
    use std::path::PathBuf;

    fn ctx_with(names: &[&str]) -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            local_root: None,
            config_path: PathBuf::new(),
        };
        let cmds = names
            .iter()
            .map(|n| CommandInfo {
                name: n.to_string(),
                path: PathBuf::from(format!("/r/libexec/rush-{n}")),
                front: FrontMatter::default(),
                is_local: false,
            })
            .collect();
        (id, None, cmds)
    }

    #[test]
    fn lists_builtins_and_externals_sorted() {
        let (id, cfg, cmds) = ctx_with(&["who"]);
        let ctx = Context {
            identity: &id,
            config: &cfg,
            commands: &cmds,
        };
        let out = collect(&[], &ctx);
        assert!(out.contains(&"who".to_string()));
        assert!(out.contains(&"help".to_string()));
        let mut sorted = out.clone();
        sorted.sort();
        assert_eq!(out, sorted);
    }

    #[test]
    fn sh_filter_strips_prefix() {
        let (id, cfg, cmds) = ctx_with(&["sh-cd", "who"]);
        let ctx = Context {
            identity: &id,
            config: &cfg,
            commands: &cmds,
        };
        let out = collect(&["--sh".to_string()], &ctx);
        assert_eq!(out, vec!["cd".to_string()]);
    }

    #[test]
    fn no_sh_filter_excludes_sh_commands() {
        let (id, cfg, cmds) = ctx_with(&["sh-cd", "who"]);
        let ctx = Context {
            identity: &id,
            config: &cfg,
            commands: &cmds,
        };
        let out = collect(&["--no-sh".to_string()], &ctx);
        assert!(out.contains(&"who".to_string()));
        assert!(!out.contains(&"cd".to_string()));
    }
}
