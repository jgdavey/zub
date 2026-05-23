use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::Command;
use tempfile::tempdir;

/// Build a temp program tree with one external command and return its root.
fn program_tree(name: &str) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let libexec = dir.path().join("libexec");
    fs::create_dir_all(&libexec).unwrap();
    fs::write(dir.path().join("sub.yml"), format!("name: {name}\n")).unwrap();
    let script = libexec.join(format!("{name}-hi"));
    fs::write(&script, "#!/bin/sh\necho hello-from-hi\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

fn run_program(root: &std::path::Path, name: &str, args: &[&str]) -> std::process::Output {
    let bin = env!("CARGO_BIN_EXE_zub");
    Command::new(bin)
        .arg0(name)
        .args(args)
        .env(format!("_{}_ROOT", name.to_uppercase()), root)
        .output()
        .unwrap()
}

#[test]
fn dispatches_external_command() {
    let tree = program_tree("rush");
    let out = run_program(tree.path(), "rush", &["hi"]);
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello-from-hi");
}

#[test]
fn unknown_command_errors() {
    let tree = program_tree("rush");
    let out = run_program(tree.path(), "rush", &["nope"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no such command `nope'"));
}
