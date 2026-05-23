use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

/// Build a temp program tree with one external command. Returns the temp dir;
/// the config lives at `<root>/zub.yml`.
fn program_tree(name: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let libexec = dir.path().join("libexec");
    fs::create_dir_all(&libexec).unwrap();
    fs::write(dir.path().join("zub.yml"), format!("name: {name}\n")).unwrap();
    let script = libexec.join(format!("{name}-hi"));
    fs::write(&script, "#!/bin/sh\necho hello-from-hi\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
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
