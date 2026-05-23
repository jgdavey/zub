# Binary Split + Config-Authoritative Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split the single `zub` binary into a workhorse `zub` (invoke/dispatch) and a separate `zub-scaffold` (bootstrapping) tool, and make a per-program config file passed via `-C/--config` the single source of truth for a program's identity and root.

**Architecture:** `zub` resolves its config path from `-C/--config`, else the `ZUB_CONFIG` env var, else errors. It loads the config (`name` + optional `root`), derives `root` from the `root` field or the config file's parent directory, and derives every other path (`libexec`, `bin`, `completions`, `share`) by convention from `root`. The argv0-name derivation and filesystem walk are removed. `zub-scaffold` creates a new program tree with a self-locating `bin/<name>` shim that re-invokes `zub -C <root>/zub.yml`.

**Tech Stack:** Rust (edition 2021), `serde` + `serde_yaml`, `tempfile` for tests. Unix-only (`std::os::unix`).

---

## File Structure

- `Cargo.toml` — add a second `[[bin]]` for `zub-scaffold`.
- `src/config.rs` — `root` becomes `Option<String>`; `Option` fields skip serialization when `None`; `load_root` removed.
- `src/identity.rs` — `Identity` gains `config_path: PathBuf`; new `resolve(config_path, config)`; `name_from_argv0`/`resolve_root`/`invocation_dir`/`find_root_from` removed.
- `src/main.rs` — `zub` entry point: parse `-C/--config`/`ZUB_CONFIG`, build identity via `identity::resolve`, dispatch.
- `src/scaffold.rs` — **new** lib module holding `create_program` (no `Context`); writes `zub.yml`, the `bin/<name>` shim, and empty dirs.
- `src/bin/zub-scaffold.rs` — **new** thin `main` over `scaffold::create_program`.
- `src/lib.rs` — declare `pub mod scaffold;`.
- `src/builtins/scaffold.rs` — **deleted**.
- `src/builtins/mod.rs` — drop `scaffold` from `pub mod`, `BUILTIN_DOCS`, and `run()`.
- `src/dispatch.rs` — drop `"scaffold"` from `BUILTINS` (`[&str; 6]`).
- `src/builtins/init.rs` — the emitted `<name>()` wrapper calls `zub -C "<config_path>"`.
- `src/env_setup.rs`, `src/index.rs`, `src/builtins/commands.rs` — add `config_path` to `Identity` test literals.
- `tests/dispatch.rs` — invoke `zub -C <config>`; add `ZUB_CONFIG` fallback + override tests.

Tasks are ordered so the crate compiles and all tests pass after every task.

---

## Task 1: Make `Config.root` optional

**Files:**
- Modify: `src/config.rs`
- Modify: `src/builtins/scaffold.rs` (one-line literal fix to keep it compiling)

- [ ] **Step 1: Update the `Config` struct and remove `load_root`**

Replace the struct and the two load functions at the top of `src/config.rs` (everything from `pub struct Config` through the end of `load_root`) with:

```rust
#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub struct Config {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

use std::fs;
use std::path::Path;

/// Load a config from an explicit file path. Returns `None` when the file is
/// missing or cannot be parsed.
pub fn load(path: &Path) -> Option<Config> {
    if let Ok(contents) = fs::read_to_string(path) {
        if let Ok(cfg) = serde_yaml::from_str::<Config>(&contents) {
            return Some(cfg);
        }
    }
    None
}
```

- [ ] **Step 2: Replace the test module in `src/config.rs`**

Replace the entire `#[cfg(test)] mod tests { … }` block with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn loads_config_with_optional_root() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nroot: /opt/rush\nversion: 0.2.0\n").unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.name, "rush");
        assert_eq!(cfg.root.as_deref(), Some("/opt/rush"));
        assert_eq!(cfg.version.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn loads_config_without_root() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: tool\n").unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.name, "tool");
        assert_eq!(cfg.root, None);
    }

    #[test]
    fn ignores_unknown_keys() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nfuture_key: 1\n").unwrap();
        assert_eq!(load(&path).unwrap().name, "rush");
    }

    #[test]
    fn missing_config_returns_none() {
        let dir = tempdir().unwrap();
        assert!(load(&dir.path().join("nope.yml")).is_none());
    }

    #[test]
    fn omits_none_fields_when_serialized() {
        let cfg = Config {
            name: "rush".into(),
            root: None,
            version: None,
            description: None,
        };
        let yaml = serde_yaml::to_string(&cfg).unwrap();
        assert!(yaml.contains("name: rush"));
        assert!(!yaml.contains("root"));
        assert!(!yaml.contains("version"));
    }
}
```

- [ ] **Step 3: Fix the `Config` literal in `src/builtins/scaffold.rs`**

In `create_program`, change the `root` field so it compiles against the new `Option<String>` type. Replace:

```rust
        root: String::from(root),
