# Changelog

Notable changes to zub. Releases before this file was started are recorded in
the git history and the `v*` tags.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `$ZUB_LOCAL_ROOT` pseudo-variable for `command_roots` entries: the nearest
  ancestor of the current directory (the current directory included, searching
  up to `/`) that holds a `.<name>` directory, the way `git` finds `.git`. A
  project's `.<name>/libexec` now supplies its commands anywhere inside that
  project instead of only in the one directory containing it.
- `ZUB_LOCAL_ROOT` is exported to every subcommand when a project root was
  found, giving scripts the equivalent of `git rev-parse --show-toplevel`. It is
  explicitly *removed* from the environment when no project root was found, so a
  subcommand that re-enters `zub` from another directory can't inherit a stale
  value.

### Changed

- **The default `command_roots` now uses `$ZUB_LOCAL_ROOT`** rather than `$PWD`:

  ```yaml
  command_roots:
    - $ZUB_ROOT/libexec
    - $ZUB_LOCAL_ROOT/.$ZUB_INSTANCE/libexec   # was $PWD/.$ZUB_INSTANCE/libexec
  ```

  A `.<name>/libexec` *above* your current directory now contributes commands
  where it previously did not. Programs that want the old exact-match behavior
  should set `command_roots` explicitly with `$PWD/.$ZUB_INSTANCE/libexec`;
  `$PWD` keeps its literal meaning.

  Two consequences worth knowing:

  - The search looks only for `.<name>` itself and stops at the first one it
    finds, so a nearer `.<name>/` shadows a further one even when it holds no
    `libexec` — in that case no local commands load.
  - The walk continues to the filesystem root, so a `~/.<name>/` acts as a
    user-level overlay everywhere under your home directory.

- A command root is flagged working-directory-local (its commands marked
  `(local)` in listings) when its template references `$PWD` **or**
  `$ZUB_LOCAL_ROOT`.
