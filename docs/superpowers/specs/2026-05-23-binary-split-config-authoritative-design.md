# Splitting zub into two binaries with config-authoritative resolution — Design

**Date:** 2026-05-23
**Status:** Approved design, pending implementation plan

## Goal

Split the single `zub` binary into two, and make a per-program config file the
single source of truth for a program's identity and root:

- **`zub`** — the workhorse/invoke binary. Dispatches subcommands, runs
  built-ins (`commands`, `completions`, `help`, `init`, `new`, `source`). Every
  scaffolded program's `bin/<name>` points at it.
- **`zub-scaffold`** — the human-facing bootstrapping tool that creates new zub
  program trees. No longer a built-in.

`zub` takes a global `-C/--config <path>` option naming the config file for a
given program instance. Identity (name, root) and every derived location
(`libexec`, `bin`, `completions`, `share`) come from that config, by convention
from the root. This is especially important for `init`, whose emitted shell
integration (PATH munging, completion locations, the `<name>()` wrapper) all
derive from the config.

## Background: current resolution

Today `zub` figures out which program it is from three sources: the name it was
invoked as (`argv[0]`), the `_<NAME>_ROOT` env var, and a filesystem walk
upward looking for `zub.yml`/`zub.yaml`. `scaffold` is a built-in. This design
replaces that with config-authoritative resolution and removes the argv0-name
derivation and the filesystem walk entirely.

## Architecture overview

```
program-root/
├── zub.yml                 # name (+ optional root, version, description)
├── bin/<name>              # self-locating shim: exec zub -C <root>/zub.yml "$@"
├── libexec/<name>-<cmd>    # subcommand executables, any language
├── completions/            # shipped shell-completion shims
└── share/                  # static data, exposed via _<NAME>_ROOT
```

Two `[[bin]]` targets share the existing library crate:

- `zub` → `src/main.rs`
- `zub-scaffold` → `src/bin/zub-scaffold.rs`

## Components

### Crate layout

- `src/scaffold.rs` (**new lib module**) — holds `create_program`, moved out of
  `builtins/scaffold.rs`. Takes plain arguments (target dir, name), no
  `Context`, so both `zub-scaffold` and tests can call it directly.
- `src/bin/zub-scaffold.rs` (**new**) — thin `main` over `scaffold::create_program`:
  parse the program name argument, resolve the target dir from cwd, print
  next-step guidance.
- `src/builtins/scaffold.rs` — **deleted**.
- `src/builtins/mod.rs` — remove `scaffold` from `BUILTIN_DOCS`, the `run()`
  match, and the `pub mod scaffold;` declaration.
- `src/dispatch.rs` — remove `"scaffold"` from `BUILTINS` (now `[&str; 6]`).

