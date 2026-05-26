use crate::builtins::{top_level_names, Context};
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Choose the pager: `bat` if available, else `$PAGER`, else `cat`.
/// `bat_path` is `Some` when `bat` is on PATH; `pager_env` is `$PAGER`.
pub fn pager(bat_path: Option<&str>, pager_env: Option<&str>) -> String {
    if bat_path.is_some() {
        "bat".to_string()
    } else if let Some(p) = pager_env.filter(|p| !p.is_empty()) {
        p.to_string()
    } else {
        "cat".to_string()
    }
}

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
    let pager_env = std::env::var("PAGER").ok();
    let chosen = pager(bat.as_deref(), pager_env.as_deref());

    let err = Command::new(&chosen).arg(&info.path).exec();
    eprintln!("{}: failed to exec {chosen}: {err}", ctx.identity.name);
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_bat_then_pager_then_cat() {
        assert_eq!(pager(Some("bat"), Some("less")), "bat");
        assert_eq!(pager(None, Some("less")), "less");
        assert_eq!(pager(None, None), "cat");
    }
}
