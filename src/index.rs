use crate::builtins::{self, Builtin};
use crate::frontmatter;
use crate::frontmatter::FrontMatter;
use crate::identity::Identity;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

/// A leaf command in the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    /// The full path components (`["db", "migrate"]`).
    pub components: Vec<String>,
    /// The executable's filesystem path.
    pub path: PathBuf,
    pub front: FrontMatter,
    pub is_local: bool,
}

impl Command {
    /// The full, space-joined command name (`db migrate`). Used for display, the
    /// eval wrapper, and error messages.
    pub fn full_name(&self) -> String {
        self.components.join(" ")
    }

    pub fn name(&self) -> String {
        self.components.last().cloned().unwrap_or_default()
    }
}

/// A namespace branch in the tree — a directory grouping subcommands, holding
/// its own child nodes (just as a `Command` is held directly in a `Node::Leaf`).
#[derive(Debug, Clone, PartialEq)]
pub struct Namespace {
    /// The full path components leading to this namespace (`["db"]`).
    pub components: Vec<String>,
    /// The directory's filesystem path (the first scan to create the branch
    /// wins, so a local namespace's path is kept over an overlapping root one).
    pub path: PathBuf,
    /// Child nodes keyed by name.
    pub children: BTreeMap<String, Node>,
}

impl Namespace {
    /// The immediate child entry names, sorted.
    pub fn subcommands(&self) -> Vec<String> {
        self.children.keys().cloned().collect()
    }

    /// The immediate children as resolutions, sorted by name (BTreeMap order).
    pub fn child_resolutions(&self) -> Vec<Resolution<'_>> {
        self.children.values().map(Node::resolution).collect()
    }

    pub fn name(&self) -> String {
        self.components.last().cloned().unwrap_or_default()
    }
}

/// A node in the command tree: either a leaf command or a namespace branch.
/// (A filesystem name is a file xor a directory; on a local/root overlay
/// conflict, the first scan — local — wins the slot.)
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Leaf(Command),
    Branch(Namespace),
}

impl Node {
    pub fn is_namespace(&self) -> bool {
        matches!(self, Node::Branch(_))
    }

    /// View this node as a [`Resolution`]: a leaf is a `Command`, a branch a
    /// `Namespace`. Lets a caller that already holds a node build its resolution
    /// without re-resolving by name.
    pub fn resolution(&self) -> Resolution<'_> {
        match self {
            Node::Leaf(command) => Resolution::Command { command },
            Node::Branch(namespace) => Resolution::Namespace { namespace },
        }
    }
}

