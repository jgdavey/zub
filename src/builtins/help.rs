use crate::builtins::top_level_names;
use crate::builtins::Context;
use crate::index::{Namespace, Resolution};
use std::env;

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
fn render_rows(ctx: &Context, prefix: Option<&str>, rows: Vec<(String, String)>, columns: usize) -> String {
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
    out.push_str(&format!("Usage: {prog} {header}<command> [<args>]\n\n", ));
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
    let names = top_level_names(ctx);
    let entries: Vec<Resolution> = names
        .iter()
        .map(|n| ctx.index.resolve(std::slice::from_ref(n)))
        .collect();
    render_rows(ctx, None, rows_for(&entries), columns)
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

/// The terminal width in columns. `COLUMNS` is honored as an explicit override
/// when set — but shells keep it as a *non-exported* shell variable, so it is
/// usually absent from a child process's environment. The reliable source is
/// the controlling tty, queried via `TIOCGWINSZ`; 80 is the last-resort default
/// (e.g. when stdout is a pipe rather than a terminal).
fn terminal_columns() -> usize {
    columns_from(env::var("COLUMNS").ok().as_deref(), tty_columns())
}

/// Pick a width: an explicit, parseable `COLUMNS` override wins; else the
/// tty-reported width; else 80.
fn columns_from(env_cols: Option<&str>, tty_cols: Option<usize>) -> usize {
    env_cols
        .and_then(|c| c.parse().ok())
        .or(tty_cols)
        .unwrap_or(80)
}

/// The stdout terminal's column count, or `None` when stdout is not a terminal.
fn tty_columns() -> Option<usize> {
    terminal_size::terminal_size().map(|(width, _)| width.0 as usize)
}

pub fn complete(args: &[String], ctx: &Context) -> i32 {
    match ctx.index.resolve(args) {
        Resolution::NotFound => {
            if args.is_empty() {
                for name in top_level_names(ctx) {
                    println!("{name}");
                }
                0
            } else {
                1
            }
        }
        Resolution::Namespace { namespace, .. } => {
            for s in namespace.subcommands() {
                print!("{s}");
            }
            0
        }
        Resolution::Builtin(_) | Resolution::Command { .. } => 0,
    }
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.is_empty() {
        print!("{}", render_table(ctx, terminal_columns()));
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
            print!(
                "{}",
                render_namespace_table(namespace, ctx, terminal_columns())
            );
            0
        }
        // Built-in usage carries a `<name>` placeholder; command usage never
        // does, so the replacement is harmless for both.
        res => {
            let doc = Doc {
                summary: res.summary(),
                usage: res.usage().map(|u| u.replace("<name>", &ctx.identity.name)),
                help: res.help(),
            };
            match render_detail(doc) {
                Some(detail) => {
                    print!("{detail}");
                    0
                }
                None => 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::{self, Index};
    use std::path::PathBuf;

    fn ctx() -> (Identity, Index) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            local_root: None,
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
            local_root: None,
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

    #[test]
    fn columns_prefers_explicit_override() {
        assert_eq!(columns_from(Some("120"), Some(40)), 120);
    }

    #[test]
    fn columns_falls_back_to_tty_when_override_absent_or_unparseable() {
        assert_eq!(columns_from(None, Some(40)), 40);
        assert_eq!(columns_from(Some("wide"), Some(40)), 40);
    }

    #[test]
    fn columns_defaults_to_80_without_override_or_tty() {
        assert_eq!(columns_from(None, None), 80);
    }
}
