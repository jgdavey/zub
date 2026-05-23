use std::env;
use std::process::exit;

use zub::scaffold;

fn main() {
    let mut args = env::args().skip(1);
    let Some(name) = args.next() else {
        eprintln!("usage: zub-scaffold <program>");
        exit(1);
    };

    let target = env::current_dir().unwrap_or_default().join(&name);

    match scaffold::create_program(&target, &name) {
        Ok(()) => {
            println!("Created {} at {}", name, target.display());
            println!("Next steps:");
            println!("  - ensure `zub` is on your PATH");
            println!("  - cd {name} && ./bin/{name} init", name = name);
        }
        Err(e) => {
            eprintln!("zub-scaffold: {e}");
            exit(1);
        }
    }
}