/// The outcome of resolving leading args against the index: a built-in, an
/// external command, a namespace, or nothing. A resolved entry's (possibly
/// multi-token) name took `components().len()` leading args; the rest pass
/// through.
#[derive(Debug, PartialEq)]
pub enum Resolution<'a> {
    Builtin(&'a Builtin),
    Command { command: &'a Command },
    Namespace { namespace: &'a Namespace },
    NotFound,
}

impl Resolution<'_> {
    fn extend_placeholders(&self, prefix: &str, line: String) -> String {
        if line.contains("$0") {
            let mut actual = prefix.to_string();
            actual.push(' ');
            actual.push_str(&self.full_name());
            line.replace("$0", &actual)
        } else {
            line
        }
    }

    /// The resolved entry's short name (last component), if any.
    pub fn name(&self) -> Option<String> {
        match self {
            Resolution::Builtin(b) => Some(b.name.to_string()),
            Resolution::Command { command, .. } => Some(command.name()),
            Resolution::Namespace { namespace, .. } => Some(namespace.name()),
            Resolution::NotFound => None,
        }
    }

    /// The resolved entry's full name (components space-joined), if any.
    pub fn full_name(&self) -> String {
        self.components().join(" ")
    }

    /// The resolved entry's full path components.
    pub fn components(&self) -> Vec<String> {
        match self {
            Resolution::Builtin(b) => vec![b.name.to_string()],
            Resolution::Command { command, .. } => command.components.clone(),
            Resolution::Namespace { namespace, .. } => namespace.components.clone(),
            Resolution::NotFound => Vec::new(),
        }
    }

    /// The usage line, if documented. Built-in usage may contain a `<name>`
    /// placeholder for the program name.
    pub fn usage(&self, identity: &Identity) -> Option<String> {
        match self {
            Resolution::Builtin(b) => {
                Some(self.extend_placeholders(&identity.name, b.usage.to_string()))
            }
            Resolution::Command { command, .. } => command
                .front
                .usage
                .clone()
                .map(|usage| self.extend_placeholders(&identity.name, usage)),
            Resolution::Namespace { .. } | Resolution::NotFound => None,
        }
    }

    /// The one-line summary. A namespace gets a synthetic subcommand count.
    pub fn summary(&self) -> Option<String> {
        match self {
            Resolution::Builtin(b) => Some(b.summary.to_string()),
            Resolution::Command { command, .. } => command.front.summary.clone(),
            Resolution::Namespace { namespace, .. } => {
                let subs = namespace.subcommands();
                Some(format!("{} subcommands ({})", subs.len(), subs.join(", ")))
            }
            Resolution::NotFound => None,
        }
    }

    /// The long-form help text, if documented.
    pub fn help(&self, identity: &Identity) -> Option<String> {
        match self {
            Resolution::Builtin(b) => {
                Some(self.extend_placeholders(&identity.name, b.help.to_string()))
            }
            Resolution::Command { command, .. } => command
                .front
                .help
                .clone()
                .map(|help| self.extend_placeholders(&identity.name, help)),
            Resolution::Namespace { .. } | Resolution::NotFound => None,
        }
    }
}

/// The command tree, rooted at a top-level branch keyed by first path component.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Index(BTreeMap<String, Node>);

impl Index {
    /// Resolve the leading args to a built-in, an external command, a namespace,
    /// or not-found — the single external entry point for command lookup.
    /// External names may be multi-token (`db migrate`); the longest leading run
    /// of args matching a command's components wins. Built-ins are single-token
    /// and authoritative for `args[0]` unless a depth-1 external with the same
    /// name declares `override: true`.
    pub fn resolve<'a>(&'a self, args: &[String]) -> Resolution<'a> {
        let Some(first) = args.first() else {
            return Resolution::NotFound;
        };

        if let Some(builtin) = builtins::get(first) {
            match self.0.get(first) {
                Some(Node::Leaf(c)) if c.front.overrides => (),
                _ => {
                    return Resolution::Builtin(builtin);
                }
            }
        }

        match self.resolve_node(args) {
            Some((_, Node::Leaf(command))) => Resolution::Command { command },
            Some((_, Node::Branch(namespace))) => Resolution::Namespace { namespace },
            None => Resolution::NotFound,
        }
    }

    /// Greedily match the longest leading run of `args` to a node in the tree,
    /// returning `(tokens_consumed, node)`. A leaf wins as soon as it is hit; a
    /// branch wins when args run out or the next token misses inside it. Empty
    /// args or a miss on the very first token returns `None`.
    fn resolve_node<'a, S: AsRef<str>>(&'a self, args: &[S]) -> Option<(usize, &'a Node)> {
        if args.is_empty() {
            return None;
        }
        let mut branch = &self.0;
        let mut last: Option<&Node> = None;
        for (i, tok) in args.iter().enumerate() {
            match branch.get(tok.as_ref()) {
                None => return last.map(|n| (i, n)),
                Some(leaf @ Node::Leaf(_)) => return Some((i + 1, leaf)),
                Some(n @ Node::Branch(next)) => {
                    branch = &next.children;
                    last = Some(n);
                }
            }
        }
        last.map(|n| (args.len(), n))
    }

    /// Strict navigation: walk `args` exactly, rejecting trailing tokens past a
    /// leaf. (Greedy `resolve_node` filtered to require every token consumed.)
    fn node<'a, S: AsRef<str>>(&'a self, args: &[S]) -> Option<(usize, &'a Node)> {
        self.resolve_node(args).filter(|(c, _)| *c == args.len())
    }

    /// The resolution named exactly `name` (space-joined), if any.
    pub fn get(&self, name: &str) -> Resolution<'_> {
        let args: Vec<_> = name.split(' ').map(|s| s.to_string()).collect();
        self.resolve(&args)
    }

    /// Whether `name` is a namespace (a branch).
    pub fn is_namespace<S: AsRef<str>>(&self, args: &[S]) -> bool {
        match self.node(args) {
            Some((_, node)) => node.is_namespace(),
            _ => false,
        }
    }

    /// Sorted top-level command names (depth-1 leaves and namespaces).
    pub fn top_level_command_names(&self) -> Vec<String> {
        self.0.keys().cloned().collect()
    }

    /// Sorted top-level names (commands and builtins)
    pub fn top_level(&self) -> Vec<String> {
        let mut set = std::collections::BTreeSet::new();
        for doc in builtins::BUILTINS {
            set.insert(doc.name.to_string());
        }
        for entry in self.0.keys() {
            set.insert(entry.to_string());
        }
        set.into_iter().collect()
    }

    /// The top-level commands (and builtins) as resolutions, sorted by name (BTreeMap order).
    pub fn top_level_resolutions(&self) -> Vec<Resolution<'_>> {
        self.top_level()
            .into_iter()
            .map(|n| self.resolve(&[n]))
            .collect()
    }

    /// All leaf commands, sorted by full name.
    pub fn leaves(&self) -> Vec<&Command> {
        let mut out = Vec::new();
        collect_leaves(&self.0, &mut out);
        out.sort_by_key(|a| a.full_name());
        out
    }
}

