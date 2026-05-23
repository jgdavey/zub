use crate::builtins::Context;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct Options {
    pub local: bool,
    pub sh: bool,
    pub command: Option<String>,
}

pub fn parse_flags(args: &[String]) -> Options {
    let mut opts = Options { local: false, sh: false, command: None };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-l" | "--local" => opts.local = true,
            "--sh" => opts.sh = true,
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

/// The body for the optional `sh-` companion.
pub fn sh_template(program: &str, command: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         # Call main command\n\
         output=\"$({program}-{command})\"\n\
         echo \"OUTPUT FROM MAIN COMMAND: $output\" >&2\n\
         echo \"Any output to stdout gets evaled by the shell\" >&2\n\
         \n\
         echo \"pwd\"\n"
    )
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        println!("--local");
        println!("--sh");
        return 0;
    }

    let opts = parse_flags(args);
    let Some(command) = opts.command else {
        eprintln!("Please provide a command name to generate");
        return 1;
    };

    let program = &ctx.identity.name;
    let base_dir: PathBuf = if opts.local {
        std::env::current_dir().unwrap_or_default().join(".sub")
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
    if let Err(e) = fs::write(&filepath, command_template(program, &command)) {
        eprintln!("{program}: could not write {}: {e}", filepath.display());
        return 1;
    }
    let _ = fs::set_permissions(&filepath, fs::Permissions::from_mode(0o755));

    if opts.sh {
        let sh_path = libexec.join(format!("{program}-sh-{command}"));
        let _ = fs::write(&sh_path, sh_template(program, &command));
        let _ = fs::set_permissions(&sh_path, fs::Permissions::from_mode(0o755));
    }

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
    fn parse_flags_reads_local_and_sh() {
        let opts = parse_flags(&[
            "--local".to_string(),
            "--sh".to_string(),
            "greet".to_string(),
        ]);
        assert!(opts.local);
        assert!(opts.sh);
        assert_eq!(opts.command.as_deref(), Some("greet"));
    }

    #[test]
    fn parse_flags_requires_command() {
        let opts = parse_flags(&["--local".to_string()]);
        assert_eq!(opts.command, None);
    }
}
