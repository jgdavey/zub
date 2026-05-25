# Design: recursive, prefix-free command discovery

Date: 2026-05-25

## Problem

Commands in a libexec dir must currently be named `<program>-<cmd>`. The indexer
(`index::discover`) strips that prefix and ignores any file lacking it. This
couples command identity to a filename convention and rules out grouping
commands into subdirectories.

We want to drop the prefix: index **all executable files** in libexec, support
**nested subdirectories** as git-style multi-token subcommands (`zub db migrate`
runs `libexec/db/migrate`), and keep completion and help working.

## Command identity

A command's name is its path **relative to the libexec dir, with path separators
replaced by spaces**: `libexec/db/migrate` → `"db migrate"`. This space-joined
form is the single canonical name used for dispatch matching, help, and display.
Top-level commands keep simple names (`libexec/who` → `"who"`).

## Indexing (`index.rs`)

`discover` walks each libexec dir **recursively**. An entry is indexed iff:

- it is a regular file, or a symlink resolving to one (use metadata that follows
  symlinks), **and**
- its executable bit is set (`mode & 0o111 != 0`).

Directories are descended into but never themselves indexed. **Dotfiles**
(names beginning with `.`) are skipped at every level, and dot-directories are
not descended into.

`CommandInfo` keeps its current shape (`name`, `path`, `front`, `is_local`);
`name` now holds the space-joined relative path. Local libexec is still scanned
first so it wins collisions, keyed by the full name.

## Dispatch (`dispatch.rs`, `main.rs`)

`resolve` becomes a **greedy longest-prefix match**. Given the post-config arg
list, it finds the command whose space-joined components equal the longest
leading run of args, and reports how many tokens that consumed so `main.rs` can
forward the remainder as the command's args.

Signature: `resolve(args: &[String], commands: &[CommandInfo]) -> Resolution`
where `Resolution::External` carries the consumed token count (e.g.
`External { path, consumed }`), `Builtin(name)` implies `consumed == 1`, and
`NotFound` is unchanged. `main.rs` computes `cmd_args = &rest[consumed..]`.

Precedence:

- If `args[0]` is a built-in name, the built-in wins (consume 1) **unless** a
  depth-1 external named `args[0]` declares `override: true`.
- Otherwise the longest external prefix match wins.
- A nested command whose first token equals a built-in name (e.g. a
  `help foo` leaf) is shadowed by the built-in. Acceptable; documented.

Example: `zub db migrate --force` → `External { libexec/db/migrate, consumed: 2 }`,
args `["--force"]`.

## Namespaces

A first-level path segment that is not itself a leaf command (e.g. `db` when only
`db/migrate` exists) is a **namespace**. Namespaces are derived on the fly from
the command list — never stored. A helper enumerates:

- **top-level entries**: distinct first components (each is either a depth-1 leaf
  or a namespace), and
- **children of a namespace prefix**: the distinct next components of commands
  whose name starts with `"<prefix> "`.

`zub db` (a namespace with no matching leaf) prints a help table filtered to
`db`'s children instead of erroring.

## Completion (`completions.rs`)

The shell templates already forward every typed word and render whatever list
comes back, so **no template changes are needed**; the descent logic is
server-side.

- `completions --commands` (first token): list top-level entries — depth-1 leaves
  (with front-matter summaries), namespaces (synthetic summary, see Help), and
  built-ins.
- `completions <tokens…>`: greedily match a leaf command prefix against the
  complete (non-partial) tokens.
  - Tokens land **inside a namespace** (a strict prefix of some command, no leaf
    matched) → emit the distinct next-level child components.
  - Tokens **match a leaf** → delegate to that command's `--complete` with any
    remaining args, as today (only when `complete: true`; built-ins run
    in-process).
  - No match → exit 42 (generic fallback), unchanged.

Help-completion (`help --complete`) lists top-level entries only; completing
deeper `help` arguments is out of scope.

## Help (`help.rs`)

- `render_table` iterates **top-level entries**. Leaves show their front-matter
  summary; namespaces show a synthetic summary `"<n> subcommands"`.
- `render_detail` joins all args into the lookup name. `help db migrate` → that
  leaf's detail. `help db` (namespace) → a table of `db`'s children.

## eval commands (scope limit)

eval commands remain **top-level only**. The `init` wrapper classifies by the
first token, so a nested command is never `eval`'d. This limitation is
documented; reworking the shell wrapper for nested eval is deferred.

## Scaffolding

- `scaffold.rs`: the example command is written to `libexec/who` (bare). The
  `example-command.sh` template comment that mentions running
  `@NAME@-who --complete` is updated to `@NAME@ who`.
- `new.rs`: writes `libexec/<command>` with no program prefix. A `/` in the
  command name (`new db/migrate`) creates the nested directories and sets the
  usage line to the space-joined form (`@NAME@ db migrate`). The
  already-exists check and eval/local handling are unchanged otherwise.

## Testing

- `index.rs`: recursion produces space-joined names; non-executable files and
  dotfiles are skipped; nested local command wins over a same-named root command.
- `dispatch.rs`: greedy match consumes the right token count; longest match wins
  when both `db` (no leaf) and `db migrate` exist; built-in precedence and
  override still hold; unknown → NotFound.
- `completions.rs`: top-level list includes namespaces; namespace prefix emits
  children; leaf prefix delegates with remaining args.
- `help.rs`: table lists namespaces with the synthetic summary; `help db migrate`
  detail; `help db` child table.
- `new.rs`: bare file path; `/` name creates nested path + space-joined usage.
- `tests/dispatch.rs`: switch fixtures to bare filenames; add an end-to-end test
  that `zub db migrate` runs `libexec/db/migrate`.

## Docs

Update the CLAUDE.md sections this change invalidates (identity/index/dispatch
prefix descriptions, scaffolding, `sh-`/eval already done). The stale env/PATH
description from the earlier refactor is out of scope for this change.
