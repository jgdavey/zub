use std::env;
use std::path::PathBuf;
use std::process::exit;

use zub::builtins::{self, Context};
use zub::config;
use zub::env_setup;
use zub::identity;
use zub::index::{self, Resolution};

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
        eprintln!(
            "zub: no config; pass -C <path> or set {}",
            env_setup::CONFIG
        );
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

    env_setup::apply(&identity);

    let index = index::discover(&identity);

    let ctx = Context {
        identity: &identity,
        index: &index,
    };

    // No command, or an explicit help flag, runs the `help` built-in.
    let first = rest.first().map(String::as_str);
    if rest.is_empty() || matches!(first, Some("") | Some("-h") | Some("--help")) {
        let help_args = if rest.is_empty() { &[][..] } else { &rest[1..] };
        exit(builtins::run("help", help_args, &ctx));
    }

    match index.resolve(&rest) {
        Resolution::Builtin(builtin) => exit((builtin.run)(&rest[1..], &ctx)),
        Resolution::Command { command, consumed } => {
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
