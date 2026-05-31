use crate::builtins::Context;
use crate::index::{Command, Namespace, Resolution};
use std::env;
use std::io::Write;
use std::process::Command as ProcessCommand;

struct Doc {
    summary: Option<String>,
    usage: Option<String>,
    help: Option<String>,
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max && max > 3 {
        let kept: String = s.chars().take(max - 3).collect();
        format!("{kept}...")
    } else {
        s.to_string()
    }
}

/// Build `(name, summary)` rows for the given resolutions, marking local leaves.
/// Undocumented entries (no summary) are skipped.
fn rows_for(entries: &[Resolution]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|res| {
            let summary = res.summary()?;
            let name = res.name()?;
            let summary = match res {
                Resolution::Command { command } if command.is_local => format!("(local) {summary}"),
                _ => summary,
            };
            Some((name, summary))
        })
        .collect()
}

/// Render a command table with the given usage `header` command (e.g.
/// `"<command>"` or `"db <command>"`) and rows.
fn render_rows(
    ctx: &Context,
    prefix: Option<&str>,
    rows: Vec<(String, String)>,
    columns: usize,
) -> String {
    let prog = &ctx.identity.name;
    let header = if let Some(pre) = prefix {
        let mut s = pre.to_string();
        s.push(' ');
        s
    } else {
        "".to_string()
    };
    let longest = rows.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let summary_width = columns.saturating_sub(longest + 5).max(10);
    let mut out = String::new();
    out.push_str(&format!("Usage: {prog} {header}<command> [<args>]\n\n",));
    out.push_str("Commands:\n");
    for (name, summary) in rows {
        out.push_str(&format!(
            "   {name:<longest$}  {}\n",
            truncate(&summary, summary_width)
        ));
    }
    out.push_str(&format!(
        "\nSee '{prog} help {header}<command>' for more information on a specific command.\n"
    ));
    out
}

/// Render the top-level command table shown by bare `help`.
pub fn render_table(ctx: &Context, columns: usize) -> String {
    let resolutions = ctx.index.top_level_resolutions();
    render_rows(ctx, None, rows_for(&resolutions), columns)
}

/// Render the child table for a namespace (e.g. `help db`).
pub fn render_namespace_table(namespace: &Namespace, ctx: &Context, columns: usize) -> String {
    render_rows(
        ctx,
        Some(&namespace.components.join(" ")),
        rows_for(&namespace.child_resolutions()),
        columns,
    )
}

/// Render the detailed help for a single command. `None` if unknown.
fn render_detail(doc: Doc) -> Option<String> {
    let usage = doc.usage?; // documented commands have a usage line
    let mut out = String::new();
    out.push_str(&format!("Usage: {usage}\n"));
    if let Some(summary) = doc.summary {
        out.push_str(&format!("Summary: {summary}\n"));
    }
    if let Some(help) = doc.help {
        if !help.trim().is_empty() {
            out.push('\n');
            out.push_str(help.trim_end());
            out.push('\n');
        }
    }
    Some(out)
}

/// The stdout terminal's column count, or `None` when stdout is not a terminal.
fn tty_columns() -> Option<usize> {
    terminal_size::terminal_size().map(|(width, _)| width.0 as usize)
}

/// The terminal width in columns. `COLUMNS` is honored as an explicit
/// override when set, but shells keep it as a *non-exported* shell
/// variable, so it is usually absent from a child process's
/// environment.
fn terminal_columns() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .or_else(tty_columns)
        .unwrap_or(80)
}

