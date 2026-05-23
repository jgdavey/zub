# Rust Core Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bash `sub` dispatcher and built-in commands with a single Rust binary that is faster, parses a new self-delimiting YAML front-matter, and preserves every existing design goal (subcommands in any language, no manifest, `sh-` commands, local subs, shell completion).

**Architecture:** One generic Rust binary acts as dispatcher + built-ins. It learns its program name from `argv[0]` and an authoritative `sub.yml`, resolves its root from a `_<NAME>_ROOT` env var (set once per shell by `init`) with a filesystem-walk fallback, discovers `<name>-<cmd>` executables in `libexec`, and either runs a built-in or `exec`s the external command. Built on a small, testable library: `frontmatter`, `config`, `identity`, `index`, `env_setup`, `dispatch`, and `builtins`.

**Tech Stack:** Rust (edition 2021), `serde` + `serde_yaml` for YAML, `tempfile` for tests. No CLI framework — dispatch is hand-rolled so unknown args pass through to external commands untouched. Unix-only (`std::os::unix::process::CommandExt::exec`).

**Phasing:** Tasks 1–9 deliver a working dispatcher that runs external commands (Phase 1, independently shippable). Tasks 10–16 add the built-ins (Phase 2).

---

## Prerequisites

- Install Rust via rustup if `cargo --version` fails: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` then restart the shell.
- All commands run from the repo root `/Users/jgdavey/src/sub`.
- The existing bash core (`libexec/sub*`, `bin/sub`, `completions/`, `prepare.sh`) stays in place until the Rust binary reaches parity; do not delete it in this plan.

## File Structure

- `Cargo.toml` — package manifest, edition 2021, deps.
- `src/lib.rs` — declares the library modules (so tests can `use sub::…`).
- `src/main.rs` — binary entry point: parse argv, build context, dispatch.
- `src/frontmatter.rs` — sigil/YAML header extraction and parsing.
- `src/config.rs` — load `sub.yml`/`sub.yaml`.
- `src/identity.rs` — program name + root + local-root resolution, env-var naming.
- `src/index.rs` — discover `<name>-<cmd>` executables and their front-matter.
- `src/env_setup.rs` — compute and apply the exported environment (PATH, ROOT vars).
- `src/dispatch.rs` — resolve a command name to built-in/external/not-found and `exec`.
- `src/builtins/mod.rs` — built-in registry, docs, and `run` dispatcher.
- `src/builtins/{commands,help,completions,source,new,init,scaffold}.rs` — one file per built-in.
- `tests/dispatch.rs` — end-to-end binary tests against temp program trees.

---

## Task 1: Cargo project scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/main.rs`

- [ ] **Step 1: Create `Cargo.toml`**

```toml
[package]
name = "sub"
version = "0.1.0"
edition = "2021"

[lib]
name = "sub"
path = "src/lib.rs"

[[bin]]
name = "sub"
path = "src/main.rs"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Create `src/lib.rs` with a trivial function and test**

```rust
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_version() {
        assert_eq!(version(), "0.1.0");
    }
}
```

- [ ] **Step 3: Create a minimal `src/main.rs`**

```rust
fn main() {
    println!("sub {}", sub::version());
}
```

- [ ] **Step 4: Run the test suite to verify the project builds**

Run: `cargo test`
Expected: PASS — `test tests::reports_version ... ok`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/main.rs
git commit -m "chore: scaffold Rust cargo project"
```

---

## Task 2: Front-matter parser

The hot-path parser. Reads contiguous sigil lines after an optional shebang, strips `leader + @ + one optional space` (preserving YAML indentation), stops at the first non-marker line, and parses the result as YAML.

**Files:**
- Create: `src/frontmatter.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

Add this line near the top of `src/lib.rs` (above the `version` fn):

```rust
pub mod frontmatter;
```

- [ ] **Step 2: Write failing tests in `src/frontmatter.rs`**

```rust
use serde::Deserialize;