#[cfg(test)]
impl Index {
    /// Build an index from leaf commands, keyed by each one's components. For
    /// tests that construct commands directly.
    pub fn from_leaves(cmds: Vec<Command>) -> Index {
        let mut root = BTreeMap::new();
        for info in cmds {
            let comps = info.components.clone();
            insert(&mut root, &comps, info);
        }
        Index(root)
    }

    pub fn get_command(&self, name: &str) -> Option<&Command> {
        let args: Vec<_> = name.split(' ').collect();
        match self.node(&args) {
            Some((_, Node::Leaf(info))) => Some(info),
            Some((_, Node::Branch(_))) => None,
            None => None,
        }
    }
}

/// Test helper: a leaf `Command` for `name` (space-joined) carrying `front`.
/// The path is synthesized — its exact value is irrelevant to the tests that
/// use this — and `is_local` is false. Shared across the crate's test modules
/// so they don't each re-spell the same boilerplate.
#[cfg(test)]
pub(crate) fn leaf(name: &str, front: FrontMatter) -> Command {
    let components: Vec<String> = name.split(' ').map(String::from).collect();
    Command {
        path: PathBuf::from(format!("/libexec/{}", name.replace(' ', "/"))),
        components,
        front,
        is_local: false,
    }
}

/// Test helper: an `Index` built from space-joined `names`, each a leaf with
/// default front-matter.
#[cfg(test)]
pub(crate) fn index_of(names: &[&str]) -> Index {
    Index::from_leaves(
        names
            .iter()
            .map(|n| leaf(n, FrontMatter::default()))
            .collect(),
    )
}

fn collect_leaves<'a>(branch: &'a BTreeMap<String, Node>, out: &mut Vec<&'a Command>) {
    for node in branch.values() {
        match node {
            Node::Leaf(info) => out.push(info),
            Node::Branch(b) => collect_leaves(&b.children, out),
        }
    }
}

/// The ancestor of `path` with its last `n` components removed.
fn ancestor(path: &Path, n: usize) -> PathBuf {
    let mut p = path.to_path_buf();
    for _ in 0..n {
        p.pop();
    }
    p
}

