use crate::index::CommandInfo;
use std::path::PathBuf;

/// The set of command names owned by the binary.
pub const BUILTINS: [&str; 6] = ["commands", "help", "completions", "init", "new", "source"];

#[derive(Debug, PartialEq)]
pub enum Resolution {
    Builtin(String),
    External(PathBuf),
    NotFound,
}

use std::os::unix::process::CommandExt;
use std::process::Command;

/// Resolve a command name to a built-in, an external executable, or not-found.
/// Built-ins are authoritative unless an external command with the same name
/// declares `override: true`.
pub fn resolve(command: &str, commands: &[CommandInfo]) -> Resolution {
    let external = commands.iter().find(|c| c.name == command);
    if BUILTINS.contains(&command) {
        match external {
            Some(c) if c.front.overrides => Resolution::External(c.path.clone()),
            _ => Resolution::Builtin(command.to_string()),
        }
    } else {
        match external {
            Some(c) => Resolution::External(c.path.clone()),
            None => Resolution::NotFound,
        }
    }
}

/// Replace the current process with the external command. Only returns on error.
pub fn exec_external(name: &str, path: &std::path::Path, args: &[String]) -> ! {
    let err = Command::new(path).args(args).exec();
    // Only gets here if exec failed
    eprintln!("{name}: failed to exec {}: {err}", path.display());
    std::process::exit(126);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::FrontMatter;

    fn cmd(name: &str, overrides: bool) -> CommandInfo {
        CommandInfo {
            name: name.to_string(),
            path: PathBuf::from(format!("/libexec/rush-{name}")),
            front: FrontMatter {
                overrides,
                ..Default::default()
            },
            is_local: false,
        }
    }

    #[test]
    fn external_command_resolves_to_its_path() {
        let cmds = vec![cmd("who", false)];
        assert_eq!(
            resolve("who", &cmds),
            Resolution::External(PathBuf::from("/libexec/rush-who"))
        );
    }

    #[test]
    fn unknown_command_is_not_found() {
        assert_eq!(resolve("nope", &[]), Resolution::NotFound);
    }

    #[test]
    fn reserved_name_resolves_to_builtin() {
        assert_eq!(
            resolve("help", &[]),
            Resolution::Builtin("help".to_string())
        );
    }

    #[test]
    fn reserved_name_not_overridden_without_flag() {
        let cmds = vec![cmd("help", false)];
        assert_eq!(
            resolve("help", &cmds),
            Resolution::Builtin("help".to_string())
        );
    }

    #[test]
    fn reserved_name_overridden_with_flag() {
        let cmds = vec![cmd("help", true)];
        assert_eq!(
            resolve("help", &cmds),
            Resolution::External(PathBuf::from("/libexec/rush-help"))
        );
    }
}
