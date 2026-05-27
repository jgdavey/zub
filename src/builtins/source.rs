use crate::builtins::{top_level_names, Context};
use std::os::unix::process::CommandExt;
use std::process::Command;

fn which(cmd: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn complete(_args: &[String], ctx: &Context) -> i32 {
    for name in top_level_names(ctx) {
        println!("{name}");
    }
    0
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.is_empty() {
        eprintln!("Please provide a command name");
        return 1;
    }
    let command = args.join(" ");
    let Some(info) = ctx.index.get(&command) else {
        eprintln!("Could not find command {command}");
        return 1;
    };

    let bat = which("bat");
    let pager = std::env::var("PAGER").ok().filter(|p| !p.is_empty());
    let chosen = bat.or(pager).unwrap_or_else(|| "cat".to_string());

    let err = Command::new(&chosen).arg(&info.path).exec();
    eprintln!("{}: failed to exec {chosen}: {err}", ctx.identity.name);
    1
}
