# Design: `eval: true` front-matter replaces the `sh-` prefix

Date: 2026-05-25

## Problem

Today a subcommand whose stdout should be `eval`'d by the shell (for `cd`-like
effects) is identified by a filename convention: `<name>-sh-<cmd>`. The `sh-`
prefix is stripped for display, drives a `--sh`/`--no-sh` filter on `commands`,
and is special-cased in the `init` shell wrapper. `new --sh` even generates a
*second* companion file (`<name>-sh-<cmd>`) alongside the main command.

This couples shell-eval behavior to the filename and forces a two-file pattern.
We want the behavior declared in front-matter instead.

## Solution

An eval command is any discovered `<name>-<cmd>` file whose front-matter
declares `eval: true`. The `sh-` filename convention is removed entirely — one
command is one file, named normally, and its stdout is meant to be `eval`'d by
the shell when invoked through the shell integration wrapper.

This is a **full replacement**: the `sh-` prefix is no longer recognized.

## Changes by module

### `frontmatter.rs`
Add a field to `FrontMatter`:

```rust
#[serde(default)]
pub eval: bool,
```

`eval` is not a Rust keyword, so no `#[serde(rename)]` is needed. Unknown-key
tolerance already handles old files that lack the key. Add a unit test asserting
`#@ eval: true` parses to `eval == true` and absence parses to `false`.

### `index.rs`
No logic change. `discover` already parses front-matter for every command, so
`CommandInfo.front.eval` is populated for free. There is no longer any `sh-`
prefix to key on, so command names are stored as-is.

### `commands.rs`
- Detect eval commands via `info.front.eval` instead of `name.starts_with("sh-")`.
- Stop stripping any prefix from displayed names.
- Rename the filters `--sh`/`--no-sh` → `--eval`/`--no-eval`.
- `collect` must see each command's `FrontMatter`, so it iterates `ctx.commands`
  directly rather than going through a names-only helper. Built-in commands have
  no front-matter and are never eval: under `--eval` they are excluded, under
  `--no-eval` and the default they are included.
- Update the `--complete` output to print `--eval` / `--no-eval`.

### `init.rs`
- The wrapper `case` lists eval command names (obtained via
  `commands::collect(&["--eval"], ctx)`) and evals `zub -C <config> "$command"`
  — no more `sh-` prefix on the dispatched name.
- Rename the `sh_commands` parameter and its doc comment to `eval_commands`.

### `new.rs`
- Rename the `--sh` flag to `--eval` (struct field `sh` → `eval`; `--complete`
  prints `--eval`).
- When `--eval` is set, write a **single** file `<prog>-<cmd>` whose template
  carries `#@ eval: true` and a minimal eval-style body, e.g.:

  ```bash
  #!/usr/bin/env bash
  #@ usage: <prog> <cmd>
  #@ summary: (please add docs here)
  #@ eval: true

  # stdout from an eval command is eval'd by your shell
  echo 'cd /some/path'
  ```

- Delete `sh_template` and the second-file write path. There is now exactly one
  generated file per `new` invocation.

### Docs
Update the "`sh-` commands" section of `CLAUDE.md` to describe the `eval: true`
model (single file, no prefix stripping, `--eval` filter). The
`migrate-frontmatter` Python script concerns the unrelated old
`# Summary:` → `#@` conversion and is left untouched.

## Behavior notes

- A command literally named `cd` with `eval: true` lists as `cd` everywhere
  (no prefix munging). This is intended.
- Running an eval command directly (`zub cd`, outside the shell wrapper) prints
  its stdout without evaluating it — unchanged from the old `sub sh-cd`
  behavior.
- No backward compatibility for `sh-` filenames; the branch is mid-rewrite and a
  clean cutover is preferred.

## Testing

- `frontmatter.rs`: `eval: true` parses; default is `false`.
- `commands.rs`: update the two existing filter tests to use `eval: true`
  front-matter and the `--eval`/`--no-eval` flags; assert no prefix stripping.
- `init.rs`: wrapper lists eval command names and evals `zub -C <config>`
  without the `sh-` prefix.
- `new.rs`: `--eval` writes a single file containing `#@ eval: true`; no
  companion file is created; `parse_flags` reads `--eval`.
