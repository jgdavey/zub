# Rewriting the `sub` core in Rust — Design

**Date:** 2026-05-23
**Status:** Approved design, pending implementation plan

## Goal

Rewrite the core of `sub` (the dispatcher and built-in commands) in Rust for
performance, while preserving the project's design goals:

- Subcommands remain standalone executables in **any** scripting language.
- Subcommands are **discovered on the filesystem** — no central manifest, no
  pre-declaration.
- Self-documenting commands, shell completion, `sh-` shell-eval commands, local
  subs, and `_<NAME>_ROOT` exposure all continue to work.

The two things explicitly opened up for optimization are **the core** and **the
script headers** (the magic-comment front-matter), both of which are slow today
because the bash core spawns `sed`/`awk`/`grep` once per file on every
invocation.

## Background: how `sub` works today

- `libexec/sub` is a bash dispatcher. It resolves its own path via `readlink`
  walks, exports `_SUB_ROOT`/`_LOCAL_SUB_ROOT`, prepends `libexec` dirs to
  `PATH`, then `exec`s `sub-<command>` found on `PATH`.
- Built-ins (`commands`, `completions`, `help`, `init`, `new`, `source`,
  `sh-shell`) are separate bash executables on `PATH`.
- Subcommands carry magic comments (`# Usage:`, `# Summary:`, `# Help:`,
  `# Provide <name> completions`). The core extracts them by spawning `sed`/
  `awk` per file, and detects completion support with a full-file
  `grep -- --complete`.
- `sh-` commands have their stdout `eval`'d by the shell (for `cd`-like effects),
  wired up by the function `init` emits.
- A program is created by cloning the repo and running `prepare.sh NAME`, which
  **sed-substitutes** the literal string `sub` → the new name across every file.
- Local subs: `$PWD/.sub/libexec` augments/overrides the global install.

The hot paths — bare `sub`, `help`, and completion — re-scan all of `PATH` and
shell out per file. That is exactly what the Rust core eliminates.

## Architecture overview

A single generic Rust binary provides the dispatcher and all built-ins. A "sub
program" is a directory tree (`bin/`, `libexec/`, `completions/`, `share/`, plus
a `sub.yml` config) whose `bin/<name>` entry resolves to the binary. The binary
learns *which* program it is from its invoked name and a YAML config file, and
does the expensive root/PATH setup **once per shell session** inside the `init`
script rather than on every invocation.

```
program-root/
├── sub.yml                 # declares the program name (+ optional metadata)
├── bin/<name>              # entrypoint resolving to the shared binary
├── libexec/<name>-<cmd>    # subcommand executables, any language
├── completions/            # shipped shell-completion shims
└── share/                  # static data, exposed via _<NAME>_ROOT
```

### Components

1. **Identity resolver** — determines the program name and root.
2. **Config loader** — reads `sub.yml` (authoritative name + metadata).
3. **Front-matter parser** — extracts the sigil/YAML header from a script.
4. **Command index** — discovers and describes available subcommands.
5. **Dispatcher** — resolves a command name to a built-in or an external
   executable and runs it.
6. **Built-ins** — `commands`, `help`, `completions`, `init`, `new`, `source`,
   `scaffold`.

Each component is independently testable: identity/config from fixtures, the
parser from string inputs, the index from a temp `libexec`, dispatch from a temp
tree, and built-ins from their rendered output.

## Identity & root resolution

- **Invoked name** = `basename(argv[0])` (the symlink/wrapper name). This gives
  the program name cheaply and tells the binary which `_<NAME>_ROOT` env var to
  read. The `<name>-` command prefix is derived from it.
- **Root resolution order:**
  1. `_<NAME>_ROOT` env var — the fast path, exported by the `init` script.
  2. Fallback: walk up from the **invocation path** (`argv[0]`, i.e. the
     `bin/<name>` entry inside the program tree) to find `sub.yml`. The shared
     binary lives outside the tree, so its own resolved location is not used for
     root discovery.
- **`sub.yml` is authoritative** for the name (used by `init`, and as validation
  / fallback when the env var is absent — e.g. scaffolding or CI).
- **Local subs:** `$PWD/.sub/libexec` is detected at runtime, prepended ahead of
  the global `libexec`, and exposed as `_<NAME>_LOCAL_ROOT`.

