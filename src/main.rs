use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::exit;

use lexopt::prelude::*;
use zub::builtins::{self, Context};
use zub::config;
use zub::env_setup;
use zub::identity;
use zub::index::{self, Resolution};

/// A parsed top-level invocation. `config` holds the `-C/--config` value (the
/// `ZUB_CONFIG` fallback is applied later). When `help` is set, the `help`
/// built-in runs with `rest` as its target; otherwise `rest` is the command
/// and its arguments, captured verbatim so unrecognized flags pass straight
/// through to the external command.
#[derive(Debug, PartialEq)]
struct Invocation {
    config: Option<PathBuf>,
    help: bool,
    rest: Vec<String>,
}

/// Parse the global options (`-C/--config <path>`, `-h/--help`) up to the first
/// positional (the command), then capture the command and everything after it
/// untouched. A leading `-h`/`--help`, an empty command, or no command at all
/// routes to the `help` built-in. Only the leading globals are parsed; the rest
/// is raw, so a subcommand's own flags are never interpreted by `zub`.
fn parse_args<I>(args: I) -> Result<Invocation, lexopt::Error>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let mut config = None;
    let mut parser = lexopt::Parser::from_args(args);
    while let Some(arg) = parser.next()? {
        match arg {
            Short('C') | Long("config") => config = Some(parser.value()?.into()),
            Short('h') | Long("help") => {
                return Ok(Invocation {
                    config,
                    help: true,
                    rest: raw_rest(&mut parser)?,
                });
            }
            Value(cmd) => {
                let cmd = cmd.string()?;
                // An empty command behaves like a bare `help` request.
                if cmd.is_empty() {
                    return Ok(Invocation {
                        config,
                        help: true,
                        rest: raw_rest(&mut parser)?,
                    });
                }
                let mut rest = vec![cmd];
                rest.extend(raw_rest(&mut parser)?);
                return Ok(Invocation {
                    config,
                    help: false,
                    rest,
                });
            }
            other => return Err(other.unexpected()),
        }
    }
    // No command given → help.
    Ok(Invocation {
        config,
        help: true,
        rest: Vec::new(),
    })
}

/// Collect the parser's remaining arguments verbatim (no option parsing).
fn raw_rest(parser: &mut lexopt::Parser) -> Result<Vec<String>, lexopt::Error> {
    Ok(parser
        .raw_args()?
        .map(|s| s.to_string_lossy().into_owned())
        .collect())
}

fn main() {
    let Invocation { config, help, rest } = match parse_args(env::args_os().skip(1)) {
        Ok(invocation) => invocation,
        Err(e) => {
            eprintln!("zub: {e}");
            exit(2);
        }
    };

    // `-C/--config` wins; otherwise fall back to `ZUB_CONFIG`, else error.
    let config_path = config.or_else(|| env::var_os(env_setup::CONFIG).map(PathBuf::from));
    let Some(config_path) = config_path else {
        eprintln!(
            "zub: no config; pass -C <path> or set {}",
            env_setup::CONFIG
        );
        exit(2);
    };

    let config = match config::load(&config_path) {
        Ok(config) => config,
        Err(err) => {
            eprintln!(
                "zub: could not load config at {}: {err}",
                config_path.display()
            );
            exit(1);
        }
    };

    let Some(identity) = identity::resolve(&config_path, &config) else {
        eprintln!(
            "zub: could not resolve program root from {}",
            config_path.display()
        );
        exit(1);
    };

    env_setup::apply(&identity);

    let index = index::discover(&identity);

    let ctx = Context {
        identity: &identity,
        index: &index,
    };

    // A `-h/--help`, an empty command, or no command runs the `help` built-in.
    if help {
        exit(builtins::run("help", &rest, &ctx));
    }

    match index.resolve(&rest) {
        Resolution::Builtin(builtin) => exit((builtin.run)(&rest[1..], &ctx)),
        Resolution::Command { command } => {
            let consumed = command.components.len();
            index::exec_external(&identity.name, command.path.as_ref(), &rest[consumed..])
        }
        Resolution::Namespace { .. } => {
            exit(builtins::run("help", &rest, &ctx));
        }
        Resolution::NotFound => {
            eprintln!("{}: no such command `{}'", identity.name, rest.join(" "));
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation, lexopt::Error> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_args_runs_help() {
        let inv = parse(&[]).unwrap();
        assert!(inv.help);
        assert_eq!(inv.config, None);
        assert!(inv.rest.is_empty());
    }

    #[test]
    fn help_flag_runs_help() {
        assert!(parse(&["-h"]).unwrap().help);
        assert!(parse(&["--help"]).unwrap().help);
    }

    #[test]
    fn help_flag_carries_target() {
        let inv = parse(&["--help", "db", "migrate"]).unwrap();
        assert!(inv.help);
        assert_eq!(inv.rest, vec!["db", "migrate"]);
    }

    #[test]
    fn empty_command_runs_help() {
        let inv = parse(&[""]).unwrap();
        assert!(inv.help);
    }

    #[test]
    fn config_then_command() {
        let inv = parse(&["-C", "/c/zub.yml", "hi"]).unwrap();
        assert_eq!(inv.config, Some(PathBuf::from("/c/zub.yml")));
        assert!(!inv.help);
        assert_eq!(inv.rest, vec!["hi"]);
    }

    #[test]
    fn config_long_and_equals_forms() {
        assert_eq!(
            parse(&["--config", "/c", "hi"]).unwrap().config,
            Some(PathBuf::from("/c"))
        );
        assert_eq!(
            parse(&["--config=/c", "hi"]).unwrap().config,
            Some(PathBuf::from("/c"))
        );
    }

    #[test]
    fn command_flags_pass_through_verbatim() {
        let inv = parse(&["hi", "--force", "-x", "--config", "ignored"]).unwrap();
        assert!(!inv.help);
        // Everything after the command is raw — even `--config` and `-x`.
        assert_eq!(inv.config, None);
        assert_eq!(inv.rest, vec!["hi", "--force", "-x", "--config", "ignored"]);
    }

    #[test]
    fn help_after_command_passes_through() {
        // `--help` after the command is the command's flag, not zub's.
        let inv = parse(&["mycmd", "--help"]).unwrap();
        assert!(!inv.help);
        assert_eq!(inv.rest, vec!["mycmd", "--help"]);
    }

    #[test]
    fn missing_config_value_errors() {
        assert!(parse(&["--config"]).is_err());
        assert!(parse(&["-C"]).is_err());
    }

    #[test]
    fn unknown_leading_option_errors() {
        assert!(parse(&["--force"]).is_err());
    }
}
