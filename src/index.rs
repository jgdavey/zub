use crate::frontmatter;
use crate::frontmatter::FrontMatter;
use crate::identity::Identity;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct CommandInfo {
    pub name: String,
    pub path: PathBuf,
    pub front: FrontMatter,
    pub is_local: bool,
}

/// Discover all `<name>-<cmd>` commands. Local libexec is scanned first so it
/// wins on name collisions. Results are sorted by command name.
pub fn discover(id: &Identity) -> Vec<CommandInfo> {
    let prefix = format!("{}-", id.name);
    let mut found: BTreeMap<String, CommandInfo> = BTreeMap::new();

    let mut dirs: Vec<(PathBuf, bool)> = Vec::new();
    if let Some(local) = &id.local_root {
        dirs.push((local.join("libexec"), true));
    }
    dirs.push((id.root.join("libexec"), false));

    for (dir, is_local) in dirs {
        scan_dir(&dir, &prefix, is_local, &mut found);
    }

    found.into_values().collect()
}

fn scan_dir(
    dir: &Path,
    prefix: &str,
    is_local: bool,
    found: &mut BTreeMap<String, CommandInfo>,
) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let Some(command) = file_name.strip_prefix(prefix) else {
            continue;
        };
        if found.contains_key(command) {
            continue; // earlier (local) scan wins
        }
        let path = entry.path();
        let front = frontmatter::parse_file(&path).unwrap_or_default();
        found.insert(
            command.to_string(),
            CommandInfo { name: command.to_string(), path, front, is_local },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_cmd(dir: &std::path::Path, file: &str, body: &str) {
        let libexec = dir.join("libexec");
        fs::create_dir_all(&libexec).unwrap();
        fs::write(libexec.join(file), body).unwrap();
    }

    #[test]
    fn discovers_prefixed_commands_with_metadata() {
        let root = tempdir().unwrap();
        write_cmd(root.path(), "rush-who", "#!/bin/sh\n#@ summary: who\n");
        write_cmd(root.path(), "rush-where", "#!/bin/sh\n");
        let id = Identity {
            name: "rush".into(),
            root: root.path().to_path_buf(),
            local_root: None,
        };
        let cmds = discover(&id);
        let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["where", "who"]);
        let who = cmds.iter().find(|c| c.name == "who").unwrap();
        assert_eq!(who.front.summary.as_deref(), Some("who"));
    }

    #[test]
    fn ignores_files_without_prefix() {
        let root = tempdir().unwrap();
        write_cmd(root.path(), "notmine", "#!/bin/sh\n");
        write_cmd(root.path(), "rush-yes", "#!/bin/sh\n");
        let id = Identity { name: "rush".into(), root: root.path().to_path_buf(), local_root: None };
        let cmds = discover(&id);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].name, "yes");
    }

    #[test]
    fn local_takes_precedence_over_global() {
        let root = tempdir().unwrap();
        let local = tempdir().unwrap();
        write_cmd(root.path(), "rush-who", "#!/bin/sh\n#@ summary: global\n");
        write_cmd(local.path(), "rush-who", "#!/bin/sh\n#@ summary: local\n");
        let id = Identity {
            name: "rush".into(),
            root: root.path().to_path_buf(),
            local_root: Some(local.path().to_path_buf()),
        };
        let cmds = discover(&id);
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].is_local);
        assert_eq!(cmds[0].front.summary.as_deref(), Some("local"));
    }
}
