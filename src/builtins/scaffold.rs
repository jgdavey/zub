use crate::builtins::Context;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;

/// Create a new sub program tree at `target`: `sub.yml`, `bin/<name>` symlinked
/// to `binary`, and empty `libexec`/`completions`/`share` directories.
pub fn create_program(_ctx: &Context, target: &Path, name: &str, binary: &Path) -> io::Result<()> {
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    fs::create_dir_all(target.join("bin"))?;
    fs::create_dir_all(target.join("libexec"))?;
    fs::create_dir_all(target.join("completions"))?;
    fs::create_dir_all(target.join("share"))?;

    fs::write(
        target.join("zub.yml"),
        format!("name: {name}\nversion: 0.1.0\n"),
    )?;
    symlink(binary, target.join("bin").join(name))?;
    Ok(())
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    let Some(name) = args.first() else {
        eprintln!("usage: {} scaffold <program>", ctx.identity.name);
        return 1;
    };
    let binary = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}: cannot locate binary: {e}", ctx.identity.name);
            return 1;
        }
    };
    let target = std::env::current_dir().unwrap_or_default().join(name);

    match create_program(ctx, &target, name, &binary) {
        Ok(()) => {
            println!("Created {} at {}", name, target.display());
            println!("Next: cd {name} && ./bin/{name} init", name = name);
            0
        }
        Err(e) => {
            eprintln!("{}: {e}", ctx.identity.name);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::identity::Identity;
    use crate::index::CommandInfo;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn ctx() -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity {
            name: "sub".into(),
            root: PathBuf::from("/opt/sub"),
            local_root: None,
        };
        (id, None, Vec::new())
    }

    #[test]
    fn creates_program_tree() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            commands: &cmds,
        };
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        let binary = Path::new("/usr/local/bin/sub");

        create_program(&ctx, &target, "rush", binary).unwrap();

        assert!(target.join("zub.yml").exists());
        assert!(target.join("libexec").is_dir());
        assert!(target.join("completions").is_dir());
        assert!(target.join("share").is_dir());
        let cfg = std::fs::read_to_string(target.join("zub.yml")).unwrap();
        assert!(cfg.contains("name: rush"));
        let link = std::fs::read_link(target.join("bin").join("rush")).unwrap();
        assert_eq!(link, binary);
    }

    #[test]
    fn refuses_existing_directory() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            commands: &cmds,
        };
        let work = tempdir().unwrap();
        let target = work.path().join("taken");
        std::fs::create_dir(&target).unwrap();
        assert!(create_program(&ctx, &target, "taken", Path::new("/bin/sub")).is_err());
    }
}
