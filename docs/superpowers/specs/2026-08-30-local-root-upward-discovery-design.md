# Spec: `$ZUB_LOCAL_ROOT` — upward discovery of the per-directory overlay

Status: Implemented.

## Goal

Today the default per-directory overlay is `$PWD/.$ZUB_INSTANCE/libexec`, and
`$PWD` expands to the literal current directory. A `.rush/libexec` therefore
only takes effect when you stand in exactly the directory that holds it — step
into a subdirectory and the program's local commands vanish.

Make the local overlay behave like `.git`, `.envrc`, and `node_modules`:
discovered by walking **up** from the current directory, so it applies
throughout a project tree.

## Design

### The pseudo-variable

Add `$ZUB_LOCAL_ROOT` to the pseudo-variables usable in `command_roots`
entries. It resolves to the **nearest ancestor of the current directory,
starting with the current directory itself, that contains a `.<name>`
directory**, walking up to `/`.

The default `command_roots` becomes:

``` yaml
command_roots:
  - $ZUB_ROOT/libexec
  - $ZUB_LOCAL_ROOT/.$ZUB_INSTANCE/libexec
```

Ordering and precedence are unchanged: the local root is last, so it still
overrides same-named commands from the program's own `libexec`.

`$PWD` keeps its existing literal meaning. Configs that already spell
`$PWD/.$ZUB_INSTANCE/libexec` keep exact-match behavior, and that remains the
way to opt out of upward discovery.

### Anchor rule

A directory matches if it contains a `.<name>` entry **that is a directory**.
Nothing about the contents matters, and nothing about the rest of the template
matters — the anchor is a property of the ancestor alone, so every entry that
references `$ZUB_LOCAL_ROOT` resolves against the same directory, and the value
is meaningful enough to export to child processes.

A consequence, chosen deliberately: a nearer `.rush/` that lacks `libexec`
**shadows** a fuller one further up. The nearer anchor wins and its
`.rush/libexec` is then skipped as a nonexistent root, so no local commands
load.

``` text
cwd = /work/a/b
/work/a/.rush/          (exists, no libexec)
/work/.rush/libexec/    (has commands)

-> $ZUB_LOCAL_ROOT = /work/a
-> /work/a/.rush/libexec does not exist, skipped
-> no local commands
```

This trades "find commands wherever they are" for "the nearest marker wins,
period" — the same predictability rule `.git` gives.

### Stop condition

The walk ends at the filesystem root, with no other special cases: not `$HOME`,
not a repo boundary. A `~/.rush/` therefore acts as a user-level overlay
everywhere under the home directory. That is a feature, not an accident; users
who don't want it don't create one.

### Resolution rules

- The walk runs **at most once** per invocation, and only when some template in
  the effective `command_roots` list mentions `$ZUB_LOCAL_ROOT`. Configs that
  don't use it perform no extra filesystem work.
- When the walk finds nothing, every entry referencing `$ZUB_LOCAL_ROOT` is
  **dropped from the list** — not expanded against an empty string, which would
  produce a bogus path like `/.rush/libexec`.
- `is_local` becomes "the template mentions `$PWD` **or** `$ZUB_LOCAL_ROOT`", so
  local commands keep their `(local)` marker in `commands` and `help` output.
- The starting point is `env::current_dir()`, as today. Its physical path is
  what gets walked; no additional canonicalization.
- Standing *inside* a marker directory is unremarkable: from
  `/work/.rush/libexec`, the ancestors `/work/.rush/libexec` and `/work/.rush`
  contain no `.rush`, so the walk anchors at `/work`.

### Environment

`env_setup::apply` exports `ZUB_LOCAL_ROOT` when the walk found a directory,
and **removes** the variable when it did not. Removal matters: without it, a
subcommand that re-enters `zub` from a different working directory could
inherit a stale value from the outer invocation.

``` text
ZUB_CONFIG=/opt/rush/zub.yml
ZUB_ROOT=/opt/rush
ZUB_INSTANCE=rush
ZUB_LOCAL_ROOT=/work          # new; absent when no .rush was found
```

This gives subcommands the equivalent of `git rev-parse --show-toplevel`
without each script re-implementing the walk.

## Components

All new logic lands in two existing modules. No new module, and `index.rs`, the
built-ins, and the scaffold templates are unchanged.

**`identity.rs`**

- `find_local_root(start: &Path, marker: &str) -> Option<PathBuf>` — a walk over
  `Path::ancestors()` returning the first entry where `join(marker).is_dir()`.
  Self-contained and testable against tempdirs.
- `command_roots` and `expand_pseudo_vars` take an added
  `local_root: Option<&Path>`; `command_roots` gains the drop-the-entry filter
  and the widened `is_local` test.
- `DEFAULT_COMMAND_ROOTS` swaps `$PWD` for `$ZUB_LOCAL_ROOT` in its second
  entry.
- `Identity` gains a `local_root: Option<PathBuf>` field, carrying the resolved
  value to `env_setup`. The `fixture` helper and the handful of struct literals
  in `index.rs`, `builtins/new.rs`, and `identity.rs`'s own tests are updated.

**`env_setup.rs`**

- A `LOCAL_ROOT` constant next to `CONFIG`/`ROOT`/`INSTANCE`, and a set-or-remove
  branch in `apply`.

## Testing

**Unit (`identity.rs`, tempdirs):**

- marker in the current directory itself is found;
- marker in an ancestor is found;
- nested markers resolve to the deeper one;
- a `.<name>` that is a *file* is not a match and the walk continues;
- `None` when no marker exists up to the filesystem root;
- an entry referencing `$ZUB_LOCAL_ROOT` is dropped when the walk finds nothing;
- `is_local` is set for a `$ZUB_LOCAL_ROOT` template;
- a config whose roots never mention the variable resolves unchanged.

**Integration (`tests/dispatch.rs`, via `Command::current_dir`):**

- a local command in `<tree>/.rush/libexec` dispatches when run from a
  grandchild directory;
- a subcommand run from that grandchild sees the expected `ZUB_LOCAL_ROOT`;
- with no marker anywhere, dispatch of a local command fails as before and
  `ZUB_LOCAL_ROOT` is absent from the child environment.

## Documentation

- `README.md` — the `command_roots` section (default list, pseudo-variable
  list, the upward-search description) and the environment-variable list.
- `CLAUDE.md` — the **identity** and **env_setup** pipeline bullets.

## Compatibility

This changes the **default** behavior: a `.<name>/libexec` above the current
directory now activates where it previously did not. That is the purpose of the
feature, but it is a visible change and belongs in the release notes. Explicit
`$PWD/...` entries are unaffected, and remain the way to keep exact-match
behavior.

## Out of scope

- Stacked overlays from *every* matching ancestor (only the nearest is used).
- A `new --local` flag, or any change to how `Identity::new_command_dir()`
  chooses its target directory.
- A generic "search upward" operator applicable to arbitrary `command_roots`
  entries.
