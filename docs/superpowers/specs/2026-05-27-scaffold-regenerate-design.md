# `zub-scaffold --regenerate` Design

## Problem

`scaffold::create_program` hard-refuses when the target directory already
exists. There is no way to refresh the generated scaffolding (the `bin` shim
and completion scripts) of an existing program after `zub` itself changes,
short of deleting and recreating the tree by hand — which destroys the user's
own `libexec` commands and `share` contents.

## Goal

Add a `--regenerate` mode to `zub-scaffold` that rewrites only the files
scaffold knows how to generate, while never touching the user's own work.

## CLI surface

```
zub-scaffold <program> [--regenerate[=clobber]]
```

The program name is the first non-flag argument; the flag may appear in any
position. Three modes result:

| Mode       | Flag                   | Behavior when a generated file already exists      |
|------------|------------------------|----------------------------------------------------|
| normal     | *(none)*               | Refuse upfront if the target directory exists (current behavior) |
| regenerate | `--regenerate`         | Prompt `Replace <path>? [y/N]`; overwrite only on `y` |
| clobber    | `--regenerate=clobber` | Always overwrite, no prompt                        |

## The generated file set

These are the only files scaffold ever writes or considers:

- `zub.yml`
- `bin/<name>` (self-locating shim, mode 0755)
- `completions/_zub`
- `completions/_<name>`
- `completions/<name>.bash`
- `libexec/who` (example command, mode 0755)

Everything else — the user's own `libexec/*` commands and any `share/`
contents — is **never touched** in any mode, because scaffold does not generate
it.

The four directories (`bin`, `libexec`, `completions`, `share`) are ensured via
`create_dir_all` (idempotent) in all modes.

A generated file that does **not** yet exist is always written without
prompting, even in regenerate mode. So `--regenerate` on a partially-deleted
tree fills the gaps and asks only about the files it would overwrite.

## Architecture

`create_program` gains a `Mode` enum and an injected confirm callback so the
prompt is testable without real stdin:

```rust
pub enum Mode {
    Normal,
    Regenerate,
    Clobber,
}

pub fn create_program(
    target: &Path,
    name: &str,
    mode: Mode,
    confirm: &mut dyn FnMut(&Path) -> bool,
) -> io::Result<()>
```

A private helper centralizes the exists/prompt/overwrite decision and the
optional executable-bit set:

```rust
fn write_generated(
    path: &Path,
    contents: &[u8],
    executable: bool,
    mode: Mode,
    confirm: &mut dyn FnMut(&Path) -> bool,
) -> io::Result<()>
```

Decision table inside `write_generated`:

- path does not exist → write it (and `confirm` is not called).
- path exists and `mode == Clobber` → overwrite.
- path exists and `mode == Regenerate` → overwrite iff `confirm(path)` is true.
- path exists and `mode == Normal` → does not occur, because Normal refuses
  upfront when the target directory exists; treat defensively as skip.

The upfront "directory already exists" refusal applies to `Normal` mode only.

## Binary wiring (`bin/zub-scaffold.rs`)

Parse the program name and optional `--regenerate` / `--regenerate=clobber`
into a `Mode`. An unrecognized flag, or `--regenerate=<other>`, is a usage
error. Pass a real confirm closure that, for `Regenerate`, prints
`Replace <path>? [y/N] ` and reads a line from stdin — empty input or EOF means
no; a leading `y`/`Y` means yes. For `Normal` and `Clobber` the closure is
never consulted, so a trivial closure suffices there.

Adjust the post-run "Next steps" output so it is not misleading when
regenerating an existing program (only print the create message in `Normal`
mode).

## Testing

Existing scaffold tests are updated to the new signature, passing `Mode::Normal`
and a never-called confirm.

New tests (canned closures, no real stdin):

- regenerate **skips** an existing generated file when confirm returns false,
  leaving its original contents.
- regenerate **overwrites** an existing generated file when confirm returns
  true.
- regenerate **leaves a hand-written `libexec/foo`** (not in the generated set)
  untouched.
- regenerate **fills in a missing generated file** without calling confirm.
- clobber **overwrites** `zub.yml` and `libexec/who` unconditionally (confirm
  never called).

## Out of scope

- The `new` built-in (single-command scaffolding) is unchanged.
- No change to the dispatcher, index, or any built-in.
