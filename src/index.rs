use crate::frontmatter;
use crate::frontmatter::FrontMatter;
use crate::identity::Identity;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct CommandInfo {
    /// Full, space-joined command name (`db migrate`). Kept for display, the
    /// eval wrapper, and error messages.
    pub name: String,
    pub path: PathBuf,
    pub front: FrontMatter,
    pub is_local: bool,
}

/// A node in the command tree: either a leaf command or a namespace branch.
/// (A filesystem name is a file xor a directory; on a local/root overlay
/// conflict, the first scan — local — wins the slot.)
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Leaf(CommandInfo),
    Branch(BTreeMap<String, Node>),
}

/// The command tree, rooted at a branch keyed by first path component.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Index(BTreeMap<String, Node>);

impl Index {
    /// Greedily match the longest leading run of `args` to a leaf command,
    /// returning `(tokens_consumed, command)`. A run that ends on a branch (a
    /// namespace) or misses returns `None`.
    pub fn resolve(&self, args: &[String]) -> Option<(usize, &CommandInfo)> {
        let mut branch = &self.0;
        for (i, tok) in args.iter().enumerate() {
            match branch.get(tok)? {
                Node::Leaf(info) => return Some((i + 1, info)),
                Node::Branch(next) => branch = next,
            }
        }
        None
    }

    /// Navigate a space-joined `name` to its node, if any.
    fn node(&self, name: &str) -> Option<&Node> {
        if name.is_empty() {
            return None;
        }
        let comps: Vec<&str> = name.split(' ').collect();
        let mut branch = &self.0;
        for (i, tok) in comps.iter().enumerate() {
            let node = branch.get(*tok)?;
            if i + 1 == comps.len() {
                return Some(node);
            }
            match node {
                Node::Branch(next) => branch = next,
                Node::Leaf(_) => return None, // path runs through a leaf
            }
        }
        None
    }

    /// The leaf command named exactly `name`, if any.
    pub fn get(&self, name: &str) -> Option<&CommandInfo> {
        match self.node(name)? {
            Node::Leaf(info) => Some(info),
            Node::Branch(_) => None,
        }
    }

    /// Whether `name` is a namespace (a branch).
    pub fn is_namespace(&self, name: &str) -> bool {
        matches!(self.node(name), Some(Node::Branch(_)))
    }

    /// Sorted child component names under namespace `prefix` (empty otherwise).
    pub fn children(&self, prefix: &str) -> Vec<String> {
        match self.node(prefix) {
            Some(Node::Branch(b)) => b.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Sorted top-level entry names (depth-1 leaves and namespaces).
    pub fn top_level(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    /// All leaf commands, sorted by full name.
    pub fn leaves(&self) -> Vec<&CommandInfo> {
        let mut out = Vec::new();
        collect_leaves(&self.0, &mut out);
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

#[cfg(test)]
impl Index {
    /// Build an index from leaf commands, keyed by each one's space-joined
    /// `name`. For tests that construct commands directly.
    pub fn from_leaves(cmds: Vec<CommandInfo>) -> Index {
        let mut root = BTreeMap::new();
        for info in cmds {
            let comps: Vec<String> = info.name.split(' ').map(String::from).collect();
            insert(&mut root, &comps, info);
        }
        Index(root)
    }
}

fn collect_leaves<'a>(branch: &'a BTreeMap<String, Node>, out: &mut Vec<&'a CommandInfo>) {
    for node in branch.values() {
        match node {
            Node::Leaf(info) => out.push(info),
            Node::Branch(b) => collect_leaves(b, out),
        }
    }
}

/// Insert a command at its path components into the tree. The first occupant of
/// a slot wins: an existing leaf or branch is never replaced, and a conflicting
/// kind (leaf where a branch is needed, or vice versa) is dropped.
fn insert(branch: &mut BTreeMap<String, Node>, components: &[String], info: CommandInfo) {
    let (head, tail) = components.split_first().expect("non-empty components");
    if tail.is_empty() {
        branch.entry(head.clone()).or_insert(Node::Leaf(info));
        return;
    }
    let child = branch
        .entry(head.clone())
        .or_insert_with(|| Node::Branch(BTreeMap::new()));
    if let Node::Branch(next) = child {
        insert(next, tail, info);
    }
    // else: `head` is already a leaf — conflict, drop this command.
}

/// Discover all command executables under each libexec dir into a tree. Files
/// are indexed recursively; a command's name is its path relative to libexec
/// with separators as spaces (`db/migrate` -> `"db migrate"`). Local libexec is
/// scanned first, so it wins slot collisions.
pub fn discover(id: &Identity) -> Index {
    let mut root: BTreeMap<String, Node> = BTreeMap::new();

    let mut dirs: Vec<(PathBuf, bool)> = Vec::new();
    if let Some(local) = &id.local_root {
        dirs.push((local.join("libexec"), true));
    }
    dirs.push((id.root.join("libexec"), false));

    for (dir, is_local) in dirs {
        scan_dir(&dir, &dir, is_local, &mut root);
    }

    Index(root)
}

/// Recursively scan `dir`, inserting commands with names relative to `base`.
fn scan_dir(base: &Path, dir: &Path, is_local: bool, root: &mut BTreeMap<String, Node>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if file_name.starts_with('.') {
            continue; // skip dotfiles and dot-directories
        }
        let path = entry.path();
        // metadata() follows symlinks, so a symlink to an executable counts.
        let Ok(meta) = fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            scan_dir(base, &path, is_local, root);
            continue;
        }
        if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
            continue; // only executable regular files are commands
        }
        let Ok(rel) = path.strip_prefix(base) else {
            continue;
        };
        let components: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        if components.is_empty() {
            continue;
        }
        let name = components.join(" ");
        let front = frontmatter::parse_file(&path).unwrap_or_default();
        insert(
            root,
            &components,
            CommandInfo {
                name,
                path,
                front,
                is_local,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;

    /// Write an executable command file at `libexec/<rel>` (rel may be nested).
    fn write_exec(dir: &std::path::Path, rel: &str, body: &str) {
        let path = dir.join("libexec").join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn id_for(root: &std::path::Path, local: Option<PathBuf>) -> Identity {
        Identity {
            name: "rush".into(),
            root: root.to_path_buf(),
            local_root: local,
            config_path: PathBuf::new(),
        }
    }

    /// Build an Index directly from space-joined names (no filesystem).
    fn build(names: &[&str]) -> Index {
        let mut root = BTreeMap::new();
        for n in names {
            let comps: Vec<String> = n.split(' ').map(String::from).collect();
            let info = CommandInfo {
                name: n.to_string(),
                path: PathBuf::from(format!("/lx/{}", n.replace(' ', "/"))),
                front: FrontMatter::default(),
                is_local: false,
            };
            insert(&mut root, &comps, info);
        }
        Index(root)
    }

    #[test]
    fn discover_lists_leaves_with_metadata() {
        let root = tempdir().unwrap();
        write_exec(root.path(), "who", "#!/bin/sh\n#@ summary: who\n");
        write_exec(root.path(), "where", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), None));
        let names: Vec<&str> = index.leaves().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["where", "who"]);
        assert_eq!(
            index.get("who").unwrap().front.summary.as_deref(),
            Some("who")
        );
    }

    #[test]
    fn discover_nests_subdirectories() {
        let root = tempdir().unwrap();
        write_exec(root.path(), "db/migrate", "#!/bin/sh\n");
        write_exec(root.path(), "db/seed", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), None));
        assert!(index.get("db migrate").is_some());
        assert!(index.is_namespace("db"));
        assert_eq!(index.children("db"), vec!["migrate", "seed"]);
    }

