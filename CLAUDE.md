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

1. **config** — `main.rs`'s `parse_args` (lexopt; unit-tested) reads the leading globals into an `Invocation { config, version, help, rest }`: `-C/--config <path>` (also `--config=path`), `-V/--version` (prints `zub <CARGO_PKG_VERSION>` and exits, before any config), and `-h/--help`. The command and everything after it are captured **verbatim** via lexopt's `raw_args()`, so a subcommand's own flags are never parsed by `zub`. The config path is `-C/--config`, else `$ZUB_CONFIG`, else error. `config::load` returns `Result<Config, LoadError>`, distinguishing a read failure (missing/unreadable file) from a YAML parse error so `main.rs` can report the underlying cause. (`name`, optional `root`/`command_roots`/`version`/`description`.)
2. **identity** — `resolve(config_path, config)` derives `name` from the config, `root` from the config's `root` field (when set) else the config file's parent dir, and resolves `command_roots` from the config's `command_roots` list (`effective_templates`, defaulting to `["$ZUB_ROOT/libexec", "$ZUB_LOCAL_ROOT/libexec"]`). Each entry's `$ZUB_ROOT`/`$ZUB_INSTANCE`/`$PWD`/`$ZUB_LOCAL_ROOT` pseudo-vars are expanded, a still-relative entry is resolved against `root`, and an entry referencing `$PWD` or `$ZUB_LOCAL_ROOT` is flagged `is_local`. `$ZUB_LOCAL_ROOT` is the local counterpart of `$ZUB_ROOT` — **the `.<name>` directory itself**, not its parent, so a template never respells `.$ZUB_INSTANCE`: `find_local_root` walks up from `$PWD` (inclusive, to `/`) and returns the first `<ancestor>/.<name>` that is a **directory** — contents irrelevant, so a nearer empty `.<name>` shadows a fuller one further up. The walk runs only when some template mentions the var, and when it finds nothing those entries are **dropped** (not expanded against an empty path). Produces an `Identity { name, root, command_roots: Vec<CommandRoot { path, is_local }>, local_root: Option<PathBuf>, config_path }`. The list is ordered lowest-precedence first (a later root overrides an earlier one). `Identity::new_command_dir()` (where `new` creates a command) is the first non-local root.
3. **env_setup** — `apply` exports `ZUB_CONFIG`, `ZUB_ROOT`, and `ZUB_INSTANCE` (the program name) so every child process inherits the program's identity, plus `ZUB_LOCAL_ROOT` when `identity.local_root` is `Some` — *removing* that var when it is `None`, so a nested `zub` call from another directory can't inherit a stale value. It deliberately does **not** put libexec on `PATH`: subcommands are reachable only through `zub` (or the generated wrapper), never as standalone executables on `PATH`, so they can't shadow built-ins, system commands, or each other. A subcommand that needs a sibling re-enters via `zub` (`$ZUB_CONFIG` is set for exactly this).
4. **index** — recursively discover executable files across `identity.command_roots` (scanned in reverse so the highest-precedence/last root is scanned first and wins slot collisions, since the first occupant of a slot wins; nonexistent roots skipped; dotfiles and non-executables skipped; a command whose front-matter is malformed YAML is kept with default docs and a one-line warning is printed) into a tree of `Node::Leaf(Command)` / `Node::Branch(Namespace)`, where a `Namespace` holds its `name`, `components`, directory `path`, and child nodes (just as a `Command` is held directly in a leaf). A command's name is its path relative to libexec with separators as spaces (`db/migrate` → `"db migrate"`). Each leaf is a `Command { name, components, path, front, is_local }` (`name` is the last component, `components` the full path, `full_name()` the space-joined form); each `Namespace` exposes `subcommands()` (sorted child names). The `Index` exposes the dispatch-style `resolve(args)` (below) as its external entry point, plus thin lookup helpers: `get`, `is_namespace`, `top_level`, `leaves`. The internal greedy tree walk (`resolve_node`/`node`) is private.
5. **resolution** — `index.resolve(args)` (also in `index.rs`) walks the tree and lifts the result into a `Resolution`: `Builtin(&Builtin)`, `Command { command, consumed }`, `Namespace { namespace, consumed }`, or `NotFound`. `Command`/`Namespace` carry `name`, `components`, and a filesystem `path`; `Resolution` exposes `name()`, `components()`, `usage()`, `summary()`, and `help()` accessors that unify these across variants (a namespace's summary is a synthetic subcommand count). `consumed` is how many tokens the (possibly multi-token) name took; the rest pass through. Built-ins are single-token and authoritative for `args[0]` unless a depth-1 external declares `override: true`. A bare namespace (`zub db`) resolves to `Namespace` and `main.rs` routes it to the `help` built-in for the child table. External commands are run via `index::exec_external` (process replacement).

### Front-matter (`command_meta.rs`)

Subcommands self-document via a contiguous run of comment lines beginning with a comment leader + `@` (`#@`, `//@`, `--@`, `;@`). The text after the sigil is plain YAML. The parser captures the shebang's interpreter (the text after `#!`) into the non-YAML `interpreter` field (`#[serde(skip)]`, reserved for future use), then collects marker lines until the first non-marker line, and stops — it never reads the whole script, keeping discovery fast regardless of file size. Recognized keys: `summary`, `usage`, `help`, `complete`, `eval`, `dynamic_help`, `override`. Unknown keys are ignored (forward-compatible). `serde` maps the YAML `override` key to the `overrides` field.

`dynamic_help: true` makes `help <cmd>` print the static `help` text (if any) and then run the command with `--help` appended, letting the script emit the rest of its help (the `help` built-in handles this — a dynamic-help command is shown even with no static front-matter). The default (`false`) shows only the static text.

### Built-ins (`builtins/`)

`builtins/mod.rs` holds the single registry (`BUILTINS`) and the `run`/`complete` dispatchers; each built-in is one file (`commands`, `completions`, `help`, `init`, `new`, `source`). A `Context` struct (the `Identity` plus the discovered-command `Index`) is threaded through all of them. Each `Builtin` carries the command's name, summary/usage/help strings, and `run`/`complete` `fn(&[String], &Context) -> i32` pointers — adding a built-in is a single new entry in `BUILTINS`. Built-in membership is decided in `Index::resolve` (it consults `builtins::get` first), so a built-in is authoritative for `args[0]` unless a depth-1 external declares `override: true`. (Scaffolding is *not* a built-in — see below.)

### eval commands

A command whose front-matter sets `eval: true` is a shell-eval command (its stdout is meant to be `eval`'d by the shell, enabling `cd`-like effects). There is no name munging — one command is one file, named normally. The `commands` built-in filters these with `--eval`/`--no-eval` (built-ins are never eval), and the `init` script's wrapper `eval`s their stdout (`eval $(zub -C <config> "$command")`) instead of running them normally. `new --eval` scaffolds a single file carrying `eval: true`. See `builtins/commands.rs`, `builtins/new.rs`, and `builtins/init.rs`.

## Conventions

- **Unix-only.** Relies on `std::os::unix::process::CommandExt` for `exec`/`arg0`.
- **Argument parsing uses `lexopt`** (a tiny, zero-dependency parser); don't reach for a heavier framework like `clap`. `main.rs` (`parse_args`), `zub-scaffold` (`parse_args`), and the `new` built-in (`parse_flags`) all parse with lexopt and return `Result<_, lexopt::Error>`. Crucially, `main.rs` parses only the leading globals and then captures the command + its args **verbatim** via `Parser::raw_args()`, so unrecognized flags still pass straight through to external commands — never feed a subcommand's args through an option parser. The pass-through built-ins (`help`/`source`/`completions`) and the trivial first-arg checks (`commands`, `init`) stay hand-rolled.
- **Error reporting & exit codes.** User-facing errors go to stderr prefixed with the program name (`{ctx.identity.name}: …`; the `zub`/`zub-scaffold` binaries use their own name before a config is loaded). Exit codes come from the `zub::exit_codes` module (`FAILURE` 1, `USAGE` 2, `EXEC_FAILED` 126, `NOT_FOUND` 127, `COMPLETION_FALLBACK` 42) rather than bare literals — use those constants. "No such command" is written `{prog}: no such command \`{cmd}'` and returns `NOT_FOUND`.
- **Test style.** Unit tests live inline (`#[cfg(test)] mod tests`) next to each module; pure functions (`parse_str`, `resolve`) are favored so logic is testable without filesystem or process effects. `tests/dispatch.rs` builds temp program trees and runs the compiled binary end-to-end via `CARGO_BIN_EXE_zub`.

## Scaffolding & templates

### Scaffolding (`scaffold.rs` + `bin/zub-scaffold.rs`)

`scaffold::create_program(target, name, mode, confirm)` bootstraps a new program tree: `zub.yml`, a self-locating `bin/<name>` shim (re-execs `zub -C <root>/zub.yml`), the completion scripts (`completions/_zub` shared + `_<name>` + `<name>.bash`), and an example `libexec/who` command (front-matter + `--complete` branch + forwards to system `who`). `bin/zub-scaffold.rs` is a thin `main` over it: its `parse_args` (pure, unit-tested) reads `<program>`, an optional `--dir <path>` (the target dir, default `<cwd>/<name>`; a relative path is taken against the cwd), the regenerate flags, and `-V/--version` (prints `zub-scaffold <CARGO_PKG_VERSION>` and exits, no name required). The `Mode` arg controls how pre-existing *generated* files are handled (the user's own `libexec` commands and `share` contents are never touched): `Normal` refuses if the target dir exists (the default, `zub-scaffold <name>`); `Regenerate` (`--regenerate`) rewrites them, calling `confirm` before replacing any that already exist and writing missing ones silently; `Clobber` (`--regenerate=clobber`) replaces them unconditionally. A private `write_generated` helper centralizes the exists/prompt/overwrite decision; the binary's `confirm` is a stdin `[y/N]` prompt.

### Scaffold templates (`src/templates/`)

The scaffolded shell files live as real, lint-able files in `src/templates/`, embedded at compile time via `include_str!`. The `@NAME@` sentinel is replaced with the program name. Edit these files directly — not escaped Rust string literals. The zsh completer is name-agnostic (reads `$service`); only the per-program `_<name>` and `<name>.bash` carry the literal name.

### `migrate-frontmatter` (repo root)

Standalone Python 3 script converting old-style front-matter (`# Summary:`/`# Usage:`/`# Help:`) to the new `#@` YAML. Adds `complete: true` when the body has a `--complete` branch. Idempotent; `-i` rewrites in place, else prints to stdout.