## Configuration: `sub.yml`

YAML, for consistency with the front-matter (single serialization format, one
parser). `sub.yaml` is also accepted.

```yaml
name: rush
# optional:
version: 0.1.0
description: A delicious way to organize programs
```

Unknown keys are ignored for forward compatibility — the same philosophy as the
header parser.

## Script front-matter (the new header)

A self-delimiting, sigil-prefixed YAML block placed immediately after the
shebang.

```bash
#!/usr/bin/env bash
#@ summary: Check who's logged in
#@ usage: rush who
#@ complete: true
#@ help: |
#@   Prints who is logged in.
#@   Indented, multi-line, the YAML way.

who
```

### Parse algorithm (the hot path)

1. Skip the shebang line if present.
2. Collect **contiguous** lines matching the marker. The marker is the file's
   comment leader followed by `@`: `#@` (bash/ruby/python/perl), `;@` (lisp),
   `//@` (JS/C-likes), `--@` (sql/lua).
3. From each collected line strip exactly `leader + @ + one optional space`.
   This preserves YAML's significant indentation: `#@   Prints` → `  Prints`, so
   block scalars and nesting survive.
4. **Stop at the first non-marker line.** For the example, that is the blank
   line — the parser touches ~5 lines and quits, never reading the script body.
5. Concatenate the stripped remainders and parse the result with a real YAML
   parser (full YAML: scalars, typed values, block scalars, lists, nesting).

### Recognized keys

| Key         | Meaning                                                        |
|-------------|----------------------------------------------------------------|
| `summary`   | One-line description for `help` and bare `<name>` listings.    |
| `usage`     | Usage line for `help <cmd>`.                                   |
| `help`      | Long help text (block scalars welcome).                        |
| `complete`  | `true` ⇒ command accepts `--complete`. Replaces the full-file `grep -- --complete`. |
| `override`  | `true` ⇒ an external command may replace a built-in of the same name. |

Unknown keys are ignored. Full YAML is honored so new keys (e.g. a static
completions list) can be added later without a format change.

### Migration

