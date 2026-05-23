use crate::builtins::{all_command_names, BUILTIN_DOCS};
use crate::builtins::Context;
use std::env;

struct Doc {
    summary: Option<String>,
    usage: Option<String>,
    help: Option<String>,
    is_local: bool,
}

/// Gather documentation for a command from the built-in registry or an external
/// command's front-matter.
fn doc_for(name: &str, ctx: &Context) -> Option<Doc> {
    if let Some(c) = ctx.commands.iter().find(|c| c.name == name) {
        return Some(Doc {
            summary: c.front.summary.clone(),
            usage: c.front.usage.clone(),
            help: c.front.help.clone(),
            is_local: c.is_local,
        });
    }
    if let Some(b) = BUILTIN_DOCS.iter().find(|b| b.name == name) {
        let prog = &ctx.identity.name;
        return Some(Doc {
            summary: Some(b.summary.to_string()),
            usage: Some(b.usage.replace("<name>", prog)),
            help: Some(b.help.to_string()),
            is_local: false,
        });
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() > max && max > 3 {
        let kept: String = s.chars().take(max - 3).collect();
        format!("{kept}...")
    } else {
        s.to_string()
    }
}

/// Render the command table shown by bare `help`.
pub fn render_table(ctx: &Context, columns: usize) -> String {
    let prog = &ctx.identity.name;
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut longest = 0;

    for name in all_command_names(ctx) {
        if let Some(doc) = doc_for(&name, ctx) {
            if let Some(summary) = doc.summary {
                let summary = if doc.is_local { format!("(local) {summary}") } else { summary };
                longest = longest.max(name.len());
                rows.push((name, summary));
            }
        }
    }

    let summary_width = columns.saturating_sub(longest + 5).max(10);
    let mut out = String::new();
    out.push_str(&format!("Usage: {prog} <command> [<args>]\n\n"));
    out.push_str(&format!("Some useful {prog} commands are:\n"));
    for (name, summary) in rows {
        out.push_str(&format!(
            "   {name:<longest$}  {}\n",
            truncate(&summary, summary_width)
        ));
    }
    out.push_str(&format!("\nSee '{prog} help <command>' for more information on a specific command.\n"));
    out
}

/// Render the detailed help for a single command. `None` if unknown.
pub fn render_detail(name: &str, ctx: &Context) -> Option<String> {
    let doc = doc_for(name, ctx)?;
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
    env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).unwrap_or(80)
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        for name in all_command_names(ctx) {
            if doc_for(&name, ctx).and_then(|d| d.usage).is_some() {
                println!("{name}");
            }
        }
        return 0;
    }
    match args.first() {
        None => {
            print!("{}", render_table(ctx, terminal_columns()));
            0
        }
        Some(name) => match render_detail(name, ctx) {
            Some(detail) => {
                print!("{detail}");
                0
            }
            None => {
                eprintln!("{}: no such command `{name}'", ctx.identity.name);
                1
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::CommandInfo;
    use std::path::PathBuf;

    fn ctx() -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity { name: "rush".into(), root: PathBuf::from("/r"), local_root: None };
        let cmds = vec![CommandInfo {
            name: "who".into(),
            path: PathBuf::from("/r/libexec/rush-who"),
            front: FrontMatter {
                summary: Some("Check who's logged in".into()),
                usage: Some("rush who".into()),
                help: Some("Long help here.".into()),
                ..Default::default()
            },
            is_local: false,
        }];
        (id, None, cmds)
    }

    #[test]
    fn table_lists_commands_with_summaries() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let table = render_table(&ctx, 80);
        assert!(table.contains("rush <command>"));
        assert!(table.contains("who"));
        assert!(table.contains("Check who's logged in"));
    }

    #[test]
    fn detail_renders_usage_summary_help() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let detail = render_detail("who", &ctx).unwrap();
        assert!(detail.contains("Usage: rush who"));
        assert!(detail.contains("Summary: Check who's logged in"));
        assert!(detail.contains("Long help here."));
    }

    #[test]
    fn detail_for_builtin_uses_registry() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let detail = render_detail("commands", &ctx).unwrap();
        assert!(detail.contains("List all commands"));
    }

    #[test]
    fn detail_for_unknown_is_none() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        assert!(render_detail("nope", &ctx).is_none());
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