#[derive(Debug, Default, PartialEq, Deserialize)]
pub struct FrontMatter {
    pub summary: Option<String>,
    pub usage: Option<String>,
    pub help: Option<String>,
    #[serde(default)]
    pub complete: bool,
    #[serde(rename = "override", default)]
    pub overrides: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars_and_flags() {
        let src = "\
#!/usr/bin/env bash
#@ summary: Check who's logged in
#@ usage: rush who
#@ complete: true

who
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("Check who's logged in"));
        assert_eq!(fm.usage.as_deref(), Some("rush who"));
        assert!(fm.complete);
        assert!(!fm.overrides);
    }

    #[test]
    fn preserves_block_scalar_indentation() {
        let src = "\
#!/usr/bin/env bash
#@ help: |
#@   line one
#@     deeper
";
        let fm = parse_str(src);
        assert_eq!(fm.help.as_deref(), Some("line one\n  deeper\n"));
    }

    #[test]
    fn stops_at_first_non_marker_line() {
        let src = "\
#!/usr/bin/env bash
#@ summary: kept
echo not part of header
#@ usage: ignored
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("kept"));
        assert_eq!(fm.usage, None);
    }

    #[test]
    fn supports_other_comment_leaders() {
        let src = "\
#!/usr/bin/env node
//@ summary: a js command
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("a js command"));
    }

    #[test]
    fn empty_or_blockless_returns_default() {
        assert_eq!(parse_str(""), FrontMatter::default());
        assert_eq!(parse_str("#!/bin/sh\necho hi\n"), FrontMatter::default());
    }

    #[test]
    fn malformed_yaml_returns_default() {
        let src = "#@ : : : not yaml\n";
        assert_eq!(parse_str(src), FrontMatter::default());
    }

    #[test]
    fn override_key_maps_to_overrides() {
        let fm = parse_str("#@ override: true\n");
        assert!(fm.overrides);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib frontmatter`
Expected: FAIL — `cannot find function 'parse_str'`.

- [ ] **Step 4: Implement the parser in `src/frontmatter.rs`**

Add above the `#[cfg(test)]` block:

```rust
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

const LEADERS: [&str; 4] = ["//", "--", "#", ";"];

/// Strip the marker (`<leader>@` plus one optional space) from a line.
/// Returns `(leader, remainder)` when the line is a marker line. When a leader
/// is already known, only that leader is accepted.
fn strip_marker(line: &str, known: &Option<String>) -> Option<(String, String)> {
    let leaders: Vec<&str> = match known {
        Some(l) => vec![l.as_str()],
        None => LEADERS.to_vec(),
    };
    for leader in leaders {
        let marker = format!("{leader}@");
        if let Some(rest) = line.strip_prefix(&marker) {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            return Some((leader.to_string(), rest.to_string()));
        }
    }
    None
}

/// Collect the YAML payload from a line iterator: skip a leading shebang, then
/// gather contiguous marker lines, stopping at the first line that is not one.
fn extract_block<I: Iterator<Item = String>>(mut lines: I) -> String {
    let mut current = lines.next();
    if let Some(first) = &current {
        if first.starts_with("#!") {
            current = lines.next();
        }
    }

    let mut block = String::new();
    let mut leader: Option<String> = None;
    while let Some(line) = current {
        match strip_marker(&line, &leader) {
            Some((found, rest)) => {
                if leader.is_none() {
                    leader = Some(found);
                }
                block.push_str(&rest);
                block.push('\n');
                current = lines.next();
            }
            None => break,
        }
    }
    block
}

fn parse_block(block: &str) -> FrontMatter {
    if block.trim().is_empty() {
        return FrontMatter::default();
    }
    serde_yaml::from_str(block).unwrap_or_default()
}

/// Parse front-matter from an in-memory string.
pub fn parse_str(source: &str) -> FrontMatter {
    let block = extract_block(source.lines().map(|l| l.to_string()));
    parse_block(&block)
}

/// Parse front-matter from a file, reading only the header region (the lazy
/// `lines()` iterator stops being polled once the block ends).
pub fn parse_file(path: &Path) -> std::io::Result<FrontMatter> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let block = extract_block(reader.lines().map_while(Result::ok));
    Ok(parse_block(&block))
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib frontmatter`
Expected: PASS — all 7 `frontmatter::tests::*` pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/frontmatter.rs
git commit -m "feat: add sigil/YAML front-matter parser"
```

---

## Task 3: Config loader

**Files:**
- Create: `src/config.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

```rust
pub mod config;
```

- [ ] **Step 2: Write failing tests in `src/config.rs`**

```rust
use serde::Deserialize;

#[derive(Debug, PartialEq, Deserialize)]
pub struct Config {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_sub_yml() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("sub.yml"), "name: rush\nversion: 0.2.0\n").unwrap();
        let cfg = load(dir.path()).unwrap();
        assert_eq!(cfg.name, "rush");
        assert_eq!(cfg.version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn accepts_yaml_extension() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("sub.yaml"), "name: tool\n").unwrap();
        assert_eq!(load(dir.path()).unwrap().name, "tool");
    }

    #[test]
    fn ignores_unknown_keys() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("sub.yml"), "name: rush\nfuture_key: 1\n").unwrap();
        assert_eq!(load(dir.path()).unwrap().name, "rush");
    }

    #[test]
    fn missing_config_returns_none() {
        let dir = tempdir().unwrap();
        assert!(load(dir.path()).is_none());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib config`
Expected: FAIL — `cannot find function 'load'`.

- [ ] **Step 4: Implement `load` in `src/config.rs`**

Add above the `#[cfg(test)]` block:

```rust
use std::fs;
use std::path::Path;

/// Load `sub.yml` (or `sub.yaml`) from `root`. Returns `None` when neither
/// exists or the file cannot be parsed.
pub fn load(root: &Path) -> Option<Config> {
    for filename in ["sub.yml", "sub.yaml"] {
        let path = root.join(filename);
        if let Ok(contents) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_yaml::from_str::<Config>(&contents) {
                return Some(cfg);
            }
        }
    }
    None
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib config`
Expected: PASS — all 4 `config::tests::*` pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/config.rs
git commit -m "feat: load sub.yml/sub.yaml config"
```

---

## Task 4: Identity — name and env-var naming

**Files:**
- Create: `src/identity.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

```rust
pub mod identity;
```

- [ ] **Step 2: Write failing tests in `src/identity.rs`**

```rust
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    pub local_root: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_from_plain_argv0() {
        assert_eq!(name_from_argv0("rush"), "rush");
    }

    #[test]
    fn name_from_path_argv0() {
        assert_eq!(name_from_argv0("/home/me/.rush/bin/rush"), "rush");
    }

    #[test]
    fn root_env_var_name_uppercases_and_substitutes() {
        assert_eq!(env_var_name("rush"), "_RUSH_ROOT");
        assert_eq!(env_var_name("my-tool"), "_MY_TOOL_ROOT");
    }

    #[test]
    fn local_env_var_name() {
        assert_eq!(env_var_name_local("rush"), "_RUSH_LOCAL_ROOT");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib identity`
Expected: FAIL — `cannot find function 'name_from_argv0'`.

- [ ] **Step 4: Implement the naming helpers in `src/identity.rs`**

Add above the `#[cfg(test)]` block:

```rust
use std::path::Path;

/// Derive the program name from the invoked path's final component.
pub fn name_from_argv0(argv0: &str) -> String {
    Path::new(argv0)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| argv0.to_string())
}

fn shout(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect()
}

/// Name of the env var holding the program root, e.g. `rush` -> `_RUSH_ROOT`.
pub fn env_var_name(name: &str) -> String {
    format!("_{}_ROOT", shout(name))
}

/// Name of the env var holding the local-sub root, e.g. `_RUSH_LOCAL_ROOT`.
pub fn env_var_name_local(name: &str) -> String {
    format!("_{}_LOCAL_ROOT", shout(name))
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib identity`
Expected: PASS — all 4 `identity::tests::*` pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/identity.rs
git commit -m "feat: derive program name and env-var names"
```

---

## Task 5: Identity — root and local-root resolution

**Files:**
- Modify: `src/identity.rs`

- [ ] **Step 1: Add failing tests to `src/identity.rs` `tests` module**

Append inside the existing `mod tests`:

```rust
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn root_from_env_fast_path() {
        let dir = tempdir().unwrap();
        std::env::set_var("_RUSHTEST_ROOT", dir.path());
        let root = resolve_root("rushtest", Path::new("rushtest")).unwrap();
        assert_eq!(root, dir.path());
        std::env::remove_var("_RUSHTEST_ROOT");
    }

    #[test]
    fn root_fallback_walks_up_from_invocation_path() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("sub.yml"), "name: walker\n").unwrap();
        let bin = dir.path().join("bin").join("walker");
        fs::write(&bin, "").unwrap();
        // env var deliberately unset for this name
        std::env::remove_var("_WALKER_ROOT");
        let root = resolve_root("walker", &bin).unwrap();
        assert_eq!(root.canonicalize().unwrap(), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn local_root_detected_when_dot_sub_libexec_exists() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".sub").join("libexec")).unwrap();
        assert_eq!(local_root_in(dir.path()), Some(dir.path().join(".sub")));
    }

    #[test]
    fn local_root_absent_otherwise() {
        let dir = tempdir().unwrap();
        assert_eq!(local_root_in(dir.path()), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib identity`
Expected: FAIL — `cannot find function 'resolve_root'`.

- [ ] **Step 3: Implement resolution in `src/identity.rs`**

Add after `env_var_name_local`:

```rust
use std::env;
use std::path::PathBuf;

/// Resolve the program root: env-var fast path, else walk up from the
/// invocation path looking for `sub.yml`/`sub.yaml`.
pub fn resolve_root(name: &str, argv0: &Path) -> Option<PathBuf> {
    if let Ok(root) = env::var(env_var_name(name)) {
        if !root.is_empty() {
            return Some(PathBuf::from(root));
        }
    }
    let start = invocation_dir(argv0, name)?;
    find_root_from(&start)
}

/// Directory containing the invoked entry. If `argv0` carries a path, use its
/// parent; otherwise search `PATH` for an entry named `name`.
fn invocation_dir(argv0: &Path, name: &str) -> Option<PathBuf> {
    if argv0.components().count() > 1 {
        return argv0.parent().and_then(|d| d.canonicalize().ok());
    }
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return dir.canonicalize().ok();
        }
    }
    None
}