    #[test]
    fn discover_skips_non_executable_and_dotfiles() {
        let root = tempdir().unwrap();
        write_exec(root.path(), "who", "#!/bin/sh\n");
        fs::write(root.path().join("libexec").join("README.md"), "notes\n").unwrap();
        write_exec(root.path(), ".hidden", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), None));
        let names: Vec<&str> = index.leaves().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["who"]);
    }

    #[test]
    fn local_leaf_wins_overlay_conflict_with_root_namespace() {
        // local file `db` (a leaf) vs root dir `db/migrate` (a namespace).
        let root = tempdir().unwrap();
        let local = tempdir().unwrap();
        write_exec(local.path(), "db", "#!/bin/sh\n#@ summary: local-db\n");
        write_exec(root.path(), "db/migrate", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), Some(local.path().to_path_buf())));
        assert_eq!(
            index.get("db").unwrap().front.summary.as_deref(),
            Some("local-db")
        );
        assert!(!index.is_namespace("db")); // root's db/migrate was dropped
        assert!(index.get("db migrate").is_none());
    }

    #[test]
    fn local_leaf_wins_same_name() {
        let root = tempdir().unwrap();
        let local = tempdir().unwrap();
        write_exec(root.path(), "who", "#!/bin/sh\n#@ summary: global\n");
        write_exec(local.path(), "who", "#!/bin/sh\n#@ summary: local\n");
        let index = discover(&id_for(root.path(), Some(local.path().to_path_buf())));
        let leaves = index.leaves();
        assert_eq!(leaves.len(), 1);
        assert!(leaves[0].is_local);
        assert_eq!(leaves[0].front.summary.as_deref(), Some("local"));
    }

    #[test]
    fn resolve_greedy_returns_deepest_leaf_and_consumed() {
        let index = build(&["db migrate"]);
        let args: Vec<String> = ["db", "migrate", "--force"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let (consumed, info) = index.resolve(&args).unwrap();
        assert_eq!(consumed, 2);
        assert_eq!(info.name, "db migrate");
    }

    #[test]
    fn resolve_namespace_prefix_is_none() {
        let index = build(&["db migrate"]);
        assert!(index.resolve(&["db".to_string()]).is_none());
    }

    #[test]
    fn get_children_top_level_and_namespace() {
        let index = build(&["who", "db migrate", "db seed", "db schema dump"]);
        assert!(index.get("who").is_some());
        assert!(index.get("db").is_none()); // a namespace, not a leaf
        assert_eq!(index.children("db"), vec!["migrate", "schema", "seed"]);
        assert_eq!(index.children("db schema"), vec!["dump"]);
        assert_eq!(index.top_level(), vec!["db", "who"]);
        assert!(index.is_namespace("db"));
        assert!(!index.is_namespace("who"));
    }

    #[test]
    fn leaves_are_sorted_by_full_name() {
        let index = build(&["who", "db seed", "db migrate"]);
        let names: Vec<&str> = index.leaves().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["db migrate", "db seed", "who"]);
    }
}
