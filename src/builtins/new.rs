use crate::builtins::Context;
use lexopt::prelude::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Options {
    pub local: bool,
    pub eval: bool,
    pub command: Option<String>,
}

/// Parse `new`'s flags: `-l`/`--local`, `--eval`, and the lone positional
/// command name (use `--` before a name that starts with a dash).
pub fn parse_flags(args: &[String]) -> Result<Options, lexopt::Error> {
    let mut opts = Options {
        local: false,
        eval: false,
        command: None,
    };
    let mut parser = lexopt::Parser::from_args(args.iter().cloned());
    while let Some(arg) = parser.next()? {
        match arg {
            Short('l') | Long("local") => opts.local = true,
            Long("eval") => opts.eval = true,
            Value(val) if opts.command.is_none() => opts.command = Some(val.string()?),
            _ => return Err(arg.unexpected()),
        }
    }
    Ok(opts)
}

/// The base directory for a `--local` command: `<cwd>/.<program>`. Must match
/// the default local command root (`$PWD/.<name>/libexec`), so a
/// locally-generated command is actually found at dispatch time.
fn local_base_dir(cwd: &Path, program: &str) -> PathBuf {
    cwd.join(format!(".{program}"))
}

/// The script body for a new subcommand.
pub fn command_template(program: &str, command: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         #@ usage: {program} {command}\n\
         #@ summary: (please add docs here)\n\
         #@ help: |\n\
         #@   (add longer optional help here, that\n\
         #@   can be multi-line and include examples)\n\
         \n\
         echo \"It was generated\"\n"
    )
}

/// The script body for a new eval command (`eval: true`). Its stdout is `eval`'d
/// by the shell, so the stub emits a shell statement rather than running work.
pub fn eval_template(program: &str, command: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         #@ usage: {program} {command}\n\
         #@ summary: (please add docs here)\n\
         #@ eval: true\n\
         \n\
         # stdout from an eval command is eval'd by your shell\n\
         echo 'cd /some/path'\n"
    )
}

pub fn complete(_args: &[String], _ctx: &Context) -> i32 {
    println!("--local");
    println!("--eval");
    0
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    let opts = match parse_flags(args) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("{}: {e}", ctx.identity.name);
            return 1;
        }
    };
    let Some(command) = opts.command else {
        eprintln!("Please provide a command name to generate");
        return 1;
    };

    let program = &ctx.identity.name;
    // A `--local` command goes in the per-directory overlay (`.<name>/libexec`,
    // the default local command root); otherwise it goes in the program's
    // primary (non-local) command root.
    let libexec: PathBuf = if opts.local {
        local_base_dir(&std::env::current_dir().unwrap_or_default(), program).join("libexec")
    } else {
        ctx.identity.new_command_dir()
    };
    // A `/` in the command name nests the file; the displayed (space-joined)
    // name is what the user types: `new db/migrate` -> `libexec/db/migrate`,
    // invoked as `<program> db migrate`.
    let filepath = libexec.join(&command);
    let display = command.replace('/', " ");

    if filepath.exists() {
        eprintln!("That command already exists");
        return 1;
    }
    let parent = filepath.parent().unwrap_or(&libexec);
    if let Err(e) = fs::create_dir_all(parent) {
        eprintln!("{program}: could not create {}: {e}", parent.display());
        return 1;
    }
    let body = if opts.eval {
        eval_template(program, &display)
    } else {
        command_template(program, &display)
    };
    if let Err(e) = fs::write(&filepath, body) {
        eprintln!("{program}: could not write {}: {e}", filepath.display());
        return 1;
    }
    let _ = fs::set_permissions(&filepath, fs::Permissions::from_mode(0o755));

    println!("Generated {}", filepath.display());
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{fixture, CommandRoot};
    use crate::index::Index;
    use tempfile::tempdir;

    fn run_in(root: &Path, args: &[&str]) {
        let id = fixture("rush", root);
        let index = Index::default();
        let ctx = Context {
            identity: &id,
            index: &index,
        };
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        run(&owned, &ctx);
    }

    #[test]
    fn run_writes_bare_executable_command_file() {
        let root = tempdir().unwrap();
        run_in(root.path(), &["greet"]);
        let path = root.path().join("libexec").join("greet");
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("#@ usage: rush greet"));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }

    #[test]
    fn run_writes_into_primary_command_root() {
        let root = tempdir().unwrap();
        let mut id = fixture("rush", root.path());
        // Commands are created in the primary (first non-local) root.
        id.command_roots = vec![CommandRoot {
            path: root.path().join("cmds"),
            is_local: false,
        }];
        let index = Index::default();
        let ctx = Context {
            identity: &id,
            index: &index,
        };
        run(&["greet".to_string()], &ctx);
        assert!(root.path().join("cmds").join("greet").exists());
        assert!(!root.path().join("libexec").join("greet").exists());
    }

    #[test]
    fn run_nested_creates_dirs_and_space_joined_usage() {
        let root = tempdir().unwrap();
        run_in(root.path(), &["db/migrate"]);
        let path = root.path().join("libexec").join("db").join("migrate");
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("#@ usage: rush db migrate"));
    }

    #[test]
    fn template_uses_program_name_and_command() {
        let t = command_template("rush", "who");
        assert!(t.starts_with("#!/usr/bin/env bash\n"));
        assert!(t.contains("#@ usage: rush who"));
        assert!(t.contains("#@ summary:"));
    }

    #[test]
    fn parse_flags_reads_local_and_eval() {
        let opts = parse_flags(&[
            "--local".to_string(),
            "--eval".to_string(),
            "greet".to_string(),
        ])
        .unwrap();
        assert!(opts.local);
        assert!(opts.eval);
        assert_eq!(opts.command.as_deref(), Some("greet"));
    }

    #[test]
    fn eval_template_declares_eval_and_emits_stub() {
        let t = eval_template("rush", "cd");
        assert!(t.contains("#@ eval: true"));
        assert!(t.contains("#@ usage: rush cd"));
        assert!(t.contains("echo 'cd /some/path'"));
    }

    #[test]
    fn parse_flags_requires_command() {
        let opts = parse_flags(&["--local".to_string()]).unwrap();
        assert_eq!(opts.command, None);
    }

    #[test]
    fn local_base_dir_uses_program_name() {
        // Must mirror identity::local_root_in, which looks in `.<name>/libexec`.
        assert_eq!(
            local_base_dir(Path::new("/work"), "rush"),
            PathBuf::from("/work/.rush")
        );
    }
}