/// Insert a command at its path components into the tree. The first occupant of
/// a slot wins: an existing leaf or branch is never replaced, and a conflicting
/// kind (leaf where a branch is needed, or vice versa) is dropped. A branch's
/// directory path is derived from the command's path by stripping the trailing
/// components below the branch.
fn insert(branch: &mut BTreeMap<String, Node>, components: &[String], info: Command) {
    let (head, tail) = components.split_first().expect("non-empty components");
    if tail.is_empty() {
        branch.entry(head.clone()).or_insert(Node::Leaf(info));
        return;
    }
    let dir = ancestor(&info.path, tail.len());
    let components = info.components[..info.components.len() - tail.len()].to_vec();
    let child = branch.entry(head.clone()).or_insert_with(|| {
        Node::Branch(Namespace {
            components,
            path: dir,
            children: BTreeMap::new(),
        })
    });
    if let Node::Branch(next) = child {
        insert(&mut next.children, tail, info);
    }
    // else: `head` is already a leaf — conflict, drop this command.
}

/// Discover all command executables under each command root into a tree. Files
/// are indexed recursively; a command's name is its path relative to the root
/// with separators as spaces (`db/migrate` -> `"db migrate"`). The roots are
/// ordered lowest-precedence first, so we scan them in reverse — the
/// highest-precedence (last) root is scanned first and wins slot collisions
/// (the first occupant of a slot wins). A nonexistent root is silently skipped.
pub fn discover(id: &Identity) -> Index {
    let mut root: BTreeMap<String, Node> = BTreeMap::new();

    for command_root in id.command_roots.iter().rev() {
        let dir = &command_root.path;
        scan_dir(&id.name, dir, dir, command_root.is_local, &mut root);
    }

    Index(root)
}

/// Recursively scan `dir`, inserting commands with names relative to `base`.
/// `name` is the program name, used only to prefix front-matter warnings.
fn scan_dir(
    name: &str,
    base: &Path,
    dir: &Path,
    is_local: bool,
    root: &mut BTreeMap<String, Node>,
) {
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
            scan_dir(name, base, &path, is_local, root);
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
        // A malformed command's front-matter shouldn't break discovery: warn
        // (so the author hears about it) and fall back to no documentation.
        let front = match frontmatter::parse_file(&path) {
            Ok(front) => front,
            Err(err) => {
                eprintln!("{name}: {}: {err}", path.display());
                FrontMatter::default()
            }
        };
        let command = Command {
            components,
            path,
            front,
            is_local,
        };
        let comps = command.components.clone();
        insert(root, &comps, command);
    }
}

