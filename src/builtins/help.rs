use crate::builtins::Context;
use crate::builtins::{entry_summary, top_level_names};
use crate::dispatch::{self, Resolution};
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

/// Build `(name, summary)` rows for the given entry names, marking local leaves.
fn rows_for(entries: &[String], ctx: &Context) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for name in entries {
        if let Some(summary) = entry_summary(name, ctx) {
            let summary = match ctx.index.get(name) {
                Some(c) if c.is_local => format!("(local) {summary}"),
                _ => summary,
            };
            rows.push((name.clone(), summary));
        }
    }
    rows
}

/// Render a command table with the given usage `header` command (e.g.
/// `"<command>"` or `"db <command>"`) and rows.
fn render_rows(ctx: &Context, header: &str, rows: Vec<(String, String)>, columns: usize) -> String {
    let prog = &ctx.identity.name;
    let longest = rows.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let summary_width = columns.saturating_sub(longest + 5).max(10);
    let mut out = String::new();
    out.push_str(&format!("Usage: {prog} {header} [<args>]\n\n"));
    out.push_str(&format!("Some useful {prog} commands are:\n"));
    for (name, summary) in rows {
        out.push_str(&format!(
            "   {name:<longest$}  {}\n",
            truncate(&summary, summary_width)
        ));
    }
    out.push_str(&format!(
        "\nSee '{prog} help <command>' for more information on a specific command.\n"
    ));
    out
}

/// Render the top-level command table shown by bare `help`.
pub fn render_table(ctx: &Context, columns: usize) -> String {
    let rows = rows_for(&top_level_names(ctx), ctx);
    render_rows(ctx, "<command>", rows, columns)
}

/// Render the child table for a namespace prefix (e.g. `help db`).
pub fn render_namespace_table(prefix: &str, ctx: &Context, columns: usize) -> String {
    let children: Vec<String> = ctx
        .index
        .children(prefix)
        .into_iter()
        .map(|c| format!("{prefix} {c}"))
        .collect();
    let rows = rows_for(&children, ctx);
    render_rows(ctx, &format!("{prefix} <command>"), rows, columns)
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

fn terminal_columns() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(80)
}

pub fn complete(args: &[String], ctx: &Context) -> i32 {
    match dispatch::resolve(args, ctx.index) {
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
        Resolution::Namespace { subcommands, .. } => {
            for s in subcommands {
                print!("{s}");
            }
            0
        }
        Resolution::Builtin { .. } | Resolution::External { .. } => 0,
    }
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.is_empty() {
        print!("{}", render_table(ctx, terminal_columns()));
        return 0;
    }

    let name = args.join(" ");

    let doc = match dispatch::resolve(args, ctx.index) {
        Resolution::NotFound => {
            eprintln!("{}: no such command `{name}'", &ctx.identity.name);
            return 1;
        }
        Resolution::Namespace { .. } => {
            print!("{}", render_namespace_table(&name, ctx, terminal_columns()));
            return 0;
        }
        Resolution::Builtin { builtin, .. } => Doc {
            summary: Some(builtin.summary.to_string()),
            usage: Some(builtin.usage.replace("<name>", &ctx.identity.name)),
            help: Some(builtin.help.to_string()),
        },
        Resolution::External { command, .. } => Doc {
            summary: command.front.summary.clone(),
            usage: command.front.usage.clone(),
            help: command.front.help.clone(),
        },
    };

    if let Some(detail) = render_detail(doc) {
        print!("{detail}");
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::{CommandInfo, Index};
    use std::path::PathBuf;

    fn ctx() -> (Identity, Option<Config>, Index) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            local_root: None,
            config_path: PathBuf::new(),
        };
        let cmds = vec![CommandInfo {
            name: "who".into(),
            path: PathBuf::from("/r/libexec/who"),
            front: FrontMatter {
                summary: Some("Check who's logged in".into()),
                usage: Some("rush who".into()),
                help: Some("Long help here.".into()),
                ..Default::default()
            },
            is_local: false,
        }];
        (id, None, Index::from_leaves(cmds))
    }

    fn ctx_named(specs: &[(&str, Option<&str>)]) -> (Identity, Option<Config>, Index) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/r"),
            local_root: None,
            config_path: PathBuf::new(),
        };
        let cmds = specs
            .iter()
            .map(|(name, summary)| CommandInfo {
                name: name.to_string(),
                path: PathBuf::from(format!("/r/libexec/{}", name.replace(' ', "/"))),
                front: FrontMatter {
                    summary: summary.map(String::from),
                    usage: Some(format!("rush {name}")),
                    ..Default::default()
                },
                is_local: false,
            })
            .collect();
        (id, None, Index::from_leaves(cmds))
    }

    #[test]
    fn table_lists_commands_with_summaries() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let table = render_table(&ctx, 80);
        assert!(table.contains("rush <command>"));
        assert!(table.contains("who"));
        assert!(table.contains("Check who's logged in"));
    }

    #[test]
    fn table_lists_namespace_with_synthetic_summary() {
        let (id, cfg, cmds) = ctx_named(&[("db migrate", Some("m")), ("db seed", Some("s"))]);
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let table = render_table(&ctx, 80);
        assert!(table.contains("db"));
        assert!(table.contains("2 subcommands"));
        // The nested leaves are not listed at the top level.
        assert!(!table.contains("migrate"));
    }

    #[test]
    fn namespace_table_lists_children_full_names() {
        let (id, cfg, cmds) = ctx_named(&[
            ("db migrate", Some("run migrations")),
            ("db seed", Some("seed it")),
        ]);
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let table = render_namespace_table("db", &ctx, 80);
        assert!(table.contains("db migrate"));
        assert!(table.contains("run migrations"));
        assert!(table.contains("db seed"));
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
