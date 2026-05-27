use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::config::Config;

/// Create a new zub program tree at `target`: `zub.yml` (with `root` omitted),
/// an executable self-locating `bin/<name>` shim, and empty
/// `libexec`/`completions`/`share` directories.
pub fn create_program(target: &Path, name: &str) -> io::Result<()> {
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    fs::create_dir_all(target.join("bin"))?;
    fs::create_dir_all(target.join("libexec"))?;
    fs::create_dir_all(target.join("completions"))?;
    fs::create_dir_all(target.join("share"))?;

    let config = Config {
        name: name.to_string(),
        root: None,
        version: None,
        description: Some("your description".to_string()),
    };
    let config_file = fs::File::create(target.join("zub.yml"))?;
    serde_yaml::to_writer(io::BufWriter::new(config_file), &config)
        .map_err(|e| io::Error::other(e.to_string()))?;

    let shim = "#!/bin/sh\n\
                here=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
                exec zub -C \"$here/zub.yml\" \"$@\"\n";
    let shim_path = target.join("bin").join(name);
    fs::write(&shim_path, shim)?;
    fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755))?;

    let completions = target.join("completions");
    fs::write(completions.join("_zub"), ZSH_SHARED_COMPLETER)?;
    fs::write(completions.join(format!("_{name}")), zsh_per_program(name))?;
    fs::write(
        completions.join(format!("{name}.bash")),
        bash_completion(name),
    )?;

    let example_path = target.join("libexec").join("who");
    fs::write(&example_path, example_command(name))?;
    fs::set_permissions(&example_path, fs::Permissions::from_mode(0o755))?;

    Ok(())
}

/// The shared zsh completer, written verbatim into every program's
/// `completions/_zub`. The program name is read from `$service` at completion
/// time, so this file never names a specific program and never drifts.
const ZSH_SHARED_COMPLETER: &str = include_str!("templates/completion-shared.zsh");

/// Per-program and bash templates. The `@NAME@` sentinel is substituted with
/// the program name; everything else is generic.
const ZSH_PER_PROGRAM: &str = include_str!("templates/completion-program.zsh");
const BASH_COMPLETION: &str = include_str!("templates/completion.bash");
const EXAMPLE_COMMAND: &str = include_str!("templates/example-command.sh");

/// The per-program zsh file (`completions/_<name>`). Carries the literal name
/// in its `#compdef` tag and filename; the body delegates to the shared
/// completer.
fn zsh_per_program(name: &str) -> String {
    ZSH_PER_PROGRAM.replace("@NAME@", name)
}

/// The bash completion (`completions/<name>.bash`). The body is generic — it
/// derives the program from `$COMP_WORDS` — so only the final `complete` line
/// names the program.
fn bash_completion(name: &str) -> String {
    BASH_COMPLETION.replace("@NAME@", name)
}

/// The example libexec command (`libexec/who`). Ships parseable
/// front-matter, a `--complete` branch, and forwards its arguments to the
/// system `who`.
fn example_command(name: &str) -> String {
    EXAMPLE_COMMAND.replace("@NAME@", name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_program_tree() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        assert!(target.join("zub.yml").exists());
        assert!(target.join("libexec").is_dir());
        assert!(target.join("completions").is_dir());
        assert!(target.join("share").is_dir());

        let cfg = fs::read_to_string(target.join("zub.yml")).unwrap();
        assert!(cfg.contains("name: rush"));
        assert!(!cfg.contains("root"));
    }

    #[test]
    fn writes_executable_self_locating_shim() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        let shim_path = target.join("bin").join("rush");
        let shim = fs::read_to_string(&shim_path).unwrap();
        assert!(shim.contains("exec zub -C \"$here/zub.yml\" \"$@\""));

        let mode = fs::metadata(&shim_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "shim should be executable");
    }

    #[test]
    fn writes_shared_zub_completer() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        let shared = fs::read_to_string(target.join("completions").join("_zub")).unwrap();
        // Name-agnostic: derives the program from $service, never hardcodes it.
        assert!(shared.contains("$service"));
        assert!(shared.contains("_call_program ${prog}-cmds"));
    }

    #[test]
    fn writes_zsh_per_program_file() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        let per = fs::read_to_string(target.join("completions").join("_rush")).unwrap();
        assert!(per.contains("#compdef rush"));
        assert!(per.contains("_zub \"$@\""));
    }

    #[test]
    fn writes_bash_completion_with_name_only_in_complete_line() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        let bash = fs::read_to_string(target.join("completions").join("rush.bash")).unwrap();
        assert!(bash.contains("complete -F _zub_complete rush"));
        // Body is generic: program name derived at runtime, not baked in.
        assert!(bash.contains("local prog=\"${COMP_WORDS[0]##*/}\""));
    }

    #[test]
    fn writes_executable_example_who_command() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        let cmd_path = target.join("libexec").join("who");
        let body = fs::read_to_string(&cmd_path).unwrap();
        // Front-matter the indexer can parse, with the name substituted in.
        assert!(body.contains("#@ summary:"));
        assert!(body.contains("#@ usage: rush who"));
        assert!(body.contains("#@ complete: true"));
        // Demonstrates --complete handling and forwards to the system `who`.
        assert!(body.contains("--complete"));
        assert!(body.contains("exec who \"$@\""));

        let mode = fs::metadata(&cmd_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "example command should be executable");
    }

    #[test]
    fn refuses_existing_directory() {
        let work = tempdir().unwrap();
        let target = work.path().join("taken");
        fs::create_dir(&target).unwrap();
        assert!(create_program(&target, "taken").is_err());
    }
}
