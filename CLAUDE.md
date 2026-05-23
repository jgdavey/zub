# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`sub` lets you build a multi-command CLI (like `git` or `rbenv`) where each subcommand is a standalone executable in any language. A "sub program" is a directory tree — `bin/<name>`, `libexec/<name>-<cmd>`, `completions/`, `share/`, plus a config file — and one shared Rust binary acts as the dispatcher and all built-ins. The binary learns *which* program it is from `argv[0]` and the config file; there is no per-program build step or source-templating.

This branch is mid-rewrite: the historical bash core (`libexec/sub*`, `bin/sub`, `prepare.sh`, `regenerate.sh`) still ships, but the Rust crate in `src/` is the active implementation. The design and task-by-task plan live in `docs/superpowers/specs/` and `docs/superpowers/plans/`.

## Commands

```bash
cargo build              # build the binary (target/debug/zub)
cargo test               # run all unit + integration tests
cargo test <name>        # run tests matching a substring, e.g. cargo test frontmatter
cargo test --test dispatch   # run only the end-to-end binary tests
cargo clippy             # lint
cargo fmt                # format
```

Use bare `cargo`/`rustc` (not `rustup run …`).

## Architecture

`main.rs` orchestrates a fixed pipeline over a set of small, independently-tested library modules (`src/lib.rs` re-exports them):

1. **identity** — derive the program `name` from `argv[0]`, resolve the program `root` (env-var fast path `_<NAME>_ROOT`, else walk up the filesystem looking for the config file), and detect a per-directory `local_root` (`$PWD/.<name>/libexec`). Produces an `Identity { name, root, local_root }`.
2. **config** — load the program's YAML config (`name`, optional `version`/`description`) from the root.
3. **env_setup** — compute and export `PATH` (local libexec → root libexec → root bin → existing PATH), `ORIG_PATH`, and `_<NAME>_ROOT` / `_<NAME>_LOCAL_ROOT` so every child process inherits them. `build_env` is pure for testability; `apply` mutates the process env.
4. **index** — discover `<name>-<cmd>` executables across libexec dirs (local scanned first, so it wins collisions), parsing each one's front-matter into a `CommandInfo`.
5. **dispatch** — `resolve` a command name to `Builtin`, `External(path)`, or `NotFound`. Built-ins are authoritative unless an external command declares `override: true` in its front-matter. External commands are run via `exec` (process replacement), so unknown args pass through untouched.

### Front-matter (`frontmatter.rs`)

Subcommands self-document via a contiguous run of comment lines beginning with a comment leader + `@` (`#@`, `//@`, `--@`, `;@`). The text after the sigil is plain YAML. The parser skips a shebang, collects marker lines until the first non-marker line, and stops — it never reads the whole script, keeping discovery fast regardless of file size. Recognized keys: `summary`, `usage`, `help`, `complete`, `override`. Unknown keys are ignored (forward-compatible). `serde` maps the YAML `override` key to the `overrides` field.

### Built-ins (`builtins/`)

`builtins/mod.rs` holds the registry (`BUILTIN_DOCS`) and `run` dispatcher; each built-in is one file (`commands`, `completions`, `help`, `init`, `new`, `source`, `scaffold`). A `Context` struct (identity + config + discovered commands) is threaded through all of them.

There are **two parallel lists of built-in names**: `BUILTINS` in `dispatch.rs` and `BUILTIN_DOCS` in `builtins/mod.rs`. A consistency test asserts they stay in sync — update both when adding/removing a built-in.

### `sh-` commands

A command named `<name>-sh-<cmd>` is a shell-eval command (its stdout is meant to be `eval`'d by the shell, enabling `cd`-like effects). The `sh-` prefix is stripped from displayed names, and the `init` script wires up the eval. See `builtins/commands.rs`, `builtins/new.rs`, and `builtins/init.rs`.

## Conventions

- **Unix-only.** Relies on `std::os::unix::process::CommandExt` for `exec`/`arg0`.
- **No CLI framework.** Argument dispatch is hand-rolled so unrecognized flags pass straight to external commands.
- **Test style.** Unit tests live inline (`#[cfg(test)] mod tests`) next to each module; pure functions (`build_env`, `parse_str`, `resolve`) are favored so logic is testable without filesystem or process effects. `tests/dispatch.rs` builds temp program trees and runs the compiled binary end-to-end via `CARGO_BIN_EXE_zub`.

## Known in-progress inconsistency

The crate is named `zub` (`Cargo.toml`, binary `target/debug/zub`) but the project/README still call it `sub`. The rename is incomplete: `identity.rs::find_root_from` searches for `zub.yml`/`zub.yaml`, while `config.rs::load` and `builtins/scaffold.rs` still read/write `sub.yml`/`sub.yaml`. These must agree on one config filename — reconcile them before relying on root resolution + config load together.