```

with:

```rust
        root: Some(String::from(root)),
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS (all existing + new config tests).

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/builtins/scaffold.rs
git commit -m "refactor(config): make root optional, drop load_root

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 2: Add `config_path` to `Identity` and fix all literals

This is a mechanical, cross-cutting change: add one field, then add it to every `Identity { … }` literal so the crate compiles.

**Files:**
- Modify: `src/identity.rs`
- Modify: `src/env_setup.rs`
- Modify: `src/index.rs`
- Modify: `src/builtins/commands.rs`
- Modify: `src/builtins/init.rs`
- Modify: `src/builtins/scaffold.rs`

- [ ] **Step 1: Add the field to the struct**

In `src/identity.rs`, replace:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    pub local_root: Option<PathBuf>,
}
```

with:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    pub local_root: Option<PathBuf>,
    pub config_path: PathBuf,
}
```

- [ ] **Step 2: Run the build to find every broken literal**

Run: `cargo build 2>&1 | grep "missing field"`
Expected: errors in `env_setup.rs`, `index.rs`, `builtins/commands.rs`, `builtins/init.rs`, `builtins/scaffold.rs`.

- [ ] **Step 3: Add `config_path` to each `Identity` literal**

These literals live in test helpers and don't exercise `config_path`, so use `PathBuf::new()` — except `init.rs`, where Task 9 asserts on it (use a real path there).

In `src/env_setup.rs`, both `Identity { … }` literals in the test module — add as the last field:
```rust
            config_path: PathBuf::new(),
```

In `src/index.rs`, all three `Identity { … }` literals in the test module — add:
```rust
            config_path: PathBuf::new(),
```

In `src/builtins/commands.rs`, the `ctx_with` helper literal — add:
```rust
            config_path: PathBuf::new(),
```

In `src/builtins/scaffold.rs`, the `ctx()` helper literal — add:
```rust
            config_path: PathBuf::new(),
```

In `src/builtins/init.rs`, the `ctx()` helper literal — add a real path:
```rust
            config_path: PathBuf::from("/opt/rush/zub.yml"),
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: PASS (no behavior change; all literals now compile).

- [ ] **Step 5: Commit**

```bash
git add src/identity.rs src/env_setup.rs src/index.rs src/builtins/commands.rs src/builtins/init.rs src/builtins/scaffold.rs
git commit -m "refactor(identity): add config_path field to Identity

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 3: Add `identity::resolve`

Config-authoritative resolution: given a config path and its loaded `Config`, produce an `Identity`. Added alongside the old functions (removed in Task 5) so the build stays green.

**Files:**
- Modify: `src/identity.rs`

- [ ] **Step 1: Write the failing tests**

Add these two tests inside the existing `#[cfg(test)] mod tests { … }` in `src/identity.rs` (the module already has `use std::fs;` and `use tempfile::tempdir;`):

```rust
    #[test]
    fn resolve_uses_root_field_when_set() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: rush\nroot: /opt/rush\n").unwrap();
        let cfg = crate::config::load(&path).unwrap();
        let id = resolve(&path, &cfg).unwrap();
        assert_eq!(id.name, "rush");
        assert_eq!(id.root, PathBuf::from("/opt/rush"));
        assert_eq!(id.config_path, path.canonicalize().unwrap());
    }

    #[test]
    fn resolve_falls_back_to_config_parent_dir() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("zub.yml");
        fs::write(&path, "name: walker\n").unwrap();
        let cfg = crate::config::load(&path).unwrap();
        let id = resolve(&path, &cfg).unwrap();
        assert_eq!(id.root, dir.path().canonicalize().unwrap());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test resolve_uses_root_field_when_set`
Expected: FAIL to compile — `cannot find function resolve`.

- [ ] **Step 3: Add the `Config` import and the `resolve` function**

At the top of `src/identity.rs`, after `use std::path::{Path, PathBuf};`, add:

```rust
use crate::config::Config;
```

