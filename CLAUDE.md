# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`zub` lets you build a multi-command CLI (like `git` or `rbenv`) where each subcommand is a standalone executable in any language. A "zub program" is a directory tree — `bin/<name>`, `libexec/` (one executable per command; subdirectories become git-style nested subcommands), `completions/`, `share/`, plus a `zub.yml` config — and one shared Rust binary acts as the dispatcher and all built-ins. The binary is told *which* program it is by a config-file path (`-C/--config <path>`, else `$ZUB_CONFIG`); the config is the single source of truth for `name` and `root`. There is no per-program build step or source-templating.

The Rust crate in `src/` is the implementation; the historical bash core has been removed. It builds two binaries: `zub` (dispatch + built-ins) and `zub-scaffold` (bootstraps a new program tree). The only non-Rust artifact is `migrate-frontmatter` (a one-shot Python converter, documented below). The design and task-by-task plans live in `docs/superpowers/specs/` and `docs/superpowers/plans/`.

## Commands

```bash
cargo build              # build both binaries (target/debug/{zub,zub-scaffold})
cargo test               # run all unit + integration tests
cargo test <name>        # run tests matching a substring, e.g. cargo test frontmatter
cargo test --test dispatch   # run only the end-to-end binary tests
cargo clippy             # lint
cargo fmt                # format
```

Use bare `cargo`/`rustc` (not `rustup run …`).

## Architecture

`main.rs` orchestrates a fixed pipeline over a set of small, independently-tested library modules (`src/lib.rs` re-exports them):