fn find_root_from(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join("sub.yml").exists() || dir.join("sub.yaml").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

/// The local-sub root for a working directory: `<cwd>/.sub` when
/// `<cwd>/.sub/libexec` exists.
pub fn local_root_in(cwd: &Path) -> Option<PathBuf> {
    let dot_sub = cwd.join(".sub");
    if dot_sub.join("libexec").is_dir() {
        Some(dot_sub)
    } else {
        None
    }
}

/// Convenience wrapper using the current working directory.
pub fn local_root() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    local_root_in(&cwd)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib identity`
Expected: PASS — all 8 `identity::tests::*` pass.

- [ ] **Step 5: Commit**

```bash
git add src/identity.rs
git commit -m "feat: resolve program root and local-sub root"
```

---

## Task 6: Command index / discovery

Discovers `<name>-<cmd>` executables across local (first) then global `libexec`, dedupes by command name with local precedence, and parses each command's front-matter.

**Files:**
- Create: `src/index.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

```rust
pub mod index;
```

- [ ] **Step 2: Write failing tests in `src/index.rs`**

```rust
use crate::frontmatter::FrontMatter;
use crate::identity::Identity;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct CommandInfo {
    pub name: String,
    pub path: PathBuf,
    pub front: FrontMatter,
    pub is_local: bool,
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib index`
Expected: FAIL — `cannot find function 'discover'`.

- [ ] **Step 4: Implement discovery in `src/index.rs`**

Add above the `#[cfg(test)]` block:

```rust
use crate::frontmatter;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib index`
Expected: PASS — all 3 `index::tests::*` pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/index.rs
git commit -m "feat: discover libexec commands with front-matter"
```

---

## Task 7: Environment setup

Builds the exported environment (pure, testable) and applies it to the current process so all children inherit it.

**Files:**
- Create: `src/env_setup.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

```rust
pub mod env_setup;
```

- [ ] **Step 2: Write failing tests in `src/env_setup.rs`**

```rust
use crate::identity::Identity;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn lookup<'a>(vars: &'a [(String, String)], key: &str) -> Option<&'a str> {
        vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn exports_root_and_path_with_libexec_prepended() {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/opt/rush"),
            local_root: None,
        };
        let vars = build_env(&id, "/usr/bin:/bin");
        assert_eq!(lookup(&vars, "_RUSH_ROOT"), Some("/opt/rush"));
        assert_eq!(lookup(&vars, "ORIG_PATH"), Some("/usr/bin:/bin"));
        assert_eq!(
            lookup(&vars, "PATH"),
            Some("/opt/rush/libexec:/opt/rush/bin:/usr/bin:/bin")
        );
        assert_eq!(lookup(&vars, "_RUSH_LOCAL_ROOT"), Some(""));
    }

    #[test]
    fn local_libexec_comes_first() {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/opt/rush"),
            local_root: Some(PathBuf::from("/work/.sub")),
        };
        let vars = build_env(&id, "/bin");
        assert_eq!(
            lookup(&vars, "PATH"),
            Some("/work/.sub/libexec:/opt/rush/libexec:/opt/rush/bin:/bin")
        );
        assert_eq!(lookup(&vars, "_RUSH_LOCAL_ROOT"), Some("/work/.sub"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib env_setup`
Expected: FAIL — `cannot find function 'build_env'`.

- [ ] **Step 4: Implement in `src/env_setup.rs`**

Add above the `#[cfg(test)]` block:

```rust
use crate::identity;
use std::env;

/// Build the environment variables to export, given the current `PATH`.
/// Pure (no process mutation) so it can be unit-tested.
pub fn build_env(id: &Identity, current_path: &str) -> Vec<(String, String)> {
    let mut vars = Vec::new();
    vars.push(("ORIG_PATH".to_string(), current_path.to_string()));

    let mut parts: Vec<String> = Vec::new();
    if let Some(local) = &id.local_root {
        parts.push(local.join("libexec").to_string_lossy().into_owned());
    }
    parts.push(id.root.join("libexec").to_string_lossy().into_owned());
    parts.push(id.root.join("bin").to_string_lossy().into_owned());
    if !current_path.is_empty() {
        parts.push(current_path.to_string());
    }
    vars.push(("PATH".to_string(), parts.join(":")));

    vars.push((
        identity::env_var_name(&id.name),
        id.root.to_string_lossy().into_owned(),
    ));
    let local_val = id
        .local_root
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    vars.push((identity::env_var_name_local(&id.name), local_val));

    vars
}

/// Compute the environment from the live `PATH` and apply it to this process,
/// so every child (built-in subprocess or `exec`'d command) inherits it.
pub fn apply(id: &Identity) {
    let current_path = env::var("PATH").unwrap_or_default();
    for (key, value) in build_env(id, &current_path) {
        env::set_var(key, value);
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib env_setup`
Expected: PASS — both `env_setup::tests::*` pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/env_setup.rs
git commit -m "feat: build and apply exported environment"
```

---

## Task 8: Dispatcher resolution

Pure resolution logic (no process exec) plus the exec helper.

**Files:**
- Create: `src/dispatch.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

```rust
pub mod dispatch;
```

- [ ] **Step 2: Write failing tests in `src/dispatch.rs`**

```rust
use crate::index::CommandInfo;
use std::path::PathBuf;

/// The set of command names owned by the binary.
pub const BUILTINS: [&str; 7] =
    ["commands", "help", "completions", "init", "new", "source", "scaffold"];

#[derive(Debug, PartialEq)]
pub enum Resolution {
    Builtin(String),
    External(PathBuf),
    NotFound,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::FrontMatter;

    fn cmd(name: &str, overrides: bool) -> CommandInfo {
        CommandInfo {
            name: name.to_string(),
            path: PathBuf::from(format!("/libexec/rush-{name}")),
            front: FrontMatter { overrides, ..Default::default() },
            is_local: false,
        }
    }

    #[test]
    fn external_command_resolves_to_its_path() {
        let cmds = vec![cmd("who", false)];
        assert_eq!(resolve("who", &cmds), Resolution::External(PathBuf::from("/libexec/rush-who")));
    }

    #[test]
    fn unknown_command_is_not_found() {
        assert_eq!(resolve("nope", &[]), Resolution::NotFound);
    }

    #[test]
    fn reserved_name_resolves_to_builtin() {
        assert_eq!(resolve("help", &[]), Resolution::Builtin("help".to_string()));
    }

    #[test]
    fn reserved_name_not_overridden_without_flag() {
        let cmds = vec![cmd("help", false)];
        assert_eq!(resolve("help", &cmds), Resolution::Builtin("help".to_string()));
    }

    #[test]
    fn reserved_name_overridden_with_flag() {
        let cmds = vec![cmd("help", true)];
        assert_eq!(resolve("help", &cmds), Resolution::External(PathBuf::from("/libexec/rush-help")));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib dispatch`
Expected: FAIL — `cannot find function 'resolve'`.

- [ ] **Step 4: Implement in `src/dispatch.rs`**

Add above the `#[cfg(test)]` block:

```rust
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Resolve a command name to a built-in, an external executable, or not-found.
/// Built-ins are authoritative unless an external command with the same name
/// declares `override: true`.
pub fn resolve(command: &str, commands: &[CommandInfo]) -> Resolution {
    let external = commands.iter().find(|c| c.name == command);
    if BUILTINS.contains(&command) {
        match external {
            Some(c) if c.front.overrides => Resolution::External(c.path.clone()),
            _ => Resolution::Builtin(command.to_string()),
        }
    } else {
        match external {
            Some(c) => Resolution::External(c.path.clone()),
            None => Resolution::NotFound,
        }
    }
}

/// Replace the current process with the external command. Only returns on error.
pub fn exec_external(path: &std::path::Path, args: &[String]) -> ! {
    let err = Command::new(path).args(args).exec();
    eprintln!("sub: failed to exec {}: {err}", path.display());
    std::process::exit(126);
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib dispatch`
Expected: PASS — all 5 `dispatch::tests::*` pass.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/dispatch.rs
git commit -m "feat: command resolution and external exec"
```

---

## Task 9: Main wiring + built-ins skeleton (Phase 1 milestone)

Wires everything together so the binary dispatches external commands end-to-end. Built-ins are stubbed (real behavior in Tasks 10–16).

**Files:**
- Create: `src/builtins/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/main.rs`
- Test: `tests/dispatch.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

```rust
pub mod builtins;
```

- [ ] **Step 2: Create the built-ins registry + dispatcher stub in `src/builtins/mod.rs`**

```rust
use crate::config::Config;
use crate::identity::Identity;
use crate::index::CommandInfo;

/// Documentation for a built-in command, used by `help` and `commands`.
pub struct BuiltinDoc {
    pub name: &'static str,
    pub summary: &'static str,
    pub usage: &'static str,
    pub help: &'static str,
}

pub const BUILTIN_DOCS: &[BuiltinDoc] = &[
    BuiltinDoc { name: "commands", usage: "<name> commands", summary: "List all commands", help: "Mostly used for completion and `help`." },
    BuiltinDoc { name: "completions", usage: "<name> completions <command> [args...]", summary: "Drive subcommand completion", help: "Called by the shell completion scripts." },
    BuiltinDoc { name: "help", usage: "<name> help [<command>]", summary: "Show help for a command", help: "Run `<name> help <command>` for details." },
    BuiltinDoc { name: "init", usage: "<name> init [-]", summary: "Print shell integration", help: "Add `eval \"$(<name> init -)\"` to your shell profile." },
    BuiltinDoc { name: "new", usage: "<name> new [--local] [--sh] <command>", summary: "Generate a new command", help: "Creates a libexec script with front-matter." },
    BuiltinDoc { name: "scaffold", usage: "<name> scaffold <program>", summary: "Create a new sub program", help: "Generates a program directory with sub.yml." },
    BuiltinDoc { name: "source", usage: "<name> source <command>", summary: "Print a command's source", help: "Pages the file with bat/$PAGER/cat." },
];

