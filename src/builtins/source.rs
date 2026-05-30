use crate::builtins::Context;
use crate::index::Resolution;
use std::os::unix::process::CommandExt;
use std::process::Command;

fn which(cmd: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn complete(args: &[String], ctx: &Context) -> i32 {
    match ctx.index.resolve(args) {
        Resolution::NotFound => {
            if args.is_empty() {
                // Not using builtins here, since they have no source
                for command in ctx.index.top_level_command_names() {
                    println!("{}", command);
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
        eprintln!("Please provide a command name");
        return 1;
    }
    let command = args.join(" ");
    let info = match ctx.index.resolve(args) {
        Resolution::Builtin(_) => {
            eprintln!("Cannot show source of builtin command: {command}");
            return 5;
        }
        Resolution::Namespace { .. } => {
            eprintln!("Cannot show source of namespace: {command}");
            return 1;
        }
        Resolution::NotFound => {
            eprintln!("No such command: {command}");
            return 2;
        }
        Resolution::Command { command, .. } => command,
    };

    let bat = which("bat");
    let pager = std::env::var("PAGER").ok().filter(|p| !p.is_empty());
    let chosen = bat.or(pager).unwrap_or_else(|| "cat".to_string());

    let err = Command::new(&chosen).arg(&info.path).exec();
    eprintln!("{}: failed to exec {chosen}: {err}", ctx.identity.name);
    1
}