Then add this function (place it just above the `Identity` struct definition):

```rust
/// Build an `Identity` from a config file path and its loaded config.
/// `root` comes from the config's `root` field when set & non-empty, otherwise
/// from the config file's parent directory. The config path is canonicalized.
pub fn resolve(config_path: &Path, config: &Config) -> Option<Identity> {
    let canon = config_path.canonicalize().ok()?;
    let root = match config.root.as_deref() {
        Some(r) if !r.is_empty() => PathBuf::from(r),
        _ => canon.parent()?.to_path_buf(),
    };
    Some(Identity {
        name: config.name.clone(),
        root,
        local_root: local_root(&config.name),
        config_path: canon,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test resolve_`
Expected: PASS (both `resolve_*` tests).

- [ ] **Step 5: Commit**

```bash
git add src/identity.rs
git commit -m "feat(identity): add config-authoritative resolve()

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 4: Rewrite `zub` entry point (`src/main.rs`)

Parse the global config flag, fall back to `ZUB_CONFIG`, build identity via `identity::resolve`, and drop the argv0-name path and the `scaffold` root special-case.

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Replace the entire contents of `src/main.rs`**

```rust
use std::env;
use std::path::PathBuf;
use std::process::exit;

use zub::builtins::{self, Context};
use zub::config;
use zub::dispatch::{self, Resolution};
use zub::env_setup;
use zub::identity;
use zub::index;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    // Global config selector: `-C/--config <path>`, else `ZUB_CONFIG`, else error.
    let (config_path, rest): (Option<PathBuf>, Vec<String>) = match args.split_first() {
        Some((flag, tail)) if flag == "-C" || flag == "--config" => match tail.split_first() {
            Some((path, more)) => (Some(PathBuf::from(path)), more.to_vec()),
            None => {
                eprintln!("zub: {flag} requires a path");
                exit(2);
            }
        },
        _ => match env::var_os("ZUB_CONFIG") {
            Some(p) => (Some(PathBuf::from(p)), args.clone()),
            None => (None, args.clone()),
        },
    };

    let Some(config_path) = config_path else {
        eprintln!("zub: no config; pass -C <path> or set ZUB_CONFIG");
        exit(2);
    };

    let Some(config) = config::load(&config_path) else {
        eprintln!("zub: could not load config at {}", config_path.display());
        exit(1);
    };

    let Some(identity) = identity::resolve(&config_path, &config) else {
        eprintln!(
            "zub: could not resolve program root from {}",
            config_path.display()
        );
        exit(1);
    };

    // First arg is the command; "", "-h", "--help" map to `help`.
    let (command, cmd_args): (String, Vec<String>) = match rest.split_first() {
        None => ("help".to_string(), Vec::new()),
        Some((first, tail)) => {
            let c = match first.as_str() {
                "" | "-h" | "--help" => "help".to_string(),
                other => other.to_string(),
            };
            (c, tail.to_vec())
        }
    };

    env_setup::apply(&identity);

    let commands = index::discover(&identity);
    let config = Some(config);

    let ctx = Context {
        identity: &identity,
        config: &config,
        commands: &commands,
    };

    match dispatch::resolve(&command, &commands) {
        Resolution::Builtin(name) => exit(builtins::run(&name, &cmd_args, &ctx)),
        Resolution::External(path) => dispatch::exec_external(&identity.name, &path, &cmd_args),
        Resolution::NotFound => {
            eprintln!("{}: no such command `{}'", identity.name, command);
            exit(1);
        }
    }
}
```

- [ ] **Step 2: Build**

Run: `cargo build`
Expected: compiles. `name_from_argv0`/`resolve_root`/`invocation_dir`/`find_root_from` are now unused by `main` but still referenced by their own tests, so no dead-code error yet.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib`
Expected: PASS. (`tests/dispatch.rs` integration tests still use the old invocation and will be updated in Task 10 — they may fail here; run `--lib` to scope to unit tests for this step.)

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "refactor(zub): resolve identity from -C/--config (or ZUB_CONFIG)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 5: Remove dead resolution functions

**Files:**
- Modify: `src/identity.rs`

- [ ] **Step 1: Delete the four unused functions**

In `src/identity.rs`, delete these functions entirely: `name_from_argv0`, `resolve_root`, `invocation_dir`, and `find_root_from`. Keep `shout`, `env_var_name`, `env_var_name_local`, `local_root_in`, `local_root`, `resolve`, and the `Identity` struct.