/// Shared context handed to every built-in.
pub struct Context<'a> {
    pub identity: &'a Identity,
    pub config: &'a Option<Config>,
    pub commands: &'a [CommandInfo],
}

/// Run a built-in by name. Real implementations land in Tasks 10–16.
pub fn run(name: &str, args: &[String], ctx: &Context) -> i32 {
    let _ = (args, ctx);
    eprintln!("{}: built-in `{name}' not implemented yet", ctx.identity.name);
    1
}
```

- [ ] **Step 3: Write the failing end-to-end test in `tests/dispatch.rs`**

```rust
use std::fs;
use std::os::unix::fs::PermissionsExt;
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
    let bin = env!("CARGO_BIN_EXE_sub");
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
```

Note: `Command::arg0` requires `use std::os::unix::process::CommandExt;` — add it to the imports at the top of the test file:

```rust
use std::os::unix::process::CommandExt;
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `cargo test --test dispatch`
Expected: FAIL — `main.rs` still prints the version, so neither assertion holds.

- [ ] **Step 5: Implement `src/main.rs`**

Replace the entire file with:

```rust
use std::env;
use std::path::Path;
use std::process::exit;

use sub::builtins::{self, Context};
use sub::config;
use sub::dispatch::{self, Resolution};
use sub::env_setup;
use sub::identity::{self, Identity};
use sub::index;

fn main() {
    let mut argv = env::args();
    let argv0 = argv.next().unwrap_or_default();
    let rest: Vec<String> = argv.collect();

    let name = identity::name_from_argv0(&argv0);

    // First arg is the command; "", "-h", "--help" map to `help`.
    let (command, cmd_args): (String, Vec<String>) = match rest.split_first() {
        None => ("help".to_string(), Vec::new()),
        Some((first, tail)) => {
            let c = match first.as_str() {
                "-h" | "--help" => "help".to_string(),
                other => other.to_string(),
            };
            (c, tail.to_vec())
        }
    };

    let root = match identity::resolve_root(&name, Path::new(&argv0)) {
        Some(r) => r,
        None => {
            eprintln!("{name}: could not locate program root");
            exit(1);
        }
    };
    let identity = Identity { name, root, local_root: identity::local_root() };

    env_setup::apply(&identity);

    let config = config::load(&identity.root);
    let commands = index::discover(&identity);

    let ctx = Context { identity: &identity, config: &config, commands: &commands };

    match dispatch::resolve(&command, &commands) {
        Resolution::Builtin(name) => exit(builtins::run(&name, &cmd_args, &ctx)),
        Resolution::External(path) => dispatch::exec_external(&path, &cmd_args),
        Resolution::NotFound => {
            eprintln!("{}: no such command `{}'", identity.name, command);
            exit(1);
        }
    }
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --test dispatch`
Expected: PASS — both tests pass.

- [ ] **Step 7: Run the full suite**

Run: `cargo test`
Expected: PASS — all unit and integration tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/lib.rs src/main.rs src/builtins/mod.rs tests/dispatch.rs
git commit -m "feat: wire dispatcher end-to-end (Phase 1)"
```

---

## Task 10: Built-in `commands`

Lists command names (built-ins + externals), with `--sh` / `--no-sh` filters and `--complete`.

**Files:**
- Create: `src/builtins/commands.rs`
- Modify: `src/builtins/mod.rs`

- [ ] **Step 1: Declare the submodule and route to it in `src/builtins/mod.rs`**

Add at the top of the file (after the `use` lines):

```rust
pub mod commands;
```

Replace the body of `run` with:

