use std::env;
use std::path::Path;
use std::process::exit;

use sub::builtins::{self, Context};
use sub::config;
use sub::dispatch::{self, Resolution};
use sub::env_setup;
use sub::identity::{self, Identity};
use sub::index;

fn main() {
    let mut argv = env::args();
    let argv0 = argv.next().unwrap_or_default();
    let rest: Vec<String> = argv.collect();

    let name = identity::name_from_argv0(&argv0);

    // First arg is the command; "", "-h", "--help" map to `help`.
    let (command, cmd_args): (String, Vec<String>) = match rest.split_first() {
        None => ("help".to_string(), Vec::new()),
        Some((first, tail)) => {
            // normalize help command
            let c = match first.as_str() {
                "-h" | "--help" => "help".to_string(),
                other => other.to_string(),
            };
            (c, tail.to_vec())
        }
    };

    let root = match identity::resolve_root(&name, Path::new(&argv0)) {
        Some(r) => r,
        None if (&command == "scaffold") => match env::current_dir() {
            Ok(cwd) => cwd,
            Err(e) => {
                eprintln!("{name}: could not locate pwd: {}", e);
                exit(2);
            }
        },
        None => {
            eprintln!("{name}: could not locate program root");
            exit(1);
        }
    };
    let identity = Identity {
        name,
        root,
        local_root: identity::local_root(),
    };

    env_setup::apply(&identity);

    let config = config::load(&identity.root);
    let commands = index::discover(&identity);

    let ctx = Context {
        identity: &identity,
        config: &config,
        commands: &commands,
    };

    match dispatch::resolve(&command, &commands) {
        Resolution::Builtin(name) => exit(builtins::run(&name, &cmd_args, &ctx)),
        Resolution::External(path) => dispatch::exec_external(&identity.name, &path, &cmd_args),
        Resolution::NotFound => {
            eprintln!("{}: no such command `{}'", identity.name, command);
            exit(1);
        }
    }
}