/// Replace the current process with the external command. Only returns on error.
pub fn exec_external(name: &str, path: &Path, args: &[String]) -> ! {
    let err = ProcessCommand::new(path).args(args).exec();
    // Only gets here if exec failed
    eprintln!("{name}: failed to exec {}: {err}", path.display());
    std::process::exit(126);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::CommandRoot;
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

    /// An `Identity` whose command roots mirror the historical default: the
    /// root `libexec` (the base layer) plus, when `local` is given, a local
    /// `<local>/libexec` overlay (highest precedence, listed last).
    fn id_for(root: &std::path::Path, local: Option<PathBuf>) -> Identity {
        let mut command_roots = vec![CommandRoot {
            path: root.join("libexec"),
            is_local: false,
        }];
        if let Some(local) = local {
            command_roots.push(CommandRoot {
                path: local.join("libexec"),
                is_local: true,
            });
        }
        Identity {
            name: "rush".into(),
            root: root.to_path_buf(),
            command_roots,
            config_path: PathBuf::new(),
        }
    }

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    /// The sorted child names of the namespace `path` resolves to (panics otherwise).
    fn subcommands(index: &Index, path: &[&str]) -> Vec<String> {
        match index.resolve(&args(path)) {
            Resolution::Namespace { namespace } => namespace.subcommands(),
            other => panic!("expected namespace, got {other:?}"),
        }
    }

    #[test]
    fn discover_lists_leaves_with_metadata() {
        let root = tempdir().unwrap();
        write_exec(root.path(), "who", "#!/bin/sh\n#@ summary: who\n");
        write_exec(root.path(), "where", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), None));
        let names: Vec<String> = index.leaves().iter().map(|c| c.full_name()).collect();
        assert_eq!(names, vec!["where", "who"]);
        assert_eq!(
            index.get_command("who").unwrap().front.summary.as_deref(),
            Some("who")
        );
    }

    #[test]
    fn discover_nests_subdirectories() {
        let root = tempdir().unwrap();
        write_exec(root.path(), "db/migrate", "#!/bin/sh\n");
        write_exec(root.path(), "db/seed", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), None));
        assert!(index.get_command("db migrate").is_some());
        assert!(index.is_namespace(&["db"]));
        assert_eq!(subcommands(&index, &["db"]), vec!["migrate", "seed"]);
    }

    #[test]
    fn discover_records_namespace_directory_path() {
        let root = tempdir().unwrap();
        write_exec(root.path(), "db/migrate", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), None));
        match index.resolve(&args(&["db"])) {
            Resolution::Namespace { namespace, .. } => {
                assert_eq!(namespace.path, root.path().join("libexec").join("db"));
                assert_eq!(namespace.components, vec!["db"]);
            }
            other => panic!("expected namespace, got {other:?}"),
        }
    }

    #[test]
    fn discover_skips_non_executable_and_dotfiles() {
        let root = tempdir().unwrap();
        write_exec(root.path(), "who", "#!/bin/sh\n");
        fs::write(root.path().join("libexec").join("README.md"), "notes\n").unwrap();
        write_exec(root.path(), ".hidden", "#!/bin/sh\n");
        let index = discover(&id_for(root.path(), None));
        let names: Vec<String> = index.leaves().iter().map(|c| c.full_name()).collect();
        assert_eq!(names, vec!["who"]);
    }

    #[test]
    fn discover_uses_custom_command_root() {
        // Commands live in `cmds`, not `libexec`.
        let root = tempdir().unwrap();
        let cmds = root.path().join("cmds");
        fs::create_dir_all(&cmds).unwrap();
        let who = cmds.join("who");
        fs::write(&who, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&who, fs::Permissions::from_mode(0o755)).unwrap();
        let mut id = id_for(root.path(), None);
        id.command_roots = vec![CommandRoot {
            path: cmds,
            is_local: false,
        }];
        let index = discover(&id);
        let names: Vec<String> = index.leaves().iter().map(|c| c.full_name()).collect();
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
            index.get_command("db").unwrap().front.summary.as_deref(),
            Some("local-db")
        );
        assert!(!index.is_namespace(&["db"])); // root's db/migrate was dropped
        assert!(index.get_command("db migrate").is_none());
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
    fn resolve_node_greedy_returns_deepest_leaf_and_consumed() {
        let index = index_of(&["db migrate"]);
        let (consumed, info) = index
            .resolve_node(&args(&["db", "migrate", "--force"]))
            .unwrap();
        assert_eq!(consumed, 2);
        if let Node::Leaf(info) = info {
            assert_eq!(info.full_name(), "db migrate");
        } else {
            panic!("not a node leaf")
        }
    }

    #[test]
    fn resolve_node_namespace_prefix_returns_branch() {
        let index = index_of(&["db migrate"]);
        let (consumed, node) = index.resolve_node(&["db".to_string()]).unwrap();
        assert_eq!(consumed, 1);
        let Node::Branch(ns) = node else {
            panic!("expected branch");
        };
        assert_eq!(ns.subcommands(), vec!["migrate"]);
    }

    #[test]
    fn get_children_top_level_and_namespace() {
        let index = index_of(&["who", "db migrate", "db seed", "db schema dump"]);
        assert!(index.get_command("who").is_some());
        assert!(index.get_command("db").is_none()); // a namespace, not a leaf
        assert_eq!(
            subcommands(&index, &["db"]),
            vec!["migrate", "schema", "seed"]
        );
        assert_eq!(subcommands(&index, &["db", "schema"]), vec!["dump"]);
        assert_eq!(index.top_level_command_names(), vec!["db", "who"]);
        assert!(index.is_namespace(&["db"]));
        assert!(!index.is_namespace(&["who"]));
    }

    #[test]
    fn leaves_are_sorted_by_full_name() {
        let index = index_of(&["who", "db seed", "db migrate"]);
        let names: Vec<String> = index.leaves().iter().map(|c| c.full_name()).collect();
        assert_eq!(names, vec!["db migrate", "db seed", "who"]);
    }

    // --- resolve (built-in / command / namespace dispatch) ---

    fn cmd(name: &str, overrides: bool) -> Command {
        leaf(
            name,
            FrontMatter {
                overrides,
                ..Default::default()
            },
        )
    }

    #[test]
    fn external_command_resolves_with_one_token_consumed() {
        let command = cmd("who", false);
        assert_eq!(
            Index::from_leaves(vec![command.clone()]).resolve(&args(&["who"])),
            Resolution::Command { command: &command }
        );
    }

    #[test]
    fn nested_command_consumes_its_tokens_and_passes_rest() {
        let command = cmd("db migrate", false);
        assert_eq!(
            Index::from_leaves(vec![command.clone()]).resolve(&args(&["db", "migrate", "--force"])),
            Resolution::Command { command: &command }
        );
    }

    #[test]
    fn namespace_prefix_alone_resolves_to_namespace() {
        let index = Index::from_leaves(vec![cmd("db migrate", false)]);
        match index.resolve(&args(&["db"])) {
            Resolution::Namespace { namespace } => {
                assert_eq!(namespace.subcommands(), vec!["migrate"]);
                assert_eq!(namespace.components, vec!["db"]);
            }
            other => panic!("expected namespace, got {other:?}"),
        }
    }

    #[test]
    fn unknown_command_is_not_found() {
        assert_eq!(
            Index::default().resolve(&args(&["nope"])),
            Resolution::NotFound
        );
    }

    #[test]
    fn reserved_name_resolves_to_builtin() {
        assert_eq!(
            Index::default().resolve(&args(&["help"])),
            Resolution::Builtin(builtins::get("help").unwrap())
        );
    }

    #[test]
    fn reserved_name_not_overridden_without_flag() {
        assert_eq!(
            Index::from_leaves(vec![cmd("help", false)]).resolve(&args(&["help"])),
            Resolution::Builtin(builtins::get("help").unwrap())
        );
    }

    #[test]
    fn reserved_name_overridden_with_flag() {
        let command = cmd("help", true);
        assert_eq!(
            Index::from_leaves(vec![command.clone()]).resolve(&args(&["help"])),
            Resolution::Command { command: &command }
        );
    }

    #[test]
    fn resolution_accessors_for_command() {
        let root = tempdir().unwrap();
        let id = id_for(root.path(), None);
        let index = Index::from_leaves(vec![leaf(
            "db migrate",
            FrontMatter {
                summary: Some("run it".into()),
                usage: Some("$0".into()),
                help: Some("long help".into()),
                ..Default::default()
            },
        )]);
        let res = index.resolve(&args(&["db", "migrate"]));
        assert_eq!(res.name().as_deref(), Some("migrate"));
        assert_eq!(res.components(), vec!["db", "migrate"]);
        assert_eq!(res.usage(&id).as_deref(), Some("rush db migrate"));
        assert_eq!(res.summary().as_deref(), Some("run it"));
        assert_eq!(res.help(&id).as_deref(), Some("long help"));
    }

    #[test]
    fn resolution_accessors_for_namespace() {
        let root = tempdir().unwrap();
        let index = Index::from_leaves(vec![cmd("db migrate", false), cmd("db seed", false)]);
        let res = index.resolve(&args(&["db"]));
        let id = id_for(root.path(), None);
        assert_eq!(res.name().as_deref(), Some("db"));
        assert_eq!(res.components(), vec!["db"]);
        assert_eq!(
            res.summary().as_deref(),
            Some("2 subcommands (migrate, seed)")
        );
        assert_eq!(res.usage(&id), None);
    }
}
