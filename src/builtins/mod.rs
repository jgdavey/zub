use crate::config::Config;
use crate::identity::Identity;
use crate::index::Index;

pub mod commands;
pub mod completions;
pub mod help;
pub mod init;
pub mod new;
pub mod source;

/// Documentation for a built-in command, used by `help` and `commands`.
pub struct BuiltinDoc {
    pub name: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub help: &'static str,
}

pub const BUILTIN_DOCS: &[BuiltinDoc] = &[
    BuiltinDoc {
        name: "commands",
        usage: "<name> commands",
        summary: "List all commands",
        help: "Mostly used for completion and `help`.",
    },
    BuiltinDoc {
        name: "completions",
        usage: "<name> completions <command> [args...]",
        summary: "Drive subcommand completion",
        help: "Called by the shell completion scripts.",
    },
    BuiltinDoc {
        name: "help",
        usage: "<name> help [<command>]",
        summary: "Show help for a command",
        help: "Run `<name> help <command>` for details.",
    },
    BuiltinDoc {
        name: "init",
        usage: "<name> init [-]",
        summary: "Print shell integration",
        help: "Add `eval \"$(<name> init -)\"` to your shell profile.",
    },
    BuiltinDoc {
        name: "new",
        usage: "<name> new [--local] [--eval] <command>",
        summary: "Generate a new command",
        help: "Creates a libexec script with front-matter.",
    },
    BuiltinDoc {
        name: "source",
        usage: "<name> source <command>",
        summary: "Print a command's source",
        help: "Pages the file with bat/$PAGER/cat.",
    },
];

/// Shared context handed to every built-in.
pub struct Context<'a> {
    pub identity: &'a Identity,
    pub config: &'a Option<Config>,
    pub index: &'a Index,
}

pub fn run(name: &str, args: &[String], ctx: &Context) -> i32 {
    match name {
        "commands" => commands::run(args, ctx),
        "completions" => completions::run(args, ctx),
        "help" => help::run(args, ctx),
        "init" => init::run(args, ctx),
        "new" => new::run(args, ctx),
        "source" => source::run(args, ctx),
        _ => {
            eprintln!(
                "{}: built-in `{name}' not implemented yet",
                ctx.identity.name
            );
            1
        }
    }
}

/// Summary for an entry by name: a leaf command's front-matter summary, a
/// built-in's registered summary, or a synthetic `"<n> subcommands"` count for
/// a namespace. `None` when the name is an undocumented leaf.
pub fn entry_summary(name: &str, ctx: &Context) -> Option<String> {
    if let Some(c) = ctx.index.get(name) {
        return c.front.summary.clone();
    }
    if let Some(b) = BUILTIN_DOCS.iter().find(|b| b.name == name) {
        return Some(b.summary.to_string());
    }
    let children = ctx.index.children(name);
    if children.is_empty() {
        None
    } else {
        Some(format!("{} subcommands", children.len()))
    }
}

/// Top-level entries: built-ins plus the distinct first components of external
/// command names (each a depth-1 leaf or a namespace). Deduped, sorted.
pub fn top_level_names(ctx: &Context) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for doc in BUILTIN_DOCS {
        set.insert(doc.name.to_string());
    }
    for entry in ctx.index.top_level() {
        set.insert(entry);
    }
    set.into_iter().collect()
}

#[cfg(test)]
mod consistency_tests {
    use super::BUILTIN_DOCS;
    use crate::dispatch::BUILTINS;
    use std::collections::BTreeSet;

    #[test]
    fn builtins_and_docs_cover_the_same_names() {
        let dispatched: BTreeSet<&str> = BUILTINS.iter().copied().collect();
        let documented: BTreeSet<&str> = BUILTIN_DOCS.iter().map(|d| d.name).collect();
        assert_eq!(
            dispatched, documented,
            "BUILTINS (dispatch.rs) and BUILTIN_DOCS (builtins/mod.rs) must list the same command names"
        );
    }
}
