use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

/// Write an executable command at `<root>/libexec/<rel>` (rel may be nested).
fn write_cmd(root: &Path, rel: &str, body: &str) {
    let path = root.join("libexec").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Build a temp program tree with one external command named `hi`. Returns the
/// temp dir; the config lives at `<root>/zub.yml`.
fn program_tree(name: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("zub.yml"), format!("name: {name}\n")).unwrap();
    write_cmd(dir.path(), "hi", "#!/bin/sh\necho hello-from-hi\n");
    dir
}

fn config_path(root: &Path) -> std::path::PathBuf {
    root.join("zub.yml")
}

#[test]
fn dispatches_external_command_via_flag() {
    let tree = program_tree("rush");
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("-C")
        .arg(config_path(tree.path()))
        .arg("hi")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello-from-hi");
}

#[test]
fn dispatches_external_command_via_env() {
    let tree = program_tree("rush");
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .env("ZUB_CONFIG", config_path(tree.path()))
        .arg("hi")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello-from-hi");
}

#[test]
fn flag_overrides_env() {
    let tree = program_tree("rush");
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .env("ZUB_CONFIG", "/nonexistent/zub.yml")
        .arg("-C")
        .arg(config_path(tree.path()))
        .arg("hi")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello-from-hi");
}

#[test]
fn missing_config_errors() {
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("hi")
        .env_remove("ZUB_CONFIG")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no config"));
}

#[test]
fn config_flag_without_value_errors() {
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("--config")
        .env_remove("ZUB_CONFIG")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("requires a path"));
}

#[test]
fn dispatches_nested_command_and_passes_remaining_args() {
    let tree = program_tree("rush");
    write_cmd(
        tree.path(),
        "db/migrate",
        "#!/bin/sh\necho \"migrate got: $*\"\n",
    );
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("-C")
        .arg(config_path(tree.path()))
        .args(["db", "migrate", "--force"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "migrate got: --force"
    );
}

#[test]
fn dynamic_help_appends_script_help_output() {
    let tree = program_tree("rush");
    // The command prints its own help when invoked with --help, and declares
    // dynamic_help so `help greet` runs it after the static front-matter text.
    write_cmd(
        tree.path(),
        "greet",
        "#!/bin/sh\n\
         #@ summary: greet someone\n\
         #@ usage: rush greet <name>\n\
         #@ help: Static help line.\n\
         #@ dynamic_help: true\n\
         if [ \"$1\" = --help ]; then echo \"Dynamic help line.\"; exit 0; fi\n\
         echo \"hi $1\"\n",
    );
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("-C")
        .arg(config_path(tree.path()))
        .args(["help", "greet"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Static help line."), "got: {stdout}");
    assert!(stdout.contains("Dynamic help line."), "got: {stdout}");
    // Static text precedes the script's own output.
    assert!(
        stdout.find("Static help line.").unwrap() < stdout.find("Dynamic help line.").unwrap(),
        "static help should come first: {stdout}"
    );
}

#[test]
fn static_help_does_not_run_script() {
    let tree = program_tree("rush");
    // No dynamic_help: the script must not be invoked, only the static text shows.
    write_cmd(
        tree.path(),
        "greet",
        "#!/bin/sh\n\
         #@ summary: greet someone\n\
         #@ usage: rush greet <name>\n\
         #@ help: Static help line.\n\
         if [ \"$1\" = --help ]; then echo \"Dynamic help line.\"; exit 0; fi\n\
         echo \"hi $1\"\n",
    );
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("-C")
        .arg(config_path(tree.path()))
        .args(["help", "greet"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Static help line."), "got: {stdout}");
    assert!(!stdout.contains("Dynamic help line."), "got: {stdout}");
}

#[test]
fn unknown_command_errors() {
    let tree = program_tree("rush");
    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("-C")
        .arg(config_path(tree.path()))
        .arg("nope")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no such command `nope'"));
}

#[test]
fn dispatches_command_from_configured_libexec_dir() {
    // Commands live in `cmds`, not the default `libexec`.
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("zub.yml"), "name: rush\nlibexec: cmds\n").unwrap();
    let cmd = dir.path().join("cmds").join("hi");
    fs::create_dir_all(cmd.parent().unwrap()).unwrap();
    fs::write(&cmd, "#!/bin/sh\necho hello-from-cmds\n").unwrap();
    fs::set_permissions(&cmd, fs::Permissions::from_mode(0o755)).unwrap();

    let bin = env!("CARGO_BIN_EXE_zub");
    let out = Command::new(bin)
        .arg("-C")
        .arg(config_path(dir.path()))
        .arg("hi")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "hello-from-cmds"
    );
}
