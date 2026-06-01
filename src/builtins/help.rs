use crate::builtins::Context;
use crate::index::{exec_or_report, Command, Namespace, Resolution};
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
/// An undocumented command (no summary) still gets a row — discoverability
/// matters more than tidiness — with an empty summary; the `(local)` tag stands
/// in on its own when such a command is also local.
fn rows_for(entries: &[Resolution]) -> Vec<(String, String)> {
    entries
        .iter()
        .filter_map(|res| {
            let name = res.name()?;
            let summary = res.summary().unwrap_or_else(|| "(no summary)".to_string());
            let summary = match res {
                Resolution::Command { command } if command.is_local => {
                    format!("(local) {summary}").trim_end().to_string()
                }
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
        let row = format!("   {name:<longest$}  {}", truncate(&summary, summary_width));
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out.push_str(&format!(
        "\nSee '{prog} help {header}<command>' for more information on a specific command.\n"
    ));
    out
}

/// The program identification line shown above the top-level table, drawn from
/// the config's `version`/`description`: `<name> <version> — <description>`,
/// with either part omitted when unset. `None` when both are unset (the table
/// then opens straight at `Usage:`, as before).
fn program_header(ctx: &Context) -> Option<String> {
    let id = ctx.identity;
    match (&id.version, &id.description) {
        (None, None) => None,
        (version, description) => {
            let mut line = id.name.clone();
            if let Some(version) = version {
                line.push(' ');
                line.push_str(version);
            }
            if let Some(description) = description {
                line.push_str(" - ");
                line.push_str(description);
            }
            Some(line)
        }
    }
}

/// Render the top-level command table shown by bare `help`, preceded by the
/// program's version/description header when the config supplies either.
pub fn render_table(ctx: &Context, columns: usize) -> String {
    let resolutions = ctx.index.top_level_resolutions();
    let table = render_rows(ctx, None, rows_for(&resolutions), columns);
    match program_header(ctx) {
        Some(header) => format!("{header}\n\n{table}"),
        None => table,
    }
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
        Resolution::NotFound => super::report_not_found(ctx, args),
        Resolution::Namespace { namespace } if args.len() > namespace.components.len() => {
            // `help db migrt` — a mistyped subcommand, not a request for the
            // `db` table.
            super::report_not_found(ctx, args)
        }
        Resolution::Namespace { namespace } => {
            print!("{}", render_namespace_table(namespace, ctx, columns));
            0
        }
        res => {
            // A `dynamic_help` command appends its own `--help` output, so it is
            // shown even when it has no static front-matter to render.
            let dynamic = match &res {
                Resolution::Command { command } if command.front.dynamic_help => Some(*command),
                _ => None,
            };
            // An undocumented command still gets help: synthesize a usage line
            // from its name so `help <cmd>` describes it instead of failing
            // silently. A dynamic-help command with no static text is the one
            // case left to its own `--help`, so it keeps no synthetic usage.
            let usage = res.usage(ctx.identity).or_else(|| {
                (dynamic.is_none())
                    .then(|| format!("{} {} [<args>]", ctx.identity.name, res.full_name()))
            });
            let doc = Doc {
                summary: res.summary(),
                usage,
                help: res.help(ctx.identity),
            };
            let printed = match render_detail(doc) {
                Some(detail) => {
                    print!("{detail}");
                    true
                }
                None if dynamic.is_some() => false,
                None => return crate::exit_codes::FAILURE,
            };
            match dynamic {
                Some(command) => run_dynamic_help(command, &ctx.identity.name, printed),
                None => 0,
            }
        }
    }
}

/// Run a `dynamic_help` command with `--help` so it can emit the rest of its
/// help below the static front-matter text. `printed` says whether any static
/// detail was already written, so a blank separator can be inserted. The static
/// text is flushed first, then the current process is replaced with the command
/// (its `--help` output and exit code become the process's) — `help` is always
/// the final action, so exec rather than spawn-and-wait is fine.
fn run_dynamic_help(command: &Command, program: &str, printed: bool) -> i32 {
    if printed {
        println!();
    }
    // Flush so the static text precedes the child's output on the shared stdout.
    let _ = std::io::stdout().flush();
    let mut cmd = ProcessCommand::new(&command.path);
    cmd.arg("--help");
    exec_or_report(cmd, program, &format!("{} --help", command.path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::FrontMatter;
    use crate::identity::{fixture, Identity};
    use crate::index::{self, Index};

    fn ctx() -> (Identity, Index) {
        let id = fixture("rush", "/r");
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
        let id = fixture("rush", "/r");
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
    fn table_includes_undocumented_command() {
        // A command with no summary is still listed (just without text), so a
        // half-finished command stays discoverable.
        let (id, cmds) = ctx_named(&[("who", Some("docs")), ("draft", None)]);
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let table = render_table(&ctx, 80);
        assert!(table.lines().any(|l| l.contains("draft")));
    }

    #[test]
    fn header_combines_version_and_description() {
        let mut id = fixture("rush", "/r");
        id.version = Some("1.2.3".into());
        id.description = Some("manage the fleet".into());
        let index = Index::default();
        let ctx = Context {
            identity: &id,
            index: &index,
        };
        assert_eq!(
            program_header(&ctx).as_deref(),
            Some("rush 1.2.3 - manage the fleet")
        );
        // The header opens the top-level table, above `Usage:`.
        assert!(render_table(&ctx, 80).starts_with("rush 1.2.3 - manage the fleet\n\n"));
    }

    #[test]
    fn header_omits_unset_parts() {
        let mut id = fixture("rush", "/r");
        id.description = Some("just a description".into());
        let index = Index::default();
        let ctx = Context {
            identity: &id,
            index: &index,
        };
        assert_eq!(
            program_header(&ctx).as_deref(),
            Some("rush - just a description")
        );
    }

    #[test]
    fn header_absent_when_config_supplies_neither() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        assert_eq!(program_header(&ctx), None);
        // The table opens straight at `Usage:`, as before.
        assert!(render_table(&ctx, 80).starts_with("Usage:"));
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
