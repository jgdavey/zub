use crate::builtins::Context;
use crate::index::{exec_or_report, Resolution};
use std::process::Command;

fn which(cmd: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn complete(args: &[String], ctx: &Context) -> i32 {
    // Built-ins have no source, so they are excluded from completion.
    super::complete_command_names(args, ctx, false)
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    let prog = &ctx.identity.name;
    if args.is_empty() {
        eprintln!("{prog}: please provide a command name");
        return crate::exit_codes::USAGE;
    }
    let command = args.join(" ");
    let info = match ctx.index.resolve(args) {
        Resolution::Builtin(_) => {
            eprintln!("{prog}: cannot show source of built-in `{command}'");
            return crate::exit_codes::FAILURE;
        }
        Resolution::Namespace { .. } => {
            eprintln!("{prog}: cannot show source of namespace `{command}'");
            return crate::exit_codes::FAILURE;
        }
        Resolution::NotFound => {
            eprintln!("{prog}: no such command `{command}'");
            return crate::exit_codes::NOT_FOUND;
        }
        Resolution::Command { command, .. } => command,
    };

    let bat = which("bat");
    let pager = std::env::var("PAGER").ok().filter(|p| !p.is_empty());
    let chosen = bat.or(pager).unwrap_or_else(|| "cat".to_string());

    let mut cmd = Command::new(&chosen);
    cmd.arg(&info.path);
    exec_or_report(cmd, prog, &chosen)
}