pub fn complete(args: &[String], ctx: &Context) -> i32 {
    // `help` completes built-ins too, since it documents them.
    super::complete_command_names(args, ctx, true)
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    // Ensure a bit of padding on the right
    let columns = terminal_columns().max(20) - 2;
    if args.is_empty() {
        print!("{}", render_table(ctx, columns));
        return 0;
    }

    match ctx.index.resolve(args) {
        Resolution::NotFound => {
            eprintln!(
                "{}: no such command `{}'",
                ctx.identity.name,
                args.join(" ")
            );
            1
        }
        Resolution::Namespace { namespace } => {
            print!("{}", render_namespace_table(namespace, ctx, columns));
            0
        }
        res => {
            let doc = Doc {
                summary: res.summary(),
                usage: res.usage(ctx.identity),
                help: res.help(ctx.identity),
            };
            // A `dynamic_help` command appends its own `--help` output, so it is
            // shown even when it has no static front-matter to render.
            let dynamic = match &res {
                Resolution::Command { command } if command.front.dynamic_help => Some(*command),
                _ => None,
            };
            let printed = match render_detail(doc) {
                Some(detail) => {
                    print!("{detail}");
                    true
                }
                None if dynamic.is_some() => false,
                None => return 1,
            };
            match dynamic {
                Some(command) => run_dynamic_help(command, printed),
                None => 0,
            }
        }
    }
}

/// Run a `dynamic_help` command with `--help` so it can emit the rest of its
/// help below the static front-matter text. `printed` says whether any static
/// detail was already written, so a blank separator can be inserted. Returns
/// the child's exit code.
fn run_dynamic_help(command: &Command, printed: bool) -> i32 {
    if printed {
        println!();
    }
    // Flush so the static text precedes the child's output on the shared stdout.
    let _ = std::io::stdout().flush();
    match ProcessCommand::new(&command.path).arg("--help").status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("failed to run {} --help: {err}", command.path.display());
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::FrontMatter;
    use crate::identity::{CommandRoot, Identity};
    use crate::index::{self, Index};
    use std::path::PathBuf;

    fn ctx() -> (Identity, Index) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            command_roots: vec![CommandRoot {
                path: PathBuf::from("/r/libexec"),
                is_local: false,
            }],
            config_path: PathBuf::new(),
        };
        let cmds = vec![index::leaf(
            "who",
            FrontMatter {
                summary: Some("Check who's logged in".into()),
                usage: Some("rush who".into()),
                help: Some("Long help here.".into()),
                ..Default::default()
            },
        )];
        (id, Index::from_leaves(cmds))
    }

    fn ctx_named(specs: &[(&str, Option<&str>)]) -> (Identity, Index) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            command_roots: vec![CommandRoot {
                path: PathBuf::from("/r/libexec"),
                is_local: false,
            }],
            config_path: PathBuf::new(),
        };
        let cmds = specs
            .iter()
            .map(|(name, summary)| {
                index::leaf(
                    name,
                    FrontMatter {
                        summary: summary.map(String::from),
                        usage: Some(format!("rush {name}")),
                        ..Default::default()
                    },
                )
            })
            .collect();
        (id, Index::from_leaves(cmds))
    }

    #[test]
    fn table_lists_commands_with_summaries() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let table = render_table(&ctx, 80);
        assert!(table.contains("rush <command>"));
        assert!(table.contains("who"));
        assert!(table.contains("Check who's logged in"));
    }

    #[test]
    fn table_lists_namespace_with_synthetic_summary() {
        let (id, cmds) = ctx_named(&[("db migrate", Some("m")), ("db seed", Some("s"))]);
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let table = render_table(&ctx, 80);
        assert!(table.contains("db"));
        // The namespace's synthetic summary counts and lists its children.
        assert!(table.contains("2 subcommands (migrate, seed)"));
        // The nested leaves are not promoted to their own top-level rows.
        assert!(!table.lines().any(|l| l.trim_start().starts_with("migrate")));
    }

    #[test]
    fn namespace_table_lists_children_full_names() {
        let (id, cmds) = ctx_named(&[
            ("db migrate", Some("run migrations")),
            ("db seed", Some("seed it")),
        ]);
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let Resolution::Namespace { namespace } = ctx.index.resolve(&["db".to_string()]) else {
            panic!("expected db to resolve to a namespace");
        };
        let table = render_namespace_table(namespace, &ctx, 80);
        assert!(table.contains("migrate"));
        assert!(table.contains("run migrations"));
        assert!(table.contains("seed"));
    }

    #[test]
    fn truncate_handles_multibyte_without_panic() {
        // 10 multibyte chars (é = 2 bytes each); truncating must not panic
        let s = "éééééééééé";
        let out = truncate(s, 8);
        assert!(out.ends_with("..."));
        assert!(out.len() <= s.len());
    }

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hi", 80), "hi");
    }
}
