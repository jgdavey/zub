use crate::builtins;
use crate::index::{CommandInfo, Index, Node};

#[derive(Debug, PartialEq)]
pub enum Resolution<'a> {
    Builtin {
        builtin: &'a builtins::Builtin,
    },
    /// An external command, with the number of leading args its (possibly
    /// multi-token) name consumed. The remaining args are passed through.
    External {
        command: &'a CommandInfo,
        consumed: usize,
    },
    Namespace {
        subcommands: Vec<String>,
        consumed: usize,
    },
    NotFound,
}

use std::os::unix::process::CommandExt;
use std::process::Command;

/// Resolve the leading args to a built-in, an external executable, or not-found.
/// External names may be multi-token (`db migrate`); the longest leading run of
/// args that matches a command's space-joined name wins. Built-ins are
/// single-token and authoritative for `args[0]` unless a depth-1 external with
/// the same name declares `override: true`.
pub fn resolve<'a>(args: &'a [String], index: &'a Index) -> Resolution<'a> {
    let Some(first) = args.first() else {
        return Resolution::NotFound;
    };

    if let Some(builtin) = builtins::get(first) {
        let overriding = index.get(first).is_some_and(|c| c.front.overrides);
        if !overriding {
            return Resolution::Builtin { builtin };
        }
    }

    match index.resolve(args) {
        Some((consumed, Node::Leaf(c))) => Resolution::External {
            command: c,
            consumed,
        },
        Some((consumed, node @ Node::Branch(_))) => Resolution::Namespace {
            subcommands: node.children().unwrap(),
            consumed,
        },
        None => Resolution::NotFound,
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
    use crate::index::CommandInfo;
    use std::path::PathBuf;

    fn cmd(name: &str, overrides: bool) -> CommandInfo {
        CommandInfo {
            name: name.to_string(),
            path: PathBuf::from(format!("/libexec/{}", name.replace(' ', "/"))),
            front: FrontMatter {
                overrides,
                ..Default::default()
            },
            is_local: false,
        }
    }

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    fn index(cmds: Vec<CommandInfo>) -> Index {
        Index::from_leaves(cmds)
    }

    #[test]
    fn external_command_resolves_with_one_token_consumed() {
        let command = cmd("who", false);
        assert_eq!(
            resolve(&args(&["who"]), &index(vec![command.clone()])),
            Resolution::External {
                command: &command,
                consumed: 1,
            }
        );
    }

    #[test]
    fn nested_command_consumes_its_tokens_and_passes_rest() {
        let command = cmd("db migrate", false);
        assert_eq!(
            resolve(
                &args(&["db", "migrate", "--force"]),
                &index(vec![command.clone()])
            ),
            Resolution::External {
                command: &command,
                consumed: 2,
            }
        );
    }

    #[test]
    fn namespace_prefix_alone_is_namespace() {
        assert_eq!(
            resolve(&args(&["db"]), &index(vec![cmd("db migrate", false)])),
            Resolution::Namespace {
                consumed: 1,
                subcommands: vec![String::from("migrate")]
            }
        );
    }

    #[test]
    fn unknown_command_is_not_found() {
        assert_eq!(
            resolve(&args(&["nope"]), &index(vec![])),
            Resolution::NotFound
        );
    }

    #[test]
    fn reserved_name_resolves_to_builtin() {
        assert_eq!(
            resolve(&args(&["help"]), &index(vec![])),
            Resolution::Builtin {
                builtin: builtins::get("help").unwrap()
            }
        );
    }

    #[test]
    fn reserved_name_not_overridden_without_flag() {
        assert_eq!(
            resolve(&args(&["help"]), &index(vec![cmd("help", false)])),
            Resolution::Builtin {
                builtin: builtins::get("help").unwrap()
            }
        );
    }

    #[test]
    fn reserved_name_overridden_with_flag() {
        let command = cmd("help", true);
        assert_eq!(
            resolve(&args(&["help"]), &index(vec![command.clone()])),
            Resolution::External {
                command: &command,
                consumed: 1,
            }
        );
    }
}