- [ ] **Step 2: Delete their tests**

In the `#[cfg(test)] mod tests` block, delete the tests `name_from_plain_argv0`, `name_from_path_argv0`, `root_from_env_fast_path`, and `root_fallback_walks_up_from_invocation_path`. Keep `root_env_var_name_uppercases_and_substitutes`, `local_env_var_name`, the two `local_root_*` tests, and the two `resolve_*` tests added in Task 3.

- [ ] **Step 3: Build and check for unused imports**

Run: `cargo build 2>&1 | grep -E "unused|warning: unused"`
Expected: no unused-import warnings. (`use std::env;` is still used by `local_root`; `Path`/`PathBuf` still used.) If anything is flagged, remove that specific unused import.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/identity.rs
git commit -m "refactor(identity): remove argv0/walk resolution (dead code)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 6: Create the `scaffold` lib module

Move program-tree creation into a `Context`-free lib module that also writes the self-locating `bin/<name>` shim and omits `root` from `zub.yml`.

**Files:**
- Create: `src/scaffold.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create `src/scaffold.rs` with failing tests**

```rust
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
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let shim = "#!/bin/sh\n\
                here=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
                exec zub -C \"$here/zub.yml\" \"$@\"\n";
    let shim_path = target.join("bin").join(name);
    fs::write(&shim_path, shim)?;
    fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755))?;

    Ok(())
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
    fn refuses_existing_directory() {
        let work = tempdir().unwrap();
        let target = work.path().join("taken");
        fs::create_dir(&target).unwrap();
        assert!(create_program(&target, "taken").is_err());
    }
}
```

- [ ] **Step 2: Declare the module in `src/lib.rs`**

In `src/lib.rs`, add to the module list (alphabetical, after `pub mod index;`):

```rust
pub mod scaffold;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib scaffold`
Expected: PASS (all three `scaffold::tests`).

- [ ] **Step 4: Commit**

```bash
git add src/scaffold.rs src/lib.rs
git commit -m "feat(scaffold): add Context-free create_program lib module

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 7: Add the `zub-scaffold` binary

**Files:**
- Create: `src/bin/zub-scaffold.rs`
- Modify: `Cargo.toml`

- [ ] **Step 1: Create `src/bin/zub-scaffold.rs`**

```rust
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
```

- [ ] **Step 2: Register the binary in `Cargo.toml`**

After the existing `[[bin]]` block for `zub` (the one ending at the line with `path = "src/main.rs"`), add:

```toml
[[bin]]
name = "zub-scaffold"
path = "src/bin/zub-scaffold.rs"
```

- [ ] **Step 3: Build both binaries**

Run: `cargo build --bins`
Expected: builds `zub` and `zub-scaffold`.

- [ ] **Step 4: Smoke-test scaffolding**

Run:
```bash
cd "$(mktemp -d)" && "$OLDPWD/target/debug/zub-scaffold" demo && cat demo/zub.yml && cat demo/bin/demo && cd "$OLDPWD"
```
Expected: prints a `zub.yml` containing `name: demo` (no `root:` line) and a `bin/demo` shim containing `exec zub -C "$here/zub.yml" "$@"`.

- [ ] **Step 5: Commit**

```bash
git add src/bin/zub-scaffold.rs Cargo.toml
git commit -m "feat: add zub-scaffold binary

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 8: Remove the `scaffold` built-in

**Files:**
- Delete: `src/builtins/scaffold.rs`
- Modify: `src/builtins/mod.rs`
- Modify: `src/dispatch.rs`

- [ ] **Step 1: Delete the built-in file**

```bash
git rm src/builtins/scaffold.rs
```

- [ ] **Step 2: Remove `scaffold` from `src/builtins/mod.rs`**

Delete the `pub mod scaffold;` line. Delete the entire `BuiltinDoc { name: "scaffold", … },` entry from `BUILTIN_DOCS`. Delete the `"scaffold" => scaffold::run(args, ctx),` arm from the `run()` match.

- [ ] **Step 3: Remove `scaffold` from `BUILTINS` in `src/dispatch.rs`**

Replace:

```rust
pub const BUILTINS: [&str; 7] = [
    "commands",
    "help",
    "completions",
    "init",
    "new",
    "source",
    "scaffold",
];
```

with:

```rust
pub const BUILTINS: [&str; 6] = [
    "commands",
    "help",
    "completions",
    "init",
    "new",
    "source",
];
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib`
Expected: PASS, including `builtins_and_docs_cover_the_same_names` (now 6 names on each side).

- [ ] **Step 5: Commit**

```bash
git add -A src/builtins/mod.rs src/builtins/scaffold.rs src/dispatch.rs
git commit -m "refactor: drop scaffold built-in (now zub-scaffold)

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 9: `init` emits `zub -C <config_path>`