```rust
pub fn run(name: &str, args: &[String], ctx: &Context) -> i32 {
    match name {
        "commands" => commands::run(args, ctx),
        _ => {
            eprintln!("{}: built-in `{name}' not implemented yet", ctx.identity.name);
            1
        }
    }
}
```

Add a helper used by `commands` (and later `help`/`completions`) that merges built-in names with discovered command names:

```rust
/// All command names: built-ins plus discovered externals (deduped, sorted).
pub fn all_command_names(ctx: &Context) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for doc in BUILTIN_DOCS {
        set.insert(doc.name.to_string());
    }
    for c in ctx.commands {
        set.insert(c.name.clone());
    }
    set.into_iter().collect()
}
```

- [ ] **Step 2: Write failing tests in `src/builtins/commands.rs`**

```rust
use crate::builtins::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::CommandInfo;
    use std::path::PathBuf;

    fn ctx_with(names: &[&str]) -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity { name: "rush".into(), root: PathBuf::from("/r"), local_root: None };
        let cmds = names.iter().map(|n| CommandInfo {
            name: n.to_string(),
            path: PathBuf::from(format!("/r/libexec/rush-{n}")),
            front: FrontMatter::default(),
            is_local: false,
        }).collect();
        (id, None, cmds)
    }

    #[test]
    fn lists_builtins_and_externals_sorted() {
        let (id, cfg, cmds) = ctx_with(&["who"]);
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let out = collect(&[], &ctx);
        assert!(out.contains(&"who".to_string()));
        assert!(out.contains(&"help".to_string()));
        // sorted
        let mut sorted = out.clone();
        sorted.sort();
        assert_eq!(out, sorted);
    }

    #[test]
    fn sh_filter_strips_prefix() {
        let (id, cfg, cmds) = ctx_with(&["sh-cd", "who"]);
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let out = collect(&["--sh".to_string()], &ctx);
        assert_eq!(out, vec!["cd".to_string()]);
    }

    #[test]
    fn no_sh_filter_excludes_sh_commands() {
        let (id, cfg, cmds) = ctx_with(&["sh-cd", "who"]);
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let out = collect(&["--no-sh".to_string()], &ctx);
        assert!(out.contains(&"who".to_string()));
        assert!(!out.contains(&"cd".to_string()));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib builtins::commands`
Expected: FAIL — `cannot find function 'collect'`.

- [ ] **Step 4: Implement in `src/builtins/commands.rs`**

Add above the `#[cfg(test)]` block:

```rust
use crate::builtins::all_command_names;
use std::collections::BTreeSet;

/// Build the command-name list honoring the `--sh` / `--no-sh` filters.
/// The leading `sh-` prefix is stripped from displayed names.
pub fn collect(args: &[String], ctx: &Context) -> Vec<String> {
    let mode = args.first().map(String::as_str);
    let mut out = BTreeSet::new();
    for name in all_command_names(ctx) {
        let is_sh = name.starts_with("sh-");
        match mode {
            Some("--sh") if is_sh => {
                out.insert(name.trim_start_matches("sh-").to_string());
            }
            Some("--sh") => {}
            Some("--no-sh") if is_sh => {}
            _ => {
                out.insert(name.trim_start_matches("sh-").to_string());
            }
        }
    }
    out.into_iter().collect()
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        println!("--sh");
        println!("--no-sh");
        return 0;
    }
    for name in collect(args, ctx) {
        println!("{name}");
    }
    0
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib builtins::commands`
Expected: PASS — all 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/builtins/mod.rs src/builtins/commands.rs
git commit -m "feat: add commands built-in"
```

---

## Task 11: Built-in `help`

Bare `help` prints the command table; `help <cmd>` prints usage/summary/help for a built-in or external command.

**Files:**
- Create: `src/builtins/help.rs`
- Modify: `src/builtins/mod.rs`

- [ ] **Step 1: Declare and route the submodule in `src/builtins/mod.rs`**

Add `pub mod help;` with the other `pub mod` lines, and add a match arm in `run`:

```rust
        "help" => help::run(args, ctx),
```

- [ ] **Step 2: Write failing tests in `src/builtins/help.rs`**

```rust
use crate::builtins::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::frontmatter::FrontMatter;
    use crate::identity::Identity;
    use crate::index::CommandInfo;
    use std::path::PathBuf;

    fn ctx() -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity { name: "rush".into(), root: PathBuf::from("/r"), local_root: None };
        let cmds = vec![CommandInfo {
            name: "who".into(),
            path: PathBuf::from("/r/libexec/rush-who"),
            front: FrontMatter {
                summary: Some("Check who's logged in".into()),
                usage: Some("rush who".into()),
                help: Some("Long help here.".into()),
                ..Default::default()
            },
            is_local: false,
        }];
        (id, None, cmds)
    }

    #[test]
    fn table_lists_commands_with_summaries() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let table = render_table(&ctx, 80);
        assert!(table.contains("rush <command>"));
        assert!(table.contains("who"));
        assert!(table.contains("Check who's logged in"));
    }

    #[test]
    fn detail_renders_usage_summary_help() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let detail = render_detail("who", &ctx).unwrap();
        assert!(detail.contains("Usage: rush who"));
        assert!(detail.contains("Summary: Check who's logged in"));
        assert!(detail.contains("Long help here."));
    }

    #[test]
    fn detail_for_builtin_uses_registry() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let detail = render_detail("commands", &ctx).unwrap();
        assert!(detail.contains("List all commands"));
    }

    #[test]
    fn detail_for_unknown_is_none() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        assert!(render_detail("nope", &ctx).is_none());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib builtins::help`
Expected: FAIL — `cannot find function 'render_table'`.

- [ ] **Step 4: Implement in `src/builtins/help.rs`**

Add above the `#[cfg(test)]` block:

```rust
use crate::builtins::{all_command_names, BUILTIN_DOCS};
use std::env;

struct Doc {
    summary: Option<String>,
    usage: Option<String>,
    help: Option<String>,
    is_local: bool,
}

/// Gather documentation for a command from the built-in registry or an external
/// command's front-matter.
fn doc_for(name: &str, ctx: &Context) -> Option<Doc> {
    if let Some(c) = ctx.commands.iter().find(|c| c.name == name) {
        return Some(Doc {
            summary: c.front.summary.clone(),
            usage: c.front.usage.clone(),
            help: c.front.help.clone(),
            is_local: c.is_local,
        });
    }
    if let Some(b) = BUILTIN_DOCS.iter().find(|b| b.name == name) {
        let prog = &ctx.identity.name;
        return Some(Doc {
            summary: Some(b.summary.to_string()),
            usage: Some(b.usage.replace("<name>", prog)),
            help: Some(b.help.to_string()),
            is_local: false,
        });
    }
    None
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max && max > 3 {
        format!("{}...", &s[..max - 3])
    } else {
        s.to_string()
    }
}

/// Render the command table shown by bare `help`.
pub fn render_table(ctx: &Context, columns: usize) -> String {
    let prog = &ctx.identity.name;
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut longest = 0;

    for name in all_command_names(ctx) {
        if let Some(doc) = doc_for(&name, ctx) {
            if let Some(summary) = doc.summary {
                let summary = if doc.is_local { format!("(local) {summary}") } else { summary };
                longest = longest.max(name.len());
                rows.push((name, summary));
            }
        }
    }

    let summary_width = columns.saturating_sub(longest + 5).max(10);
    let mut out = String::new();
    out.push_str(&format!("Usage: {prog} <command> [<args>]\n\n"));
    out.push_str(&format!("Some useful {prog} commands are:\n"));
    for (name, summary) in rows {
        out.push_str(&format!(
            "   {name:<longest$}  {}\n",
            truncate(&summary, summary_width)
        ));
    }
    out.push_str(&format!("\nSee '{prog} help <command>' for more information on a specific command.\n"));
    out
}

/// Render the detailed help for a single command. `None` if unknown.
pub fn render_detail(name: &str, ctx: &Context) -> Option<String> {
    let doc = doc_for(name, ctx)?;
    let usage = doc.usage?; // documented commands have a usage line
    let mut out = String::new();
    out.push_str(&format!("Usage: {usage}\n"));
    if let Some(summary) = doc.summary {
        out.push_str(&format!("Summary: {summary}\n"));
    }
    if let Some(help) = doc.help {
        if !help.trim().is_empty() {
            out.push('\n');
            out.push_str(help.trim_end());
            out.push('\n');
        }
    }
    Some(out)
}

fn terminal_columns() -> usize {
    env::var("COLUMNS").ok().and_then(|c| c.parse().ok()).unwrap_or(80)
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        for name in all_command_names(ctx) {
            if doc_for(&name, ctx).and_then(|d| d.usage).is_some() {
                println!("{name}");
            }
        }
        return 0;
    }
    match args.first() {
        None => {
            print!("{}", render_table(ctx, terminal_columns()));
            0
        }
        Some(name) => match render_detail(name, ctx) {
            Some(detail) => {
                print!("{detail}");
                0
            }
            None => {
                eprintln!("{}: no such command `{name}'", ctx.identity.name);
                1
            }
        },
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib builtins::help`
Expected: PASS — all 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/builtins/mod.rs src/builtins/help.rs
git commit -m "feat: add help built-in (table + detail)"
```

