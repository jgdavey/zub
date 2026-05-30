use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::config::Config;

/// How `create_program` treats generated files that already exist.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Refuse if the target directory already exists (fresh creation only).
    Normal,
    /// Rewrite generated files, asking (via `confirm`) before replacing any
    /// that already exist; missing ones are written without asking.
    Regenerate,
    /// Rewrite generated files unconditionally, replacing existing ones.
    Clobber,
}

/// Create or refresh a zub program tree at `target`. The generated files are
/// `zub.yml`, the self-locating `bin/<name>` shim, the three completion
/// scripts, and the example `libexec/who` command; the user's own `libexec`
/// commands and `share` contents are never touched.
///
/// `mode` decides how pre-existing generated files are handled (see [`Mode`]).
/// `confirm` is consulted only in [`Mode::Regenerate`], once per existing file.
pub fn create_program(
    target: &Path,
    name: &str,
    mode: Mode,
    confirm: &mut dyn FnMut(&Path) -> bool,
) -> io::Result<()> {
    if mode == Mode::Normal && target.exists() {
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
    let yaml = serde_yaml::to_string(&config).map_err(|e| io::Error::other(e.to_string()))?;
    write_generated(
        &target.join("zub.yml"),
        yaml.as_bytes(),
        false,
        mode,
        confirm,
    )?;

    let shim = "#!/bin/sh\n\
                here=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
                exec zub -C \"$here/zub.yml\" \"$@\"\n";
    write_generated(
        &target.join("bin").join(name),
        shim.as_bytes(),
        true,
        mode,
        confirm,
    )?;

    let completions = target.join("completions");
    write_generated(
        &completions.join("_zub"),
        ZSH_SHARED_COMPLETER.as_bytes(),
        false,
        mode,
        confirm,
    )?;
    write_generated(
        &completions.join(format!("_{name}")),
        zsh_per_program(name).as_bytes(),
        false,
        mode,
        confirm,
    )?;
    write_generated(
        &completions.join(format!("{name}.bash")),
        bash_completion(name).as_bytes(),
        false,
        mode,
        confirm,
    )?;

    write_generated(
        &target.join("libexec").join("who"),
        example_command(name).as_bytes(),
        true,
        mode,
        confirm,
    )?;

    Ok(())
}

/// Write one generated file, honoring `mode` when the path already exists:
/// `Normal` skips (the upfront check means this shouldn't occur), `Clobber`
/// always replaces, and `Regenerate` replaces only when `confirm` agrees. A
/// path that doesn't exist is always written and never prompts.
fn write_generated(
    path: &Path,
    contents: &[u8],
    executable: bool,
    mode: Mode,
    confirm: &mut dyn FnMut(&Path) -> bool,
) -> io::Result<()> {
    if path.exists() {
        let replace = match mode {
            Mode::Normal => false,
            Mode::Clobber => true,
            Mode::Regenerate => confirm(path),
        };
        if !replace {
            return Ok(());
        }
    }
    fs::write(path, contents)?;
    if executable {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
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

    /// Fresh creation: Normal mode, confirm must never be called.
    fn create(target: &Path, name: &str) -> io::Result<()> {
        create_program(target, name, Mode::Normal, &mut |p| {
            panic!("confirm should not be called in Normal mode, got {p:?}")
        })
    }

    #[test]
    fn creates_program_tree() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

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
        create(&target, "rush").unwrap();

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
        create(&target, "rush").unwrap();

        let shared = fs::read_to_string(target.join("completions").join("_zub")).unwrap();
        // Name-agnostic: derives the program from $service, never hardcodes it.
        assert!(shared.contains("$service"));
        assert!(shared.contains("_call_program ${prog}-cmds"));
    }

    #[test]
    fn writes_zsh_per_program_file() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        let per = fs::read_to_string(target.join("completions").join("_rush")).unwrap();
        assert!(per.contains("#compdef rush"));
        assert!(per.contains("_zub \"$@\""));
    }

    #[test]
    fn writes_bash_completion_with_name_only_in_complete_line() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        let bash = fs::read_to_string(target.join("completions").join("rush.bash")).unwrap();
        assert!(bash.contains("complete -F _zub_complete rush"));
        // Body is generic: program name derived at runtime, not baked in.
        assert!(bash.contains("local prog=\"${COMP_WORDS[0]##*/}\""));
    }

    #[test]
    fn writes_executable_example_who_command() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        let cmd_path = target.join("libexec").join("who");
        let body = fs::read_to_string(&cmd_path).unwrap();
        // Front-matter the indexer can parse, with the name substituted in.
        assert!(body.contains("#@ summary: Show who"));
        assert!(body.contains("#@ usage: $0 [opts...]"));
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
        assert!(create(&target, "taken").is_err());
    }

    #[test]
    fn regenerate_skips_existing_file_when_confirm_declines() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        let shim = target.join("bin").join("rush");
        fs::write(&shim, "tampered").unwrap();

        create_program(&target, "rush", Mode::Regenerate, &mut |_| false).unwrap();

        assert_eq!(fs::read_to_string(&shim).unwrap(), "tampered");
    }

    #[test]
    fn regenerate_overwrites_existing_file_when_confirm_agrees() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        let shim = target.join("bin").join("rush");
        fs::write(&shim, "tampered").unwrap();

        create_program(&target, "rush", Mode::Regenerate, &mut |_| true).unwrap();

        assert!(fs::read_to_string(&shim).unwrap().contains("exec zub -C"));
    }

    #[test]
    fn regenerate_leaves_user_libexec_command_untouched() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        let mine = target.join("libexec").join("foo");
        fs::write(&mine, "my command").unwrap();

        // Replace everything we're asked about; `foo` is not in the generated
        // set, so it must neither be asked about nor changed.
        let mut asked = Vec::new();
        {
            let mut confirm = |p: &Path| {
                asked.push(p.to_path_buf());
                true
            };
            create_program(&target, "rush", Mode::Regenerate, &mut confirm).unwrap();
        }

        assert_eq!(fs::read_to_string(&mine).unwrap(), "my command");
        assert!(!asked.iter().any(|p| p.file_name() == Some("foo".as_ref())));
    }

    #[test]
    fn regenerate_fills_missing_file_without_prompting() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        let shared = target.join("completions").join("_zub");
        fs::remove_file(&shared).unwrap();

        let mut asked = Vec::new();
        {
            let mut confirm = |p: &Path| {
                asked.push(p.to_path_buf());
                false
            };
            create_program(&target, "rush", Mode::Regenerate, &mut confirm).unwrap();
        }

        assert!(
            shared.exists(),
            "missing generated file should be rewritten"
        );
        assert!(
            !asked.iter().any(|p| p.file_name() == Some("_zub".as_ref())),
            "a file that didn't exist should not be prompted about"
        );
    }

    #[test]
    fn clobber_overwrites_unconditionally() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create(&target, "rush").unwrap();

        fs::write(target.join("zub.yml"), "tampered").unwrap();
        fs::write(target.join("libexec").join("who"), "tampered").unwrap();

        create_program(&target, "rush", Mode::Clobber, &mut |p| {
            panic!("confirm should not be called in Clobber mode, got {p:?}")
        })
        .unwrap();

        assert!(fs::read_to_string(target.join("zub.yml"))
            .unwrap()
            .contains("name: rush"));
        assert!(fs::read_to_string(target.join("libexec").join("who"))
            .unwrap()
            .contains("#@ summary:"));
    }
}
