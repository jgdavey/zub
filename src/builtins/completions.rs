use crate::builtins;
use crate::builtins::Context;
use crate::index::{exec_or_report, Resolution};
use std::env;
use std::path::PathBuf;
use std::process::Command;

/// What `completions <tokens…>` should do, once the settled tokens are resolved.
#[derive(Debug, PartialEq)]
pub enum CompAction<'a> {
    /// Run a built-in's own `--complete` with these args.
    Builtin { name: String, args: Vec<String> },
    /// Exec an external command's `--complete` with these args (the command
    /// opted in with `complete: true`).
    Delegate { path: PathBuf, args: Vec<String> },
    /// Offer these namespace child tokens.
    Children(Vec<Resolution<'a>>),
    /// No completion source — let the shell fall back (exit 42).
    Fallback,
}

/// Decide how to complete, given the settled tokens (everything before the cursor)
// The partial here is for future potential use
pub fn plan<'a>(settled: &[String], _partial: Option<String>, ctx: &'a Context) -> CompAction<'a> {
    match ctx.index.resolve(settled) {
        Resolution::Builtin(builtin) => {
            let args = settled[1..].to_vec();
            CompAction::Builtin {
                name: builtin.name.to_string(),
                args,
            }
        }
        Resolution::Command { command } => {
            let completes = command.front.complete;
            if !completes {
                return CompAction::Fallback;
            }
            let args = settled[command.components.len()..].to_vec();
            CompAction::Delegate {
                path: command.path.clone(),
                args,
            }
        }
        Resolution::Namespace { namespace, .. } => {
            CompAction::Children(namespace.child_resolutions())
        }
        Resolution::NotFound => CompAction::Fallback,
    }
}

fn print_resolution(resolution: Resolution) -> Option<()> {
    let name = resolution.name()?;
    match resolution.summary() {
        Some(s) => println!("{name}[{s}]"),
        None => println!("{name}"),
    }
    Some(())
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
        return crate::exit_codes::USAGE;
    }

    let settled = args;
    // The currently completing word. This can be empty if at a blank spot
    let partial = env::var("COMP_WORD").ok().filter(|s| !s.is_empty());

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
            let mut cmd = Command::new(&path);
            cmd.args(&exec_args);
            exec_or_report(cmd, &ctx.identity.name, "completion")
        }
        CompAction::Children(resolutions) => {
            for resolution in resolutions {
                print_resolution(resolution);
            }
            0
        }
        CompAction::Fallback => crate::exit_codes::COMPLETION_FALLBACK,
    }
}

/// zsh-style `name[summary]` lines for top-level command completion. Lists
/// depth-1 leaves and namespaces alongside built-ins.
fn print_summaries(ctx: &Context) -> i32 {
    for resolution in ctx.index.top_level_resolutions() {
        print_resolution(resolution);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::FrontMatter;
    use crate::identity::fixture;
    use crate::index::{self, Index};
    use std::path::PathBuf;

    fn ctx_cmds(specs: &[(&str, bool)]) -> Index {
        let cmds = specs
            .iter()
            .map(|(name, complete)| {
                index::leaf(
                    name,
                    FrontMatter {
                        complete: *complete,
                        ..Default::default()
                    },
                )
            })
            .collect();
        Index::from_leaves(cmds)
    }

    fn with_ctx<R>(index: &Index, f: impl FnOnce(&Context) -> R) -> R {
        let id = fixture("rush", "/r");
        let ctx = Context {
            identity: &id,
            index,
        };
        f(&ctx)
    }

    #[test]
    fn plan_delegates_to_leaf_command_args() {
        let cmds = ctx_cmds(&[("who", true)]);
        with_ctx(&cmds, |ctx| {
            let action = plan(&["who".to_string()], None, ctx);
            assert_eq!(
                action,
                CompAction::Delegate {
                    path: PathBuf::from("/libexec/who"),
                    args: vec![],
                }
            );
        });
    }

    #[test]
    fn plan_offers_namespace_children() {
        let cmds = ctx_cmds(&[("db migrate", false), ("db seed", false)]);
        with_ctx(&cmds, |ctx| {
            let action = plan(&["db".to_string()], None, ctx);
            let resolutions = vec![ctx.index.get("db migrate"), ctx.index.get("db seed")];
            assert_eq!(action, CompAction::Children(resolutions));
        });
    }

    #[test]
    fn plan_delegates_nested_leaf_with_remaining_args() {
        let cmds = ctx_cmds(&[("db migrate", true)]);
        with_ctx(&cmds, |ctx| {
            let action = plan(
                &["db".to_string(), "migrate".to_string(), "--f".to_string()],
                None,
                ctx,
            );
            assert_eq!(
                action,
                CompAction::Delegate {
                    path: PathBuf::from("/libexec/db/migrate"),
                    args: vec!["--f".to_string()],
                }
            );
        });
    }

    #[test]
    fn plan_builtin_passes_remaining_and_partial() {
        let cmds = ctx_cmds(&[]);
        with_ctx(&cmds, |ctx| {
            let action = plan(&["commands".to_string(), "--e".to_string()], None, ctx);
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
            assert_eq!(plan(&["who".to_string()], None, ctx), CompAction::Fallback);
        });
    }

    #[test]
    fn plan_unknown_falls_back() {
        let cmds = ctx_cmds(&[]);
        with_ctx(&cmds, |ctx| {
            assert_eq!(
                plan(&["bogus".to_string()], None, ctx),
                CompAction::Fallback
            );
        });
    }
}
