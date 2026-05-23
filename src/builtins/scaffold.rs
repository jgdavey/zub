use crate::builtins::Context;
use crate::config::Config;
use std::fs;
use std::io;
use std::path::Path;

/// Create a new sub program tree at `target`: `sub.yml`, `bin/<name>` symlinked
/// to `binary`, and empty `libexec`/`completions`/`share` directories.
pub fn create_program(_ctx: &Context, target: &Path, name: &str) -> io::Result<()> {
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    fs::create_dir_all(target.join("libexec"))?;
    fs::create_dir_all(target.join("completions"))?;
    fs::create_dir_all(target.join("share"))?;

    // Okay to panic because previous lines would have failed
    let root = target.to_str().unwrap();

    let config = Config {
        name: String::from(name),
        root: Some(String::from(root)),
        description: Some(String::from("your description")),
        version: None,
    };

    // Create or open a file for writing
    let config_file = fs::File::create(target.join("zub.yml"))?;
    let writer = io::BufWriter::new(config_file);

    serde_yaml::to_writer(writer, &config)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
    Ok(())
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    let Some(name) = args.first() else {
        eprintln!("usage: {} scaffold <program>", ctx.identity.name);
        return 1;
    };
    let target = std::env::current_dir().unwrap_or_default().join(name);

    match create_program(ctx, &target, name) {
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
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn ctx() -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity {
            name: "sub".into(),
            root: PathBuf::from("/opt/sub"),
            local_root: None,
            config_path: PathBuf::new(),
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

        create_program(&ctx, &target, "rush").unwrap();

        assert!(target.join("zub.yml").exists());
        assert!(target.join("libexec").is_dir());
        assert!(target.join("completions").is_dir());
        assert!(target.join("share").is_dir());
        let cfg = std::fs::read_to_string(target.join("zub.yml")).unwrap();
        assert!(cfg.contains("name: rush"));
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
        assert!(create_program(&ctx, &target, "taken").is_err());
    }
}
