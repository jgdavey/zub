# Changelog

Notable changes to zub. Releases before this file was started are recorded in
the git history and the `v*` tags.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-08-30

### Added

- `$ZUB_LOCAL_ROOT` pseudo-variable for `command_roots` entries: the local
  counterpart of `$ZUB_ROOT`, naming the `.<name>` directory that holds a
  project's own `libexec`/`share`. It is found by walking up from the current
  directory (the current directory included, searching up to `/`) to the first
  ancestor that has one, the way `git` finds `.git`. A project's
  `.<name>/libexec` now supplies its commands anywhere inside that project
  instead of only in the one directory containing it.
- `ZUB_LOCAL_ROOT` is exported to every subcommand when the walk found a
  `.<name>` directory, so scripts can reach project-local data at
  `$ZUB_LOCAL_ROOT/share`. It is explicitly *removed* from the environment when
  nothing was found, so a subcommand that re-enters `zub` from another directory
  can't inherit a stale value.

### Changed

- **The default `command_roots` now uses `$ZUB_LOCAL_ROOT`** rather than `$PWD`:

  ```yaml
  command_roots:
    - $ZUB_ROOT/libexec
    - $ZUB_LOCAL_ROOT/libexec   # was $PWD/.$ZUB_INSTANCE/libexec
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