---

## Task 12: Built-in `completions`

Drives subcommand argument completion: passes through to commands that declare `complete: true`, runs built-in completion in-process, and signals generic fallback with exit 42.

**Files:**
- Create: `src/builtins/completions.rs`
- Modify: `src/builtins/mod.rs`

- [ ] **Step 1: Declare and route the submodule in `src/builtins/mod.rs`**

Add `pub mod completions;` and the arm:

```rust
        "completions" => completions::run(args, ctx),
```

- [ ] **Step 2: Write failing tests in `src/builtins/completions.rs`**

```rust
use crate::builtins::Context;

/// Completion words after the command, plus the COMP_* values they imply.
#[derive(Debug, PartialEq)]
pub struct CompWords {
    pub words: Vec<String>,
    pub last: String,
    pub penult: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_empty_word_when_comp_word_unset() {
        let cw = comp_words(&["sub".to_string()], None);
        assert_eq!(cw.words, vec!["sub".to_string(), "".to_string()]);
        assert_eq!(cw.last, "");
        assert_eq!(cw.penult.as_deref(), Some("sub"));
    }

    #[test]
    fn uses_words_as_is_when_comp_word_set() {
        let cw = comp_words(&["sub".to_string(), "fo".to_string()], Some("fo".to_string()));
        assert_eq!(cw.words, vec!["sub".to_string(), "fo".to_string()]);
        assert_eq!(cw.last, "fo");
        assert_eq!(cw.penult.as_deref(), Some("sub"));
    }

    #[test]
    fn single_word_has_no_penult() {
        let cw = comp_words(&[], Some("x".to_string()));
        // empty args + COMP_WORD set => words is just empty list
        assert_eq!(cw.penult, None);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib builtins::completions`
Expected: FAIL — `cannot find function 'comp_words'`.

- [ ] **Step 4: Implement in `src/builtins/completions.rs`**

Add above the `#[cfg(test)]` block:

```rust
use crate::builtins;
use crate::dispatch::BUILTINS;
use std::env;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Compute the completion words and the COMP_LASTARG / COMP_PENULT they imply.
/// When `COMP_WORD` is unset/empty, an empty trailing word is appended (the
/// user is starting a fresh argument).
pub fn comp_words(args: &[String], comp_word: Option<String>) -> CompWords {
    let mut words: Vec<String> = args.to_vec();
    if comp_word.as_deref().unwrap_or("").is_empty() {
        words.push(String::new());
    }
    let last = words.last().cloned().unwrap_or_default();
    let penult = if words.len() > 1 {
        Some(words[words.len() - 2].clone())
    } else {
        None
    };
    CompWords { words, last, penult }
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--commands") {
        return print_summaries(ctx);
    }

    let Some(command) = args.first() else {
        eprintln!("usage: {} completions command [arg1 arg2...]", ctx.identity.name);
        return 1;
    };
    let rest = &args[1..];

    // Built-in completion runs in-process.
    if BUILTINS.contains(&command.as_str())
        && !ctx.commands.iter().any(|c| &c.name == command && c.front.overrides)
    {
        let mut a = vec!["--complete".to_string()];
        a.extend_from_slice(rest);
        return builtins::run(command, &a, ctx);
    }

    // External command: only those declaring `complete: true` participate.
    let Some(info) = ctx.commands.iter().find(|c| &c.name == command) else {
        return 42; // unknown command -> generic fallback
    };
    if !info.front.complete {
        return 42; // not completion-capable -> generic fallback
    }

    let comp_word = env::var("COMP_WORD").ok();
    let cw = comp_words(rest, comp_word);
    env::set_var("COMP_LASTARG", &cw.last);
    env::set_var("COMP_PENULT", cw.penult.unwrap_or_default());

    let mut exec_args = vec!["--complete".to_string()];
    exec_args.extend(cw.words);
    let err = Command::new(&info.path).args(&exec_args).exec();
    eprintln!("{}: failed to exec completion: {err}", ctx.identity.name);
    1
}

/// zsh-style `name[summary]` lines for top-level command completion.
fn print_summaries(ctx: &Context) -> i32 {
    for name in builtins::all_command_names(ctx) {
        let summary = ctx
            .commands
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.front.summary.clone())
            .or_else(|| {
                builtins::BUILTIN_DOCS
                    .iter()
                    .find(|b| b.name == name)
                    .map(|b| b.summary.to_string())
            });
        match summary {
            Some(s) => println!("{name}[{s}]"),
            None => println!("{name}"),
        }
    }
    0
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib builtins::completions`
Expected: PASS — all 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/builtins/mod.rs src/builtins/completions.rs
git commit -m "feat: add completions built-in"
```

---

## Task 13: Built-in `source`

Prints a command's source through `bat`, `$PAGER`, or `cat`.

**Files:**
- Create: `src/builtins/source.rs`
- Modify: `src/builtins/mod.rs`

- [ ] **Step 1: Declare and route the submodule in `src/builtins/mod.rs`**

Add `pub mod source;` and the arm:

```rust
        "source" => source::run(args, ctx),
