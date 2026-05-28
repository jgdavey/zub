# GitHub Actions CI + Release Design

## Problem

The repo (`github.com/jgdavey/sub`, crate `zub`, binaries `zub` and
`zub-scaffold`) has no automated testing and no release process. We want:

1. CI that tests on branch pushes (and PRs).
2. A release that builds cross-platform binaries when a semver tag is pushed.
3. Those releases installable via `mise` (through its `ubi` backend) now, and
   structured so a Homebrew tap can be added later without reworking the build.

## Constraints / context

- The crate is **Unix-only** (`std::os::unix::process::CommandExt`), so there
  are no Windows targets — macOS + Linux only.
- Two binaries ship together: `zub` (dispatcher + built-ins) and `zub-scaffold`.
- Tags are **`v`-prefixed semver**: `v0.1.0`, `v1.2.3`.
- Tooling is **hand-rolled** GitHub Actions YAML (no `cargo-dist`/generator),
  using only thin, widely-used helper actions.

## Workflow 1 — `.github/workflows/ci.yml`

Trigger: `push` (all branches) and `pull_request`. Tag pushes are handled by the
release workflow, so CI excludes tags (`tags-ignore: ['**']` on the push event).
A `concurrency` group keyed on the workflow + ref cancels superseded runs.

Two jobs:

- **lint** (`ubuntu-latest`): `dtolnay/rust-toolchain@stable` with `rustfmt` +
  `clippy`, `Swatinem/rust-cache@v2`, then `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings`.
- **test** (matrix `os: [ubuntu-latest, macos-latest]`): stable toolchain,
  `rust-cache`, `cargo test`. macOS is included so Apple-specific breakage (the
  `exec`/permissions paths) is caught before tag time.

## Workflow 2 — `.github/workflows/release.yml`

Trigger: `push` with `tags: ['v[0-9]+.*']`. `permissions: contents: write` so the
release can be created.

### Job `verify` (ubuntu-latest)

Strips the leading `v` from `github.ref_name` and compares it to the
`Cargo.toml` package version (read via `cargo metadata --format-version=1`,
parsed with `jq`). Fails the whole workflow on mismatch, so a tag can never
publish binaries whose embedded `CARGO_PKG_VERSION` disagrees with the tag.

### Job `create-release` (needs: verify)

Creates the GitHub Release for the tag **once**, as a **draft**, with
`gh release create "$TAG" --draft --generate-notes`. Doing this in a single
job (rather than letting each build leg create-or-append) means release notes
are generated exactly once and there is no create race across the matrix. The
step is idempotent on re-runs (`gh release view` guard).

### Job `build` (needs: create-release)

Matrix of four legs, each emitting one tarball + one checksum file:

| target | runner | build method |
|---|---|---|
| `aarch64-apple-darwin` | `macos-latest` | native `cargo build --release --target` |
| `x86_64-apple-darwin` | `macos-latest` | `rustup target add` then `cargo build --release --target` (Apple toolchain cross-compiles cleanly from the arm64 host) |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` | `cross build --release --target` (static musl) |
| `aarch64-unknown-linux-musl` | `ubuntu-latest` | `cross build --release --target` (static musl) |

Matrix entries carry `target`, `os`, and a `cross: true|false` flag. `cross` is
installed via `taiki-e/install-action@cross` (an install helper, not a release
generator). The two mac legs share one runner type; the linux legs both use
`cross` for uniform static musl output.

### Packaging (shared across legs)

For version `V` (= tag minus `v`) and target `T`, after the build:

```
staging/  ->  zub  zub-scaffold  LICENSE  README.md
tar czf zub-V-T.tar.gz -C staging .
shasum -a 256 zub-V-T.tar.gz > zub-V-T.tar.gz.sha256
```

Binaries are placed at the archive root (not under a subdir) so `ubi`'s `exe=`
resolution finds `zub` directly. The full target triple in the filename is what
`ubi`/mise match on per platform.

### Publishing

Each build leg uploads its `.tar.gz` and `.tar.gz.sha256` into the existing
draft release with `gh release upload "$TAG" … --clobber` (`--clobber` makes a
re-run idempotent). The draft is never public during the build.

### Job `publish` (needs: build)

After every build leg succeeds, flips the release out of draft with
`gh release edit "$TAG" --draft=false`. Because it `needs: build`, a failure in
any leg skips publishing and leaves a draft with partial assets — nothing
half-public is ever released. Release operations use the preinstalled `gh` CLI
(`GH_TOKEN`/`GH_REPO` set at the workflow level) rather than a third-party
action.

## Install documentation (README)

Add an install section:

```sh
# via mise (ubi backend)
mise use -g 'ubi:jgdavey/sub[exe=zub]'

# manual
curl -L https://github.com/jgdavey/sub/releases/download/v0.1.0/zub-0.1.0-<target>.tar.gz \
  | tar xz && mv zub zub-scaffold ~/.local/bin/
```

`ubi`/mise install the primary `zub` binary; the tarball also carries
`zub-scaffold` for manual and (future) Homebrew installs. A second
`ubi:jgdavey/sub[exe=zub-scaffold]` entry can be added if mise should manage
both.

## Designed-in (not built now): Homebrew tap

No tap work in this change. Because release assets are already target-triple
`.tar.gz` files with `.sha256` companions, a later tap is additive: a formula
pointing at the two macOS tarballs (using the published checksums) plus a
publish step that pushes it to a `homebrew-tap` repo. The build matrix does not
change.

## Out of scope

- No Windows targets (crate is Unix-only).
- No `rust-toolchain.toml` pin; CI tracks `stable`. (Can be added later if
  reproducibility is preferred over auto-updates.)
- No Homebrew tap repo, formula, or cross-repo token.
- No crates.io publishing.

## Verification

- CI: pushing a branch runs lint + the two-OS test matrix; all green.
- Release: pushing a `v*` tag runs `verify`, builds all four legs, and attaches
  four `.tar.gz` + four `.sha256` files to a GitHub Release. A tag whose version
  disagrees with `Cargo.toml` fails at `verify`.
- Install: `mise use -g 'ubi:jgdavey/sub[exe=zub]'` fetches and runs `zub` on a
  released platform.
