use std::env;
use std::path::PathBuf;
use std::process::exit;

use zub::builtins::{self, Context};
use zub::config;
use zub::dispatch::{self, Resolution};
use zub::env_setup;
use zub::identity;
use zub::index;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // Global config selector: `-C/--config <path>`, else `ZUB_CONFIG`, else error.
    let (config_path, rest): (Option<PathBuf>, Vec<String>) = match args.split_first() {
        Some((flag, tail)) if flag == "-C" || flag == "--config" => match tail.split_first() {
            Some((path, more)) => (Some(PathBuf::from(path)), more.to_vec()),
            None => {
                eprintln!("zub: {flag} requires a path");
                exit(2);
            }
        },
        _ => match env::var_os(env_setup::CONFIG) {
            Some(p) => (Some(PathBuf::from(p)), args.clone()),
            None => (None, args.clone()),
        },
    };

    let Some(config_path) = config_path else {
        eprintln!("zub: no config; pass -C <path> or set {}", env_setup::CONFIG);
        exit(2);
    };

    let Some(config) = config::load(&config_path) else {
        eprintln!("zub: could not load config at {}", config_path.display());
        exit(1);
    };

    let Some(identity) = identity::resolve(&config_path, &config) else {
        eprintln!(
            "zub: could not resolve program root from {}",
            config_path.display()
        );
        exit(1);
    };

    // First arg is the command; "", "-h", "--help" map to `help`.
    let (command, cmd_args): (String, Vec<String>) = match rest.split_first() {
        None => ("help".to_string(), Vec::new()),
        Some((first, tail)) => {
            let c = match first.as_str() {
                "" | "-h" | "--help" => "help".to_string(),
                other => other.to_string(),
            };
            (c, tail.to_vec())
        }
    };

    env_setup::apply(&identity);

    let commands = index::discover(&identity);
    let config = Some(config);

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