```

- [ ] **Step 2: Write failing tests in `src/builtins/source.rs`**

```rust
use crate::builtins::Context;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_bat_then_pager_then_cat() {
        assert_eq!(pager(Some("bat"), Some("less")), "bat");
        assert_eq!(pager(None, Some("less")), "less");
        assert_eq!(pager(None, None), "cat");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib builtins::source`
Expected: FAIL — `cannot find function 'pager'`.

- [ ] **Step 4: Implement in `src/builtins/source.rs`**

Add above the `#[cfg(test)]` block:

```rust
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Choose the pager: `bat` if available, else `$PAGER`, else `cat`.
/// `bat_path` is `Some` when `bat` is on PATH; `pager_env` is `$PAGER`.
pub fn pager(bat_path: Option<&str>, pager_env: Option<&str>) -> String {
    if bat_path.is_some() {
        "bat".to_string()
    } else if let Some(p) = pager_env.filter(|p| !p.is_empty()) {
        p.to_string()
    } else {
        "cat".to_string()
    }
}

fn which(cmd: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(cmd))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        for c in ctx.commands {
            println!("{}", c.name);
        }
        return 0;
    }
    let Some(command) = args.first() else {
        eprintln!("Please provide a command name");
        return 1;
    };
    let Some(info) = ctx.commands.iter().find(|c| &c.name == command) else {
        eprintln!("Could not find command {command}");
        return 1;
    };

    let bat = which("bat");
    let pager_env = std::env::var("PAGER").ok();
    let chosen = pager(bat.as_deref(), pager_env.as_deref());

    let err = Command::new(&chosen).arg(&info.path).exec();
    eprintln!("{}: failed to exec {chosen}: {err}", ctx.identity.name);
    1
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib builtins::source`
Expected: PASS — the `pager` test passes.

- [ ] **Step 6: Commit**

```bash
git add src/builtins/mod.rs src/builtins/source.rs
git commit -m "feat: add source built-in"
```

---

## Task 14: Built-in `new`

Generates a new subcommand script (and optional `sh-` companion) with the new sigil front-matter.

**Files:**
- Create: `src/builtins/new.rs`
- Modify: `src/builtins/mod.rs`

- [ ] **Step 1: Declare and route the submodule in `src/builtins/mod.rs`**

Add `pub mod new;` and the arm:

```rust
        "new" => new::run(args, ctx),
```

- [ ] **Step 2: Write failing tests in `src/builtins/new.rs`**

```rust
use crate::builtins::Context;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_uses_program_name_and_command() {
        let t = command_template("rush", "who");
        assert!(t.starts_with("#!/usr/bin/env bash\n"));
        assert!(t.contains("#@ usage: rush who"));
        assert!(t.contains("#@ summary:"));
    }

    #[test]
    fn parse_flags_reads_local_and_sh() {
        let opts = parse_flags(&[
            "--local".to_string(),
            "--sh".to_string(),
            "greet".to_string(),
        ]);
        assert!(opts.local);
        assert!(opts.sh);
        assert_eq!(opts.command.as_deref(), Some("greet"));
    }

    #[test]
    fn parse_flags_requires_command() {
        let opts = parse_flags(&["--local".to_string()]);
        assert_eq!(opts.command, None);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib builtins::new`
Expected: FAIL — `cannot find function 'command_template'`.

- [ ] **Step 4: Implement in `src/builtins/new.rs`**

Add above the `#[cfg(test)]` block:

```rust
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub struct Options {
    pub local: bool,
    pub sh: bool,
    pub command: Option<String>,
}

pub fn parse_flags(args: &[String]) -> Options {
    let mut opts = Options { local: false, sh: false, command: None };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-l" | "--local" => opts.local = true,
            "--sh" => opts.sh = true,
            "--" => {
                opts.command = iter.next().cloned();
                break;
            }
            other => {
                opts.command = Some(other.to_string());
                break;
            }
        }
    }
    opts
}

/// The script body for a new subcommand.
pub fn command_template(program: &str, command: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         #@ usage: {program} {command}\n\
         #@ summary: (please add docs here)\n\
         #@ help: |\n\
         #@   (add longer optional help here, that\n\
         #@   can be multi-line and include examples)\n\
         \n\
         echo \"It was generated\"\n"
    )
}

/// The body for the optional `sh-` companion.
pub fn sh_template(program: &str, command: &str) -> String {
    format!(
        "#!/usr/bin/env bash\n\
         # Call main command\n\
         output=\"$({program}-{command})\"\n\
         echo \"OUTPUT FROM MAIN COMMAND: $output\" >&2\n\
         echo \"Any output to stdout gets evaled by the shell\" >&2\n\
         \n\
         echo \"pwd\"\n"
    )
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--complete") {
        println!("--local");
        println!("--sh");
        return 0;
    }

    let opts = parse_flags(args);
    let Some(command) = opts.command else {
        eprintln!("Please provide a command name to generate");
        return 1;
    };

    let program = &ctx.identity.name;
    let base_dir: PathBuf = if opts.local {
        std::env::current_dir().unwrap_or_default().join(".sub")
    } else {
        ctx.identity.root.clone()
    };
    let libexec = base_dir.join("libexec");
    let filepath = libexec.join(format!("{program}-{command}"));

    if filepath.exists() {
        eprintln!("That command already exists");
        return 1;
    }
    if let Err(e) = fs::create_dir_all(&libexec) {
        eprintln!("{program}: could not create {}: {e}", libexec.display());
        return 1;
    }
    if let Err(e) = fs::write(&filepath, command_template(program, &command)) {
        eprintln!("{program}: could not write {}: {e}", filepath.display());
        return 1;
    }
    let _ = fs::set_permissions(&filepath, fs::Permissions::from_mode(0o755));

    if opts.sh {
        let sh_path = libexec.join(format!("{program}-sh-{command}"));
        let _ = fs::write(&sh_path, sh_template(program, &command));
        let _ = fs::set_permissions(&sh_path, fs::Permissions::from_mode(0o755));
    }

    println!("Generated {}", filepath.display());
    0
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib builtins::new`
Expected: PASS — all 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/builtins/mod.rs src/builtins/new.rs
git commit -m "feat: add new built-in with sigil front-matter template"
```

---

## Task 15: Built-in `init`

Emits the shell integration: exports `_<NAME>_ROOT`, prepends `libexec`/`bin` to PATH, loads completions, and defines the `sh-` wrapper.

**Files:**
- Create: `src/builtins/init.rs`
- Modify: `src/builtins/mod.rs`

- [ ] **Step 1: Declare and route the submodule in `src/builtins/mod.rs`**

Add `pub mod init;` and the arm:

```rust
        "init" => init::run(args, ctx),
```

- [ ] **Step 2: Write failing tests in `src/builtins/init.rs`**

```rust
use crate::builtins::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::identity::Identity;
    use crate::index::CommandInfo;
    use std::path::PathBuf;

    fn ctx() -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity { name: "rush".into(), root: PathBuf::from("/opt/rush"), local_root: None };
        (id, None, Vec::new())
    }

    #[test]
    fn exports_root_and_path() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let script = render_init(&ctx, "bash", &[]);
        assert!(script.contains("export _RUSH_ROOT=\"/opt/rush\""));
        assert!(script.contains("export PATH=\"${PATH}:/opt/rush/bin\""));
    }

    #[test]
    fn bash_emits_completion_source_and_alias() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let script = render_init(&ctx, "bash", &[]);
        assert!(script.contains("/opt/rush/completions/rush.bash"));
        assert!(script.contains("alias rush=_rush_wrapper"));
    }

    #[test]
    fn zsh_emits_fpath_and_function() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let script = render_init(&ctx, "zsh", &[]);
        assert!(script.contains("fpath=($fpath /opt/rush/completions)"));
        assert!(script.contains("rush() { _rush_wrapper $@ }"));
    }

    #[test]
    fn sh_wrapper_lists_sh_commands() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let script = render_init(&ctx, "bash", &["cd".to_string(), "push".to_string()]);
        assert!(script.contains("cd|push)"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib builtins::init`
Expected: FAIL — `cannot find function 'render_init'`.

- [ ] **Step 4: Implement in `src/builtins/init.rs`**

Add above the `#[cfg(test)]` block:

```rust
use crate::builtins::commands;
use crate::identity;

/// Render the shell-integration script. `sh_commands` are the `sh-` command
/// names (without prefix) that the wrapper should `eval`.
pub fn render_init(ctx: &Context, shell: &str, sh_commands: &[String]) -> String {
    let prog = &ctx.identity.name;
    let root = ctx.identity.root.to_string_lossy();
    let root_var = identity::env_var_name(prog);

    let mut out = String::new();
    out.push_str(&format!("export {root_var}=\"{root}\"\n"));
    out.push_str(&format!("export PATH=\"${{PATH}}:{root}/bin\"\n"));

    match shell {
        "bash" => {
            out.push_str(&format!("source \"{root}/completions/{prog}.bash\"\n"));
        }
        "zsh" => {
            out.push_str(&format!("fpath=($fpath {root}/completions)\n"));
            out.push_str(&format!("autoload -U _{prog}\n"));
            out.push_str(&format!("compdef _{prog} {prog}\n"));
        }
        _ => {}
    }

    let cases = sh_commands.join("|");
    out.push_str(&format!(
        "_{prog}_wrapper() {{\n\
         \x20 local command=\"$1\"\n\
         \x20 local evaluate=\n\
         \x20 if [ \"$#\" -gt 0 ]; then shift; fi\n\
         \x20 case \"$command\" in\n\
         \x20 {cases})\n\
         \x20   evaluate=`{prog} \"sh-$command\" \"$@\"` && eval \"${{evaluate}}\" ;;\n\
         \x20 *)\n\
         \x20   command {prog} \"$command\" \"$@\";;\n\
         \x20 esac\n\
         }}\n"
    ));

    match shell {
        "bash" => out.push_str(&format!("alias {prog}=_{prog}_wrapper\n")),
        "zsh" => out.push_str(&format!("{prog}() {{ _{prog}_wrapper $@ }}\n")),
        _ => {}
    }

    out
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    let prog = &ctx.identity.name;
    let mut iter = args.iter();
    let print = matches!(iter.next().map(String::as_str), Some("-"));
    let shell = iter
        .next()
        .cloned()
        .or_else(|| std::env::var("SHELL").ok().map(|s| {
            std::path::Path::new(&s).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or(s)
        }))
        .unwrap_or_default();

    if !print {
        let profile = match shell.as_str() {
            "bash" => "~/.bash_profile",
            "zsh" => "~/.zshrc",
            _ => "your profile",
        };
        eprintln!("# Load {prog} automatically by adding");
        eprintln!("# the following to {profile}:");
        eprintln!();
        eprintln!("eval \"$({}/bin/{prog} init -)\"", ctx.identity.root.to_string_lossy());
        eprintln!();
        return 1;
    }

    // `sh-` command names without the prefix, for the wrapper.
    let sh_commands = commands::collect(&["--sh".to_string()], ctx);
    print!("{}", render_init(ctx, &shell, &sh_commands));
    0
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib builtins::init`
Expected: PASS — all 4 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/builtins/mod.rs src/builtins/init.rs
git commit -m "feat: add init built-in (shell integration)"
```

