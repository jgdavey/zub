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
        usage: "$0 [--eval | --no-eval]",
        summary: "List all commands",
        help: "Lists every command, one per line. `--eval`/`--no-eval` restrict \
               to (or exclude) commands whose front-matter sets `eval: true`. \
               Used by `init` to build the eval wrapper.",
        run: commands::run,
        complete: commands::complete,
    },
    Builtin {
        name: "completions",
        usage: "$0 <command> [args...]",
        summary: "Drive subcommand completion",
        help: "Called by the shell completion scripts.",
        run: completions::run,
        complete: completions::complete,
    },
    Builtin {
        name: "help",
        usage: "$0 [<command>]",
        summary: "Show help for a command",
        help: "Run `$0 <command>` for details.",
        run: help::run,
        complete: help::complete,
    },
    Builtin {
        name: "init",
        usage: "$0 [-]",
        summary: "Print shell integration",
        help: "Add `eval \"$($0 -)\"` to your shell profile.",
        run: init::run,
        complete: init::complete,
    },
    Builtin {
        name: "new",
        usage: "$0 [--local] [--eval] <command>",
        summary: "Generate a new command",
        help: "Creates a libexec script with front-matter.",
        run: new::run,
        complete: new::complete,
    },
    Builtin {
        name: "source",
        usage: "$0 <command>",
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
