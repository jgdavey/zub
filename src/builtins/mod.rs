use crate::config::Config;
use crate::identity::Identity;
use crate::index::CommandInfo;

pub mod commands;
pub mod completions;
pub mod help;
pub mod init;
pub mod new;
pub mod scaffold;
pub mod source;

/// Documentation for a built-in command, used by `help` and `commands`.
pub struct BuiltinDoc {
    pub name: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub help: &'static str,
}

pub const BUILTIN_DOCS: &[BuiltinDoc] = &[
    BuiltinDoc { name: "commands", usage: "<name> commands", summary: "List all commands", help: "Mostly used for completion and `help`." },
    BuiltinDoc { name: "completions", usage: "<name> completions <command> [args...]", summary: "Drive subcommand completion", help: "Called by the shell completion scripts." },
    BuiltinDoc { name: "help", usage: "<name> help [<command>]", summary: "Show help for a command", help: "Run `<name> help <command>` for details." },
    BuiltinDoc { name: "init", usage: "<name> init [-]", summary: "Print shell integration", help: "Add `eval \"$(<name> init -)\"` to your shell profile." },
    BuiltinDoc { name: "new", usage: "<name> new [--local] [--sh] <command>", summary: "Generate a new command", help: "Creates a libexec script with front-matter." },
    BuiltinDoc { name: "scaffold", usage: "<name> scaffold <program>", summary: "Create a new sub program", help: "Generates a program directory with sub.yml." },
    BuiltinDoc { name: "source", usage: "<name> source <command>", summary: "Print a command's source", help: "Pages the file with bat/$PAGER/cat." },
];

/// Shared context handed to every built-in.
pub struct Context<'a> {
    pub identity: &'a Identity,
    pub config: &'a Option<Config>,
    pub commands: &'a [CommandInfo],
}

/// Run a built-in by name. Real implementations land in Tasks 10–16.
pub fn run(name: &str, args: &[String], ctx: &Context) -> i32 {
    match name {
        "commands" => commands::run(args, ctx),
        "completions" => completions::run(args, ctx),
        "help" => help::run(args, ctx),
        "init" => init::run(args, ctx),
        "new" => new::run(args, ctx),
        "scaffold" => scaffold::run(args, ctx),
        "source" => source::run(args, ctx),
        _ => {
            eprintln!("{}: built-in `{name}' not implemented yet", ctx.identity.name);
            1
        }
    }
}

/// All command names: built-ins plus discovered externals (deduped, sorted).
pub fn all_command_names(ctx: &Context) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for doc in BUILTIN_DOCS {
        set.insert(doc.name.to_string());
    }
    for c in ctx.commands {
        set.insert(c.name.clone());
    }
    set.into_iter().collect()
}
