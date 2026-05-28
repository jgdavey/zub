use std::env;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::process::exit;

use zub::scaffold::{self, Mode};

const USAGE: &str = "usage: zub-scaffold <program> [--regenerate[=clobber]]";

fn main() {
    let mut name: Option<String> = None;
    let mut mode = Mode::Normal;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--regenerate" => mode = Mode::Regenerate,
            "--regenerate=clobber" => mode = Mode::Clobber,
            other if other.starts_with("--") => {
                eprintln!("zub-scaffold: unrecognized option `{other}`\n{USAGE}");
                exit(1);
            }
            _ if name.is_none() => name = Some(arg),
            _ => {
                eprintln!("zub-scaffold: unexpected argument `{arg}`\n{USAGE}");
                exit(1);
            }
        }
    }

    let Some(name) = name else {
        eprintln!("{USAGE}");
        exit(1);
    };

    let target = env::current_dir().unwrap_or_default().join(&name);

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