Existing scripts use the old `# Summary:` / `# Usage:` / `# Help:` /
`# Provide <name> completions` comments. The new format is a clean break, but an
**optional back-compat shim** may translate the old comments when no sigil block
is present. (The shim's exact scope is an implementation-plan decision.)

## Built-in commands

`commands`, `help`, `completions`, `init`, `new`, `source`, and `scaffold` live
inside the binary. They are the hottest paths in the tool, so folding them in
removes per-invocation subprocess spawns entirely.

- **Override semantics:** built-ins are authoritative by default. An external
  `libexec/<name>-<cmd>` with a reserved name only takes precedence if its
  front-matter declares `override: true`. The core therefore parses a reserved
  name's header *only when such a file exists* — rare, so the hot path stays
  clean and a stray filename can never silently mask core behavior.
- Built-ins are reached from scripts as `rush <cmd>` (not `rush-<cmd>`), since
  they no longer exist as separate executables on `PATH`.

### Built-in responsibilities

- **`commands`** — list available command names (with `--sh` / `--no-sh`
  filters, used by completion and `init`).
- **`help`** — bare form renders the command table (summaries); `help <cmd>`
  renders usage + summary + help, and may invoke `<cmd> --help` for extended
  help.
- **`completions`** — the entrypoint shell completion calls. Uses the `complete`
  header flag to decide which commands accept `--complete`, then `exec`s the
  command with `--complete` and the current words; otherwise signals fallback to
  generic (filename) completion.
- **`init`** — emits the shell integration (see below).
- **`new`** — generate a new subcommand script (today's `sub-new`), emitting the
  sigil front-matter template.
- **`source`** — print a command's source (via `bat`/`PAGER`/`cat`).
- **`scaffold`** — create a new sub *program* (replaces `prepare.sh`); see below.

## Dispatch & composition

- Subcommand files keep the **`<name>-<cmd>`** convention and `libexec` is
  **prepended to `PATH`** (preserved from today). Subcommands can `exec` siblings
  directly (`rush-other`) or route through the binary (`rush other`).
- The dispatcher resolves a command name as: external override with
  `override: true` → built-in → external `<name>-<cmd>` on `PATH`. Unresolved
  names produce `<name>: no such command \`<cmd>'` on stderr, exit 1.
- Exported environment (matching today's contract): `_<NAME>_ROOT`,
  `_<NAME>_LOCAL_ROOT`, `ORIG_PATH`, and the munged `PATH`.

## Shell integration (`init`)

`init` is where the one-time, expensive setup happens. The
`eval "$(.../bin/rush init -)"` line gives `init` the binary's full path, so it
resolves the root once and bakes the results into the emitted shell code:

- `export _<NAME>_ROOT=<resolved root>`
- prepend `libexec` (and the binary's `bin`) to `PATH`
- load the completion script for the detected shell
- define the `sh-` **wrapper function**: for commands the program marks as `sh-`,
  the wrapper captures stdout and `eval`s it (enabling `cd`-style effects);
  everything else runs normally.

Because root and PATH are established here, per-invocation calls just read env
vars — no `readlink` walks, no path resolution on the hot path.

## Completion

- The `completions` built-in remains the runtime entrypoint the shell scripts
  call.
- The `complete: true` header replaces the full-file `grep`, so the core knows
  which commands accept `--complete` without scanning bodies.
- Static shim scripts (`completions/_<name>` for zsh, `completions/<name>.bash`
  for bash) are still shipped/loadable; they simply call
  `<name> completions …` and forward the results. zsh's existing
  special-cases (e.g. `cd` directory completion, `42` ⇒ generic fallback) are
  preserved.

## Creating a sub

- **New subcommand** — the `new` built-in, updated to emit the sigil
  front-matter template (and an optional `sh-` companion, as today).
- **New program** — `scaffold <name>` (replaces `prepare.sh` and its sed
  templating). It creates the program directory with `sub.yml` (name declared,
  no source rewriting), `bin/<name>` resolving to the binary, and empty
  `libexec/`, `completions/`, and `share/`. Because identity now comes from
  `argv[0]` + `sub.yml`, **no file content is rewritten** — the sed pass that
  `prepare.sh` performed is gone entirely.

## Performance posture

- Native parsing with early-terminating header reads; **zero subprocess spawns**
  on the dispatch, `help`, and completion paths.
- Root/PATH resolution amortized to once per shell session via `init`.
- **No index cache in v1** — native parsing of a few leading lines per command is
  expected to be fast enough for realistic `libexec` sizes.

### Future extension: index cache (not built yet)

If a very large `libexec` ever makes the per-invocation header scan noticeable,
an opt-in cache can be added without changing any on-disk format:

- **Location:** a cache file under the program root (e.g.
  `<root>/.sub-cache` or an XDG cache dir keyed by root path).
- **Contents:** for each command, its resolved path, parsed `summary`/`usage`/
  `complete`/`override`, and the source file's `mtime` + size.
- **Invalidation:** on read, compare each entry's `mtime`/size against the file;
  stale or missing entries are re-parsed and the cache rewritten. A directory
  `mtime` check can short-circuit when nothing changed.
- **Scope:** benefits the index-building paths (`commands`, `help`,
  `completions`). Single-command dispatch already only stats one file and would
  not use the cache.

This is recorded as a deliberate future option, not a v1 requirement.

## Testing strategy

- **Front-matter parser:** unit tests over string inputs — each comment leader,
  indentation preservation, block scalars, early termination, missing/blank
  blocks, malformed YAML.
- **Identity/config:** fixtures for env-var fast path, `sub.yml` fallback,
  `.yaml` acceptance, and missing config.
- **Command index & dispatch:** temp `libexec` trees — discovery, local-sub
  precedence, `override` resolution, unknown command, reserved-name collisions.
- **Built-ins:** golden-output tests for `commands`, `help`, `help <cmd>`, and
  `completions` (including the `--complete` passthrough and generic-fallback
  signal).
- **Shell integration:** snapshot the `init` output for bash and zsh; an
  end-to-end test that sources it and exercises an `sh-` command.

## Out of scope

- Rewriting individual user subcommands (they stay in whatever language).
- A package/distribution mechanism beyond the program tree + shared binary.
- The index cache (documented above as a future extension).