### Config (`src/config.rs`)

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
```

`skip_serializing_if` keeps `zub-scaffold` output clean: omitted `Option`
fields don't appear as `root: null` in the generated `zub.yml`.

- `root` is **optional**. When set and non-empty, it is authoritative. When
  absent, root is derived from the config file's parent directory (see
  resolution below). `zub-scaffold` omits `root` so generated programs are
  portable by default; the field remains for manual override.
- `config::load(path)` loads a config from an explicit file path (used by `zub`
  with the `-C` path).
- `config::load_root(root)` (search `zub.yml`/`zub.yaml` in a directory) is no
  longer on the hot path; remove it unless a remaining caller needs it.

### Identity (`src/identity.rs`)

`Identity` gains the config path so `init` can embed the literal `-C <path>`:

```rust
pub struct Identity {
    pub name: String,
    pub root: PathBuf,
    pub local_root: Option<PathBuf>,
    pub config_path: PathBuf,
}
```

**Removed (dead under config-authoritative):** `name_from_argv0`,
`resolve_root`, `invocation_dir`, `find_root_from`.

**Kept:** `shout`, `env_var_name`, `env_var_name_local` (env exports still need
them); `local_root` / `local_root_in` (cwd-based `.<name>/libexec` detection is
orthogonal to config and unchanged).

**New resolution** (in `identity` or a small step in `main`):
1. Determine the config path: `-C/--config <path>` flag, else `ZUB_CONFIG`
   env var, else error with usage.
2. `config::load(config_path)` → `Config`.
3. `name` = `config.name`.
4. `root` = `config.root` when set & non-empty, else
   `config_path.parent()` canonicalized.
5. `local_root` = `local_root(name)` (cwd `.<name>/libexec` if present).
6. `config_path` = the resolved (canonicalized) config path.

### Argument grammar (`src/main.rs` for `zub`)

```
zub [-C <path> | --config <path>] <command> [args…]
```

- Config path precedence: `-C/--config` flag > `ZUB_CONFIG` env > error.
- The flag is parsed before the command; everything after the command name
  passes through to the subcommand untouched (hand-rolled, no CLI framework).
- `""`, `-h`, `--help` as the command still map to `help`.
- With no resolvable config, `zub` prints a usage message and exits non-zero.

Pipeline after resolution is unchanged: `env_setup::apply` → `index::discover`
→ `dispatch::resolve` → run built-in or `exec` external.

### Env setup (`src/env_setup.rs`)

Unchanged. `build_env` already derives `PATH` (local libexec → root libexec →
root bin → existing PATH) and exports `_<NAME>_ROOT` / `_<NAME>_LOCAL_ROOT` from
`Identity`. With root now sourced from config, no code change is required.

### `zub-scaffold` output

`zub-scaffold <name>` creates `<name>/` (refusing an existing directory):

- `zub.yml` — serialized `Config { name, root: None, version: None,
  description: Some("your description") }` (i.e. `root` omitted).
- `bin/<name>` — executable (`0o755`) self-locating shim:
  ```sh
  #!/bin/sh
  here="$(cd "$(dirname "$0")/.." && pwd)"
  exec zub -C "$here/zub.yml" "$@"
  ```
- empty `libexec/`, `completions/`, `share/`.
- Prints next steps, noting `zub` must be on `PATH`.

### `init` built-in (`src/builtins/init.rs`)

`render_init` derives everything from `ctx.identity`:

- export `_<NAME>_ROOT="<root>"` and prepend `<root>/bin` to `PATH` (unchanged;
  root now from config).
- completions wired from `<root>/completions` (bash `source`, zsh `fpath`) —
  unchanged convention.
- define the `<name>()` shell function (and bash alias) that calls
  **`zub -C "<config_path>" …`** directly, using `identity.config_path`, with
  the existing `sh-` eval handling. The function keeps `-C` explicit (no
  reliance on `ZUB_CONFIG`), so multiple zub programs in one shell never
  collide.

The physical `bin/<name>` shim coexists for non-sourced use (cron, other
scripts, tools that exec the program directly).

## Data flow (typical invocation)

1. User types `foo bar baz` → shell function `foo()` (from `init`) runs
   `zub -C /path/foo/zub.yml bar baz`.
2. `zub` resolves config path from `-C`, loads `zub.yml`, builds `Identity`
   (name `foo`, root `/path/foo` via parent-dir fallback).
3. `env_setup::apply` exports `PATH`/`_FOO_ROOT`/`_FOO_LOCAL_ROOT`.
4. `index::discover` finds `foo-*` in local + root `libexec`.
5. `dispatch::resolve("bar", …)` → built-in or `exec /path/foo/libexec/foo-bar`.

## Error handling

- No resolvable config (`-C` and `ZUB_CONFIG` both absent) → usage message,
  non-zero exit.
- `-C` path missing/unparseable → error naming the path, non-zero exit.
- `zub-scaffold` into an existing directory → `AlreadyExists` error (current
  behavior).
- Unknown subcommand → `no such command` (current behavior).

## Testing

- `tests/dispatch.rs` — invoke `zub -C <root>/zub.yml hi` (drop `arg0` /
  `_<NAME>_ROOT` reliance). Add a case asserting `ZUB_CONFIG` fallback works and
  that `-C` overrides it.
- `tests/scaffold.rs` (**new**) or unit tests in `src/scaffold.rs` — assert tree
  creation, `zub.yml` contents (no `root` key), and that `bin/<name>` exists, is
  executable, and contains the self-locating shim.
- `src/config.rs` tests — `root` optional: loads with and without the field.
- `src/identity.rs` tests — resolution: `root` field wins when set; parent-dir
  fallback when absent; `config_path` populated. Remove tests for deleted
  `name_from_argv0` / `resolve_root` / walk.
- `src/builtins/init.rs` tests — emitted function contains
  `zub -C "<config_path>"`; completions/PATH derive from root.
- `src/builtins/mod.rs` consistency test — `BUILTINS` ↔ `BUILTIN_DOCS` now cover
  the same 6 names.
- Adding `config_path` to `Identity` breaks every `Identity { … }` literal in
  test helpers (`env_setup.rs`, `init.rs`, `commands.rs`, `dispatch.rs`, the
  former `scaffold.rs`, `identity.rs`). All must add the new field — a
  mechanical but cross-cutting change.

## Out of scope (YAGNI)

- Explicit per-path overrides in config (libexec/completions/bin/share) — paths
  stay convention-from-root.
- Renamed-symlink invocation (was provided by `name_from_argv0`) — intentionally
  dropped under config-authoritative resolution.
- Migrating the legacy bash core (`libexec/zub*` etc.); it is untouched here.
