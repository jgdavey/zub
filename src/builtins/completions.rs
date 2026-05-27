use crate::builtins;
use crate::builtins::Context;
use crate::dispatch::{self, Resolution};
use std::env;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// What `completions <tokens…>` should do, once the settled tokens are resolved.
#[derive(Debug, PartialEq)]
pub enum CompAction {
    /// Run a built-in's own `--complete` with these args.
    Builtin { name: String, args: Vec<String> },
    /// Exec an external command's `--complete` with these args (the command
    /// opted in with `complete: true`).
    Delegate { path: PathBuf, args: Vec<String> },
    /// Offer these namespace child tokens.
    Children(Vec<String>),
    /// No completion source — let the shell fall back (exit 42).
    Fallback,
}

/// Decide how to complete, given the settled tokens (everything before the
/// word being typed) and the `partial` word itself. The resulting `args` are
/// what to pass after `--complete`: the command's own remaining args followed
/// by the partial word.
pub fn plan(settled: &[String], partial: &str, ctx: &Context) -> CompAction {
    match dispatch::resolve(settled, ctx.index) {
        Resolution::Builtin(name) => {
            let mut args = settled[1..].to_vec();
            args.push(partial.to_string());
            CompAction::Builtin { name, args }
        }
        Resolution::External { command, consumed, .. } => {
            let completes = command.front.complete;
            if !completes {
                return CompAction::Fallback;
            }
            let mut args = settled[consumed..].to_vec();
            args.push(partial.to_string());
            CompAction::Delegate { path: command.path.clone(), args }
        }
        Resolution::Namespace { subcommands, .. } => CompAction::Children(subcommands),
        Resolution::NotFound => CompAction::Fallback,
    }
}

pub fn complete(_args: &[String], _ctx: &Context) -> i32 {
    0
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--commands") {
        return print_summaries(ctx);
    }

    if args.is_empty() {
        eprintln!(
            "usage: {} completions command [arg1 arg2...]",
            ctx.identity.name
        );
        return 1;
    }

    // The shell always includes the word being completed as the final arg
    // (empty when starting fresh). Everything before it is settled.
    let (partial, settled) = args.split_last().unwrap();

    match plan(settled, partial, ctx) {
        CompAction::Builtin { name, args } => builtins::complete(&name, &args, ctx),
        CompAction::Delegate { path, args } => {
            // Commands read the word being completed from COMP_LASTARG, and the
            // token before it from COMP_PENULT.
            let last = args.last().cloned().unwrap_or_default();
            let penult = if args.len() >= 2 {
                args[args.len() - 2].clone()
            } else {
                String::new()
            };
            env::set_var("COMP_LASTARG", &last);
            env::set_var("COMP_PENULT", &penult);
            let mut exec_args = vec!["--complete".to_string()];
            exec_args.extend(args);
            let err = Command::new(&path).args(&exec_args).exec();
            eprintln!("{}: failed to exec completion: {err}", ctx.identity.name);
            1
        }
        CompAction::Children(children) => {
            for child in children {
                println!("{child}");
            }
            0
        }
        CompAction::Fallback => 42,
    }
}

/// zsh-style `name[summary]` lines for top-level command completion. Lists
/// depth-1 leaves and namespaces alongside built-ins.
fn print_summaries(ctx: &Context) -> i32 {
    for name in builtins::top_level_names(ctx) {
        match builtins::entry_summary(&name, ctx) {
            Some(s) => println!("{name}[{s}]"),
            None => println!("{name}"),
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::{CommandInfo, Index};
    use std::path::PathBuf;

    fn ctx_cmds(specs: &[(&str, bool)]) -> Index {
        let cmds = specs
            .iter()
            .map(|(name, complete)| CommandInfo {
                name: name.to_string(),
                path: PathBuf::from(format!("/lx/{}", name.replace(' ', "/"))),
                front: FrontMatter {
                    complete: *complete,
                    ..Default::default()
                },
                is_local: false,
            })
            .collect();
        Index::from_leaves(cmds)
    }

    fn with_ctx<R>(index: &Index, f: impl FnOnce(&Context) -> R) -> R {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            local_root: None,
            config_path: PathBuf::new(),
        };
        let cfg: Option<Config> = None;
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index,
        };
        f(&ctx)
    }

    #[test]
    fn plan_delegates_to_leaf_command_args() {
        let cmds = ctx_cmds(&[("who", true)]);
        with_ctx(&cmds, |ctx| {
            let action = plan(&["who".to_string()], "", ctx);
            assert_eq!(
                action,
                CompAction::Delegate {
                    path: PathBuf::from("/lx/who"),
                    args: vec!["".to_string()],
                }
            );
        });
    }

    #[test]
    fn plan_offers_namespace_children() {
        let cmds = ctx_cmds(&[("db migrate", false), ("db seed", false)]);
        with_ctx(&cmds, |ctx| {
            let action = plan(&["db".to_string()], "", ctx);
            assert_eq!(
                action,
                CompAction::Children(vec!["migrate".to_string(), "seed".to_string()])
            );
        });
    }

    #[test]
    fn plan_delegates_nested_leaf_with_remaining_args() {
        let cmds = ctx_cmds(&[("db migrate", true)]);
        with_ctx(&cmds, |ctx| {
            let action = plan(&["db".to_string(), "migrate".to_string()], "--f", ctx);
            assert_eq!(
                action,
                CompAction::Delegate {
                    path: PathBuf::from("/lx/db/migrate"),
                    args: vec!["--f".to_string()],
                }
            );
        });
    }

    #[test]
    fn plan_builtin_passes_remaining_and_partial() {
        let cmds = ctx_cmds(&[]);
        with_ctx(&cmds, |ctx| {
            let action = plan(&["commands".to_string()], "--e", ctx);
            assert_eq!(
                action,
                CompAction::Builtin {
                    name: "commands".to_string(),
                    args: vec!["--e".to_string()],
                }
            );
        });
    }

    #[test]
    fn plan_non_completing_leaf_falls_back() {
        let cmds = ctx_cmds(&[("who", false)]);
        with_ctx(&cmds, |ctx| {
            assert_eq!(plan(&["who".to_string()], "", ctx), CompAction::Fallback);
        });
    }

    #[test]
    fn plan_unknown_falls_back() {
        let cmds = ctx_cmds(&[]);
        with_ctx(&cmds, |ctx| {
            assert_eq!(plan(&["bogus".to_string()], "", ctx), CompAction::Fallback);
        });
    }
}