1. **config** — `main.rs` resolves the config path (`-C/--config`, else `$ZUB_CONFIG`, else error) and loads the YAML (`name`, optional `root`/`version`/`description`).
2. **identity** — `resolve(config_path, config)` derives `name` from the config, `root` from the config's `root` field (when set) else the config file's parent dir, and detects a per-directory `local_root` (`$PWD/.<name>/libexec`). Produces an `Identity { name, root, local_root, config_path }`.
3. **env_setup** — compute and export `PATH` (local libexec → root libexec → root bin → existing PATH), `ORIG_PATH`, and `_<NAME>_ROOT` / `_<NAME>_LOCAL_ROOT` so every child process inherits them. `build_env` is pure for testability; `apply` mutates the process env.
4. **index** — recursively discover executable files across libexec dirs (local scanned first, so it wins collisions; dotfiles and non-executables skipped) into a tree of `Node::Leaf(Command)` / `Node::Branch(Namespace)`, where a `Namespace` holds its `name`, `components`, directory `path`, and child nodes (just as a `Command` is held directly in a leaf). A command's name is its path relative to libexec with separators as spaces (`db/migrate` → `"db migrate"`). Each leaf is a `Command { name, components, path, front, is_local }` (`name` is the last component, `components` the full path, `full_name()` the space-joined form); each `Namespace` exposes `subcommands()` (sorted child names). The `Index` exposes the dispatch-style `resolve(args)` (below) as its external entry point, plus thin lookup helpers: `get`, `is_namespace`, `top_level`, `leaves`. The internal greedy tree walk (`resolve_node`/`node`) is private.
5. **resolution** — `index.resolve(args)` (also in `index.rs`) walks the tree and lifts the result into a `Resolution`: `Builtin(&Builtin)`, `Command { command, consumed }`, `Namespace { namespace, consumed }`, or `NotFound`. `Command`/`Namespace` carry `name`, `components`, and a filesystem `path`; `Resolution` exposes `name()`, `components()`, `usage()`, `summary()`, and `help()` accessors that unify these across variants (a namespace's summary is a synthetic subcommand count). `consumed` is how many tokens the (possibly multi-token) name took; the rest pass through. Built-ins are single-token and authoritative for `args[0]` unless a depth-1 external declares `override: true`. A bare namespace (`zub db`) resolves to `Namespace` and `main.rs` routes it to the `help` built-in for the child table. External commands are run via `index::exec_external` (process replacement).

### Front-matter (`frontmatter.rs`)

Subcommands self-document via a contiguous run of comment lines beginning with a comment leader + `@` (`#@`, `//@`, `--@`, `;@`). The text after the sigil is plain YAML. The parser skips a shebang, collects marker lines until the first non-marker line, and stops — it never reads the whole script, keeping discovery fast regardless of file size. Recognized keys: `summary`, `usage`, `help`, `complete`, `eval`, `override`. Unknown keys are ignored (forward-compatible). `serde` maps the YAML `override` key to the `overrides` field.

### Built-ins (`builtins/`)

`builtins/mod.rs` holds the single registry (`BUILTINS`) and `run` dispatcher; each built-in is one file (`commands`, `completions`, `help`, `init`, `new`, `source`). A `Context` struct (identity + config + discovered commands) is threaded through all of them. Each `BuiltinDoc` carries the command's name, summary/usage/help strings, and a `run: fn(&[String], &Context) -> i32` pointer — adding a built-in is a single new entry in `BUILTIN_DOCS`. Dispatch's built-in membership check (`builtins::is_builtin`) reads the same list. (Scaffolding is *not* a built-in — see below.)

### eval commands

A command whose front-matter sets `eval: true` is a shell-eval command (its stdout is meant to be `eval`'d by the shell, enabling `cd`-like effects). There is no name munging — one command is one file, named normally. The `commands` built-in filters these with `--eval`/`--no-eval` (built-ins are never eval), and the `init` script's wrapper `eval`s their stdout (`eval $(zub -C <config> "$command")`) instead of running them normally. `new --eval` scaffolds a single file carrying `eval: true`. See `builtins/commands.rs`, `builtins/new.rs`, and `builtins/init.rs`.

## Conventions

- **Unix-only.** Relies on `std::os::unix::process::CommandExt` for `exec`/`arg0`.
- **No CLI framework.** Argument dispatch is hand-rolled so unrecognized flags pass straight to external commands.
- **Test style.** Unit tests live inline (`#[cfg(test)] mod tests`) next to each module; pure functions (`build_env`, `parse_str`, `resolve`) are favored so logic is testable without filesystem or process effects. `tests/dispatch.rs` builds temp program trees and runs the compiled binary end-to-end via `CARGO_BIN_EXE_zub`.

## Scaffolding & templates

### Scaffolding (`scaffold.rs` + `bin/zub-scaffold.rs`)

`scaffold::create_program(target, name, mode, confirm)` bootstraps a new program tree: `zub.yml`, a self-locating `bin/<name>` shim (re-execs `zub -C <root>/zub.yml`), the completion scripts (`completions/_zub` shared + `_<name>` + `<name>.bash`), and an example `libexec/who` command (front-matter + `--complete` branch + forwards to system `who`). `bin/zub-scaffold.rs` is a thin `main` over it. The `Mode` arg controls how pre-existing *generated* files are handled (the user's own `libexec` commands and `share` contents are never touched): `Normal` refuses if the target dir exists (the default, `zub-scaffold <name>`); `Regenerate` (`--regenerate`) rewrites them, calling `confirm` before replacing any that already exist and writing missing ones silently; `Clobber` (`--regenerate=clobber`) replaces them unconditionally. A private `write_generated` helper centralizes the exists/prompt/overwrite decision; the binary's `confirm` is a stdin `[y/N]` prompt.

### Scaffold templates (`src/templates/`)

The scaffolded shell files live as real, lint-able files in `src/templates/`, embedded at compile time via `include_str!`. The `@NAME@` sentinel is replaced with the program name. Edit these files directly — not escaped Rust string literals. The zsh completer is name-agnostic (reads `$service`); only the per-program `_<name>` and `<name>.bash` carry the literal name.

### `migrate-frontmatter` (repo root)

Standalone Python 3 script converting old-style front-matter (`# Summary:`/`# Usage:`/`# Help:`) to the new `#@` YAML. Adds `complete: true` when the body has a `--complete` branch. Idempotent; `-i` rewrites in place, else prints to stdout.
