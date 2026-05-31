use std::env;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::exit;

use lexopt::prelude::*;
use zub::scaffold::{self, Mode};

const USAGE: &str = "usage: zub-scaffold <program> [--dir <path>] [--regenerate[=clobber]]";

/// Parsed command-line arguments.
#[derive(Debug)]
struct Args {
    name: String,
    mode: Mode,
    /// The target directory to create the program in. When `None`, the program
    /// is created at `<cwd>/<name>`.
    dir: Option<String>,
    /// `-V/--version` was given: print the version and exit (no name required).
    version: bool,
}

/// Parse the argument list (everything after the binary name). `-V/--version`
/// short-circuits; `--regenerate` takes an optional `=clobber`; `--dir` takes a
/// value; the lone positional is the program name.
fn parse_args<I>(args: I) -> Result<Args, lexopt::Error>
where
    I: IntoIterator,
    I::Item: Into<OsString>,
{
    let mut name: Option<String> = None;
    let mut mode = Mode::Normal;
    let mut dir: Option<String> = None;

    let mut parser = lexopt::Parser::from_args(args);
    while let Some(arg) = parser.next()? {
        match arg {
            Short('V') | Long("version") => {
                return Ok(Args {
                    name: name.unwrap_or_default(),
                    mode,
                    dir,
                    version: true,
                });
            }
            Long("dir") => dir = Some(parser.value()?.string()?),
            Long("regenerate") => {
                mode = match parser.optional_value() {
                    None => Mode::Regenerate,
                    Some(v) => match v.string()?.as_str() {
                        "clobber" => Mode::Clobber,
                        other => return Err(format!("invalid --regenerate value `{other}`").into()),
                    },
                };
            }
            Value(val) if name.is_none() => name = Some(val.string()?),
            _ => return Err(arg.unexpected()),
        }
    }

    let name = name.ok_or("missing program name")?;
    Ok(Args {
        name,
        mode,
        dir,
        version: false,
    })
}

fn main() {
    let parsed = match parse_args(env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("zub-scaffold: {e}\n{USAGE}");
            exit(1);
        }
    };
    let Args {
        name,
        mode,
        dir,
        version,
    } = parsed;

    // `-V/--version` prints the version and exits, before anything else.
    if version {
        println!("zub-scaffold {}", env!("CARGO_PKG_VERSION"));
        exit(0);
    }

    // `--dir` overrides the target (a relative path is taken against the cwd);
    // otherwise the program is created at `<cwd>/<name>`.
    let cwd = env::current_dir().unwrap_or_default();
    let target = match dir {
        Some(d) => {
            let p = PathBuf::from(d);
            if p.is_absolute() {
                p
            } else {
                cwd.join(p)
            }
        }
        None => cwd.join(&name),
    };

    let mut confirm = |path: &Path| prompt_replace(path);
    match scaffold::create_program(&target, &name, mode, &mut confirm) {
        Ok(()) => {
            if mode == Mode::Normal {
                println!("Created {} at {}", name, target.display());
                println!("Next steps:");
                println!("  - ensure `zub` is on your PATH");
                println!("  - cd {name} && ./bin/{name} init", name = name);
            } else {
                println!("Regenerated {} at {}", name, target.display());
            }
        }
        Err(e) => {
            eprintln!("zub-scaffold: {e}");
            exit(1);
        }
    }
}

/// Ask on stdin whether to replace an existing file. Empty input or EOF is no;
/// a leading `y`/`Y` is yes.
fn prompt_replace(path: &Path) -> bool {
    print!("Replace {}? [y/N] ", path.display());
    let _ = io::stdout().flush();

    let mut line = String::new();
    match io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => false,
        Ok(_) => matches!(line.trim_start().bytes().next(), Some(b'y' | b'Y')),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, lexopt::Error> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_name_only_with_defaults() {
        let a = parse(&["rush"]).unwrap();
        assert_eq!(a.name, "rush");
        assert_eq!(a.mode, Mode::Normal);
        assert_eq!(a.dir, None);
        assert!(!a.version);
    }

    #[test]
    fn version_flag_needs_no_name() {
        assert!(parse(&["-V"]).unwrap().version);
        assert!(parse(&["--version"]).unwrap().version);
    }

    #[test]
    fn parses_dir_with_separate_value() {
        let a = parse(&["rush", "--dir", "/opt/rush"]).unwrap();
        assert_eq!(a.dir.as_deref(), Some("/opt/rush"));
    }

    #[test]
    fn parses_dir_equals_form() {
        let a = parse(&["--dir=/opt/rush", "rush"]).unwrap();
        assert_eq!(a.dir.as_deref(), Some("/opt/rush"));
        assert_eq!(a.name, "rush");
    }

    #[test]
    fn dir_without_value_errors() {
        let msg = parse(&["rush", "--dir"]).unwrap_err().to_string();
        assert!(msg.contains("dir"), "got: {msg}");
    }

    #[test]
    fn parses_regenerate_modes() {
        assert_eq!(
            parse(&["rush", "--regenerate"]).unwrap().mode,
            Mode::Regenerate
        );
        assert_eq!(
            parse(&["rush", "--regenerate=clobber"]).unwrap().mode,
            Mode::Clobber
        );
    }

    #[test]
    fn combines_flags_in_any_order() {
        let a = parse(&["--regenerate", "--dir", "out", "rush"]).unwrap();
        assert_eq!(a.name, "rush");
        assert_eq!(a.mode, Mode::Regenerate);
        assert_eq!(a.dir.as_deref(), Some("out"));
    }

    #[test]
    fn missing_name_errors() {
        assert_eq!(parse(&[]).unwrap_err().to_string(), "missing program name");
        assert_eq!(
            parse(&["--dir", "x"]).unwrap_err().to_string(),
            "missing program name"
        );
    }

    #[test]
    fn unrecognized_option_errors() {
        let msg = parse(&["rush", "--nope"]).unwrap_err().to_string();
        assert!(msg.contains("nope"), "got: {msg}");
    }

    #[test]
    fn extra_positional_errors() {
        let msg = parse(&["rush", "extra"]).unwrap_err().to_string();
        assert!(msg.contains("extra"), "got: {msg}");
    }
}
