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
    /// Delegate to the `usage` binary for a usage-style command: run
    /// `usage complete-word` against the script, with the reconstructed command
    /// line `words` and the index `cword` of the word being completed.
    UsageComplete {
        path: PathBuf,
        words: Vec<String>,
        cword: usize,
    },
    /// Offer these namespace child tokens.
    Children(Vec<Resolution<'a>>),
    /// No completion source — let the shell fall back (exit 42).
    Fallback,
}

/// Decide how to complete, given the settled tokens (everything before the
/// cursor) and `partial`, the word currently being completed (if any).
pub fn plan<'a>(settled: &[String], partial: Option<String>, ctx: &'a Context) -> CompAction<'a> {
    match ctx.index.resolve(settled) {
        Resolution::Builtin(builtin) => {
            let args = settled[1..].to_vec();
            CompAction::Builtin {
                name: builtin.name.to_string(),
                args,
            }
        }
        Resolution::Command { command } if command.meta.is_usage() => {
            let after = &settled[command.components.len()..];
            let (words, cword) = usage_words(&command.name(), after, partial.unwrap_or_default());
            CompAction::UsageComplete {
                path: command.path.clone(),
                words,
                cword,
            }
        }
        Resolution::Command { command } => {
            if !command.meta.wants_completion() {
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

/// Reconstruct the command-line `words` and current-word index `cword` that
/// `usage complete-word` expects, from the command `name`, the args `after` it,
/// and the `partial` word being completed.
///
/// Word 0 is the command name (a placeholder for usage's `bin`); the trailing
/// word is the one being completed. The two shell completers differ in whether
/// `after` already includes the partial — zsh excludes it, bash includes it as
/// the last element — so the partial is appended only when not already present.
/// Returns `(words, cword)`.
fn usage_words(name: &str, after: &[String], partial: String) -> (Vec<String>, usize) {
    let mut words = vec![name.to_string()];
    words.extend(after.iter().cloned());
    if words.last() != Some(&partial) {
        words.push(partial);
    }
    let cword = words.len() - 1;
    (words, cword)
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
        CompAction::UsageComplete { path, words, cword } => usage_complete(&path, &words, cword),
        CompAction::Children(resolutions) => {
            for resolution in resolutions {
                print_resolution(resolution);
            }
            0
        }
        CompAction::Fallback => crate::exit_codes::COMPLETION_FALLBACK,
    }
}

/// Delegate a usage command's completion to the `usage` binary, translating its
/// output into zub's `name[summary]` line format. We always ask `usage` for zsh
/// output (the richest — tab-separated `value\tdescription\t…`) and reformat, so
/// the result works for both zub's zsh and bash completers regardless of the
/// caller's shell. Returns the [`COMPLETION_FALLBACK`] code when `usage` cannot
/// be run (e.g. not installed), so the shell falls back to its default; a
/// successful run with no candidates returns 0 (no completions).
///
/// [`COMPLETION_FALLBACK`]: crate::exit_codes::COMPLETION_FALLBACK
fn usage_complete(path: &std::path::Path, words: &[String], cword: usize) -> i32 {
    let mut cmd = Command::new("usage");
    cmd.arg("complete-word")
        .arg("--shell")
        .arg("zsh")
        .arg("-f")
        .arg(path)
        .arg("--cword")
        .arg(cword.to_string())
        .arg("--")
        .args(words);
    let output = match cmd.output() {
        Ok(output) if output.status.success() => output,
        _ => return crate::exit_codes::COMPLETION_FALLBACK,
    };
    // `usage --shell zsh` emits tab-separated `value\tdescription\tdisplay`
    // lines; we keep the value and description and drop the display column.
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = line.split('\t');
        let value = fields.next().unwrap_or("");
        if value.is_empty() {
            continue;
        }
        match fields.next().map(str::trim).filter(|d| !d.is_empty()) {
            Some(description) => println!("{value}[{description}]"),
            None => println!("{value}"),
        }
    }
    0
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
    use crate::command_meta::FrontMatter;
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

    fn usage_index() -> Index {
        Index::from_leaves(vec![index::leaf_usage("greet", Some("Greet"))])
    }

    /// zsh-style call: the partial word is not in `settled`, so it is appended.
    #[test]
    fn plan_usage_appends_partial_not_in_settled() {
        let index = usage_index();
        with_ctx(&index, |ctx| {
            let action = plan(&["greet".to_string()], Some("--lo".to_string()), ctx);
            assert_eq!(
                action,
                CompAction::UsageComplete {
                    path: PathBuf::from("/libexec/greet"),
                    words: vec!["greet".into(), "--lo".into()],
                    cword: 1,
                }
            );
        });
    }

    /// bash-style call: the partial is already the last settled token; no dup.
    #[test]
    fn plan_usage_keeps_partial_already_present() {
        let index = usage_index();
        with_ctx(&index, |ctx| {
            let action = plan(
                &["greet".to_string(), "--lo".to_string()],
                Some("--lo".to_string()),
                ctx,
            );
            assert_eq!(
                action,
                CompAction::UsageComplete {
                    path: PathBuf::from("/libexec/greet"),
                    words: vec!["greet".into(), "--lo".into()],
                    cword: 1,
                }
            );
        });
    }

    /// No partial (completing a fresh word): an empty trailing word is added.
    #[test]
    fn plan_usage_blank_partial_appends_empty_word() {
        let index = usage_index();
        with_ctx(&index, |ctx| {
            let action = plan(&["greet".to_string()], None, ctx);
            assert_eq!(
                action,
                CompAction::UsageComplete {
                    path: PathBuf::from("/libexec/greet"),
                    words: vec!["greet".into(), "".into()],
                    cword: 1,
                }
            );
        });
    }
}
