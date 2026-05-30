use crate::identity::Identity;
use crate::index::Index;

pub mod commands;
pub mod completions;
pub mod help;
pub mod init;
pub mod new;
pub mod source;

/// Documentation and entry point for a built-in command.
#[allow(unpredictable_function_pointer_comparisons)]
#[derive(Debug, PartialEq)]
pub struct Builtin {
    pub name: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub help: &'static str,
    pub run: fn(&[String], &Context) -> i32,
    pub complete: fn(&[String], &Context) -> i32,
}

pub const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "commands",
        usage: "<name> commands [--eval | --no-eval]",
        summary: "List all commands",
        help: "Lists every command, one per line. `--eval`/`--no-eval` restrict \
               to (or exclude) commands whose front-matter sets `eval: true`. \
               Used by `init` to build the eval wrapper.",
        run: commands::run,
        complete: commands::complete,
    },
    Builtin {
        name: "completions",
        usage: "<name> completions <command> [args...]",
        summary: "Drive subcommand completion",
        help: "Called by the shell completion scripts.",
        run: completions::run,
        complete: completions::complete,
    },
    Builtin {
        name: "help",
        usage: "<name> help [<command>]",
        summary: "Show help for a command",
        help: "Run `<name> help <command>` for details.",
        run: help::run,
        complete: help::complete,
    },
    Builtin {
        name: "init",
        usage: "<name> init [-]",
        summary: "Print shell integration",
        help: "Add `eval \"$(<name> init -)\"` to your shell profile.",
        run: init::run,
        complete: init::complete,
    },
    Builtin {
        name: "new",
        usage: "<name> new [--local] [--eval] <command>",
        summary: "Generate a new command",
        help: "Creates a libexec script with front-matter.",
        run: new::run,
        complete: new::complete,
    },
    Builtin {
        name: "source",
        usage: "<name> source <command>",
        summary: "Print a command's source",
        help: "Pages the file with bat/$PAGER/cat.",
        run: source::run,
        complete: source::complete,
    },
];

/// Shared context handed to every built-in.
pub struct Context<'a> {
    pub identity: &'a Identity,
    pub index: &'a Index,
}

pub fn get(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|d| d.name == name)
}

pub fn is_builtin(name: &str) -> bool {
    get(name).is_some()
}

pub fn run(name: &str, args: &[String], ctx: &Context) -> i32 {
    match get(name) {
        Some(doc) => (doc.run)(args, ctx),
        None => {
            eprintln!(
                "{}: built-in `{name}' not implemented yet",
                ctx.identity.name
            );
            1
        }
    }
}

pub fn complete(name: &str, args: &[String], ctx: &Context) -> i32 {
    match get(name) {
        Some(doc) => (doc.complete)(args, ctx),
        None => {
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
    if let Some(b) = BUILTINS.iter().find(|b| b.name == name) {
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
    for doc in BUILTINS {
        set.insert(doc.name.to_string());
    }
    for entry in ctx.index.top_level() {
        set.insert(entry);
    }
    set.into_iter().collect()
}