The emitted `<name>()` wrapper must call `zub -C "<config_path>"` directly (keeping `-C` explicit so multiple zub programs in one shell never collide).

**Files:**
- Modify: `src/builtins/init.rs`

- [ ] **Step 1: Write the failing test**

Add this test inside the `#[cfg(test)] mod tests` block in `src/builtins/init.rs`:

```rust
    #[test]
    fn wrapper_invokes_zub_with_config() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            commands: &cmds,
        };
        let script = render_init(&ctx, "bash", &["cd".to_string()]);
        assert!(script.contains("zub -C \"/opt/rush/zub.yml\""));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test wrapper_invokes_zub_with_config`
Expected: FAIL — the wrapper currently calls `{prog}`, not `zub -C`.

- [ ] **Step 3: Update `render_init`**

In `src/builtins/init.rs`, near the top of `render_init` (after `let root_var = identity::env_var_name(prog);`), add:

```rust
    let config = ctx.identity.config_path.to_string_lossy();
```

Then replace the wrapper-emitting `out.push_str(&format!( … ))` block with:

```rust
    out.push_str(&format!(
        "_{prog}_wrapper() {{\n\
         \x20 local command=\"$1\"\n\
         \x20 local evaluate=\n\
         \x20 if [ \"$#\" -gt 0 ]; then shift; fi\n\
         \x20 case \"$command\" in\n\
         \x20 {cases})\n\
         \x20   evaluate=`zub -C \"{config}\" \"sh-$command\" \"$@\"` && eval \"${{evaluate}}\" ;;\n\
         \x20 *)\n\
         \x20   zub -C \"{config}\" \"$command\" \"$@\";;\n\
         \x20 esac\n\
         }}\n"
    ));
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib init`
Expected: PASS (the new test plus the existing `init` tests; `sh_wrapper_lists_sh_commands` still passes since `{cases}` is unchanged).

- [ ] **Step 5: Commit**

```bash
git add src/builtins/init.rs
git commit -m "feat(init): wrapper calls zub -C <config> directly

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Task 10: Update integration tests for `-C` invocation

**Files:**
- Modify: `tests/dispatch.rs`

- [ ] **Step 1: Replace the contents of `tests/dispatch.rs`**

```rust
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
    let out = Command::new(bin).arg("hi").env_remove("ZUB_CONFIG").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no config"));
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
```

- [ ] **Step 2: Run the full test suite**

Run: `cargo test`
Expected: PASS — all unit and integration tests.

- [ ] **Step 3: Final formatting and lint check**

Run: `cargo fmt && cargo clippy --all-targets`
Expected: no formatting diff committed separately if clean; no clippy errors. If `cargo fmt` changed anything, include it in the commit below.

- [ ] **Step 4: Commit**

```bash
git add tests/dispatch.rs
git commit -m "test: drive zub via -C/--config and ZUB_CONFIG

Co-Authored-By: Claude Opus 4.7 <noreply@anthropic.com>"
```

---

## Self-Review Notes

- **Spec coverage:** crate layout (Tasks 6,7,8), config-authoritative resolution + `-C`/`ZUB_CONFIG` precedence (Tasks 1,3,4), `root` field-or-parent-dir (Task 3), removed dead resolution + dropped argv0 symlink (Task 5), `Identity.config_path` (Task 2), `zub-scaffold` output incl. self-locating shim with `root` omitted (Tasks 6,7), `init` deriving from config + explicit `-C` (Task 9), env_setup unchanged (no task needed), testing surface (Tasks 1,3,6,9,10 + consistency test in Task 8). All spec sections map to a task.
- **Type consistency:** `create_program(&Path, &str)` signature is consistent across Tasks 6 and 7; `identity::resolve(&Path, &Config)` consistent across Tasks 3 and 4; `Config.root: Option<String>` consistent across Tasks 1, 3, 6.
- **Note on `local_root`:** unchanged (cwd-based `.<name>/libexec`); still imported/used in `identity::resolve` and exercised by existing tests.
