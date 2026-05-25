use crate::builtins::Context;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub struct Options {
    pub local: bool,
    pub eval: bool,
    pub command: Option<String>,
}

pub fn parse_flags(args: &[String]) -> Options {
    let mut opts = Options {
        local: false,
        eval: false,
        command: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-l" | "--local" => opts.local = true,
            "--eval" => opts.eval = true,
            "--" => {
                opts.command = iter.next().cloned();
                break;
            }
            other => {
                opts.command = Some(other.to_string());
                break;
            }
        }
    }
    opts
}

/// The base directory for a `--local` command: `<cwd>/.<program>`. Must match
/// the convention `identity::local_root_in` discovers (`.<name>/libexec`), so a
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

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        println!("--local");
        println!("--eval");
        return 0;
    }

    let opts = parse_flags(args);
    let Some(command) = opts.command else {
        eprintln!("Please provide a command name to generate");
        return 1;
    };

    let program = &ctx.identity.name;
    let base_dir: PathBuf = if opts.local {
        local_base_dir(&std::env::current_dir().unwrap_or_default(), program)
    } else {
        ctx.identity.root.clone()
    };
    let libexec = base_dir.join("libexec");
    let filepath = libexec.join(format!("{program}-{command}"));

    if filepath.exists() {
        eprintln!("That command already exists");
        return 1;
    }
    if let Err(e) = fs::create_dir_all(&libexec) {
        eprintln!("{program}: could not create {}: {e}", libexec.display());
        return 1;
    }
    let body = if opts.eval {
        eval_template(program, &command)
    } else {
        command_template(program, &command)
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
        ]);
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
        let opts = parse_flags(&["--local".to_string()]);
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