---

## Task 16: Built-in `scaffold`

Creates a new sub *program* directory — the no-sed replacement for `prepare.sh`.

**Files:**
- Create: `src/builtins/scaffold.rs`
- Modify: `src/builtins/mod.rs`

- [ ] **Step 1: Declare and route the submodule in `src/builtins/mod.rs`**

Add `pub mod scaffold;` and the arm:

```rust
        "scaffold" => scaffold::run(args, ctx),
```

- [ ] **Step 2: Write failing tests in `src/builtins/scaffold.rs`**

```rust
use crate::builtins::Context;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::identity::Identity;
    use crate::index::CommandInfo;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    fn ctx() -> (Identity, Option<Config>, Vec<CommandInfo>) {
        let id = Identity { name: "sub".into(), root: PathBuf::from("/opt/sub"), local_root: None };
        (id, None, Vec::new())
    }

    #[test]
    fn creates_program_tree() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        let binary = Path::new("/usr/local/bin/sub");

        create_program(&ctx, &target, "rush", binary).unwrap();

        assert!(target.join("sub.yml").exists());
        assert!(target.join("libexec").is_dir());
        assert!(target.join("completions").is_dir());
        assert!(target.join("share").is_dir());
        let cfg = std::fs::read_to_string(target.join("sub.yml")).unwrap();
        assert!(cfg.contains("name: rush"));
        // bin/<name> symlink points at the binary
        let link = std::fs::read_link(target.join("bin").join("rush")).unwrap();
        assert_eq!(link, binary);
    }

    #[test]
    fn refuses_existing_directory() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context { identity: &id, config: &cfg, commands: &cmds };
        let work = tempdir().unwrap();
        let target = work.path().join("taken");
        std::fs::create_dir(&target).unwrap();
        assert!(create_program(&ctx, &target, "taken", Path::new("/bin/sub")).is_err());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib builtins::scaffold`
Expected: FAIL — `cannot find function 'create_program'`.

- [ ] **Step 4: Implement in `src/builtins/scaffold.rs`**

Add above the `#[cfg(test)]` block:

```rust
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;

/// Create a new sub program tree at `target`: `sub.yml`, `bin/<name>` symlinked
/// to `binary`, and empty `libexec`/`completions`/`share` directories.
pub fn create_program(_ctx: &Context, target: &Path, name: &str, binary: &Path) -> io::Result<()> {
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

    fs::write(
        target.join("sub.yml"),
        format!("name: {name}\nversion: 0.1.0\n"),
    )?;
    symlink(binary, target.join("bin").join(name))?;
    Ok(())
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    let Some(name) = args.first() else {
        eprintln!("usage: {} scaffold <program>", ctx.identity.name);
        return 1;
    };
    let binary = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{}: cannot locate binary: {e}", ctx.identity.name);
            return 1;
        }
    };
    let target = std::env::current_dir().unwrap_or_default().join(name);

    match create_program(ctx, &target, name, &binary) {
        Ok(()) => {
            println!("Created {} at {}", name, target.display());
            println!("Next: cd {name} && ./bin/{name} init", name = name);
            0
        }
        Err(e) => {
            eprintln!("{}: {e}", ctx.identity.name);
            1
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib builtins::scaffold`
Expected: PASS — both tests pass.

- [ ] **Step 6: Run the full suite**

Run: `cargo test`
Expected: PASS — all unit and integration tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/builtins/mod.rs src/builtins/scaffold.rs
git commit -m "feat: add scaffold built-in (replaces prepare.sh)"
```

---

## Final verification

- [ ] **Run the whole suite and a manual smoke test**

```bash
cargo test
cargo build --release
# Manual smoke test against this repo's own libexec layout:
ln -sf "$PWD/target/release/sub" /tmp/sub-smoke
_SUBSMOKE_ROOT="$PWD" /tmp/sub-smoke help
```

Expected: tests pass; `help` prints the command table including built-ins.

- [ ] **Update the README** to document `sub.yml`, the `#@` front-matter, `scaffold`, and the `complete:`/`override:` keys (replacing the magic-comment and `prepare.sh` sections). Commit separately:

```bash
git add README.md
git commit -m "docs: document Rust core, sub.yml, and #@ front-matter"
```

---

## Notes for the implementer

- **YAML crate:** `serde_yaml` 0.9 is archived but stable and widely used; if a maintained alternative is preferred later, the only touch points are `frontmatter.rs` and `config.rs`.
- **`env::set_var`:** safe on edition 2021 (used in `env_setup::apply` and `completions`). Do not bump to edition 2024 without revisiting, where it becomes `unsafe`.
- **Old magic comments:** out of scope here. A back-compat shim that reads the legacy `# Summary:` style when no `#@` block is present can be added later inside `frontmatter::parse_block`.
- **Index cache:** intentionally not built (see the design's "Future extension" section).
- **Decommissioning bash:** once parity is confirmed, a follow-up change removes `libexec/sub*`, `bin/sub`, and `prepare.sh`/`regenerate.sh`, and points `bin/<name>` at the Rust binary.
