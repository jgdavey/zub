use crate::builtins::Context;
use crate::builtins::BUILTINS;
use std::collections::BTreeSet;

/// Build the command-name list honoring the `--eval` / `--no-eval` filters.
/// An eval command is an external whose front-matter sets `eval: true`; its
/// stdout is meant to be `eval`'d by the shell. Built-ins are never eval.
pub fn collect(args: &[String], ctx: &Context) -> Vec<String> {
    let mode = args.first().map(String::as_str);
    let mut out = BTreeSet::new();

    if mode != Some("--eval") {
        for doc in BUILTINS {
            out.insert(doc.name.to_string());
        }
    }
    for c in ctx.index.leaves() {
        let keep = match mode {
            Some("--eval") => c.front.eval,
            Some("--no-eval") => !c.front.eval,
            _ => true,
        };
        if keep {
            out.insert(c.name.clone());
        }
    }
    out.into_iter().collect()
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    for name in collect(args, ctx) {
        println!("{name}");
    }
    0
}

pub fn complete(args: &[String], _ctx: &Context) -> i32 {
    if args.is_empty() {
        println!("--eval");
        println!("--no-eval");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::{CommandInfo, Index};
    use std::path::PathBuf;

    fn ctx_with(names: &[&str]) -> (Identity, Index) {
        let pairs: Vec<(&str, bool)> = names.iter().map(|n| (*n, false)).collect();
        ctx_with_eval(&pairs)
    }

    fn ctx_with_eval(cmds: &[(&str, bool)]) -> (Identity, Index) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            local_root: None,
            config_path: PathBuf::new(),
        };
        let cmds = cmds
            .iter()
            .map(|(n, eval)| CommandInfo {
                name: n.to_string(),
                path: PathBuf::from(format!("/r/libexec/{}", n.replace(' ', "/"))),
                front: FrontMatter {
                    eval: *eval,
                    ..Default::default()
                },
                is_local: false,
            })
            .collect();
        (id, Index::from_leaves(cmds))
    }

    #[test]
    fn lists_builtins_and_externals_sorted() {
        let (id, cmds) = ctx_with(&["who"]);
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let out = collect(&[], &ctx);
        assert!(out.contains(&"who".to_string()));
        assert!(out.contains(&"help".to_string()));
        let mut sorted = out.clone();
        sorted.sort();
        assert_eq!(out, sorted);
    }

    #[test]
    fn eval_filter_lists_only_eval_commands() {
        let (id, cmds) = ctx_with_eval(&[("cd", true), ("who", false)]);
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let out = collect(&["--eval".to_string()], &ctx);
        assert_eq!(out, vec!["cd".to_string()]);
    }

    #[test]
    fn no_eval_filter_excludes_eval_commands() {
        let (id, cmds) = ctx_with_eval(&[("cd", true), ("who", false)]);
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let out = collect(&["--no-eval".to_string()], &ctx);
        assert!(out.contains(&"who".to_string()));
        assert!(!out.contains(&"cd".to_string()));
    }
}
