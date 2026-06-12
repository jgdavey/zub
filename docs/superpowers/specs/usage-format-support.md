# Spec: Support `usage` script specs alongside zub front-matter

Status: Draft (design) — spike completed, no implementation yet.

## Goal

Let a zub subcommand be authored in either of two header styles, picked per-file:

- **zub style** — the existing `#@` YAML front-matter (passive metadata; the
  script parses its own args).
- **usage style** — [usage](https://usage.jdx.dev) `#USAGE` spec directives
  (an active contract: declares flags/args/completions, and the runtime can
  parse argv and hand the script its values).

usage-style commands lean on usage itself for the spec language — at runtime via
the shebang, and (for help/completion) by delegating to the `usage` binary —
rather than zub reimplementing it. Both styles coexist in the same `libexec/`
tree and are discovered, listed, helped, and completed uniformly. zub links **no
usage-lib**: the only usage metadata it reads in-process is the one-line `about`
summary, parsed in-house (see [Decision: no usage-lib
dependency](#decision-no-usage-lib-dependency)).

## Guiding decision

A usage-style script should **feel fully usage**, end to end — from its shebang
line through to its `usage_*` environment-variable handling. zub does **not**
become the usage runtime. At exec time the script is run verbatim and its usage
shebang (`#!/usr/bin/env -S usage bash`) hands parsing and env injection to the
`usage` binary, exactly as a standalone usage script would.

What zub takes over is the **framework layer**: nesting within the zub command
tree, discovery/listing, completion, and help. Those are unified across both
header styles; everything below the framework (argument parsing, env vars, the
script's own runtime contract) stays usage's.

Two consequences follow directly:

- **`eval` and `override` are zub-only and unsupported in usage-style scripts.**
  They have no usage equivalent and would break the "fully usage" feel. A
  usage-style command is never an eval command and can never override a built-in.
- **The `usage` binary is a runtime dependency for usage-style scripts.** Running
  one requires `usage` on `PATH` (the shebang invokes it). zub itself does not
  need `usage` to *dispatch* — it just execs the file — but the script won't run
  without it. This is accepted: authoring usage-style commands means opting into
  usage.

## Why this is two models, not two syntaxes

The existing `#@` front-matter only *describes* a command (`summary`, `usage`,
`help`, `complete`, `eval`, `dynamic_help`, `override`). The script does its own
argument parsing; `complete: true` opts into a `--complete` callback the script
implements.

A usage `#USAGE` spec is a *runtime contract*. usage parses argv against the
declared `flag`/`arg`/`cmd` directives and exposes the results to the script as
`usage_<name>` environment variables; completions and help are generated from
the spec. zub leans on that contract for the framework layer (completion, help,
nesting) while leaving the runtime half — parsing argv and setting `usage_*` —
to usage itself (see [Guiding decision](#guiding-decision)).

| | zub `#@` front-matter | usage `#USAGE` spec |
|---|---|---|
| Nature | passive metadata | active runtime contract |
| Body language | YAML after `<leader>@` | usage directives after `<leader>USAGE` |
| Arg parsing | the script does it | usage parses; script reads `$usage_*` |
| Completion | script's own `--complete` branch | generated from the spec |
| Help | static `help:` / `dynamic_help` round-trip | generated from the spec |

## Spike findings (usage-lib 3.5.0)

A throwaway crate was built against `usage-lib@3.5.0` and run end-to-end to
de-risk the integration.

**Outcome of the spike:** the chosen design links **no** usage-lib (see
[Decision: no usage-lib dependency](#decision-no-usage-lib-dependency)). Runtime
parsing, help, and completion are all delegated to the `usage` binary, and the
only thing zub reads in-process — the summary — is parsed in-house. The
findings below are kept as the record that the lib's capabilities exist (in case
the no-dependency decision is ever revisited) and to pin the two format facts
zub *does* rely on: the detection regex and the `about` directive shape.

### Proven to work

- **Spec extraction — `Spec::parse_script(path)`** pulls `bin`, `about`,
  `flag`s, and `arg`s out of an embedded `#USAGE` block, ignoring the shebang
  and the script body.
- **Runtime argv parsing — `usage::parse(&spec, &argv) -> ParseOutput`** works
  (`greet --loud -g Hey World` yielded `greeting=Hey`, `loud=true`,
  `name=World`). **Not used under the chosen design** — runtime parsing and
  `usage_*` injection are the `usage` binary's job via the shebang, not zub's.
  Recorded here only to document that the capability exists, in case the decision
  is ever revisited.
- **Detection is disjoint.** usage's extractor regex is
  `^(?:#|//|::)(?:USAGE| ?\[USAGE\])`. A zub `#@ summary: …` script parsed as a
  usage spec comes back completely empty (no flags/args/about) — confirmed in
  both directions. So sigil-based detection (`#@` → zub, `#USAGE` → usage) is
  reliable. Note usage's leaders are `#`/`//`/`::`; zub additionally supports
  `;`, `;;`, `--`. The distinguishing token is `@` vs `USAGE`/`[USAGE]`.
- **Bounded header read is possible.** usage's own `parse_script` reads the
  *whole file* (`read_to_string`), which would break zub's fast lazy-header
  invariant; `Spec::from_str` works on a pre-extracted block instead. **Not used
  under the chosen design** — zub does not parse the spec, only the `about` line.
  Recorded so a future usage-lib-linked path knows to feed `from_str` a bounded
  block rather than call `parse_script`.
- **Completion-script generation — `complete::complete(opts)`** runs in-process
  for bash/zsh/fish/nu/powershell. **Not used** — completion is delegated to the
  `usage` binary (caveat A).

### Caveats (decisions, not unknowns)

- **A. Dynamic completion candidates are not in the library.**
  `complete::complete()` only emits the *static* completion script, and that
  script shells out to the `usage` binary (`usage complete-word …`) at
  completion time. The "given these words + cursor, return candidates" logic
  lives in the `usage` CLI, not `usage-lib`. Under the chosen design `usage` is
  already a runtime dependency for usage-style scripts, so zub's completion
  built-in should **delegate to `usage complete-word`** for usage commands
  rather than reimplement candidate computation — consistent with the "fully
  usage" feel. (Computing from the `Spec` in-process stays a fallback option if
  avoiding the subprocess ever matters.)
- **B. `usage_*` env rendering — not zub's concern.** `ParseOutput` would need
  converting into `usage_<name>=…` exports, but under the chosen design the
  `usage` binary does this at runtime via the shebang. zub never renders these.
  (If zub-as-runtime were ever revisited, this logic — `-`→`_` names;
  `Bool`→`true`/empty; `MultiBool`→count; `MultiString`→space-join — is ~10
  lines and would need fixture tests against real `usage` output.)
- **C. Dependency weight — resolved by not linking it.** usage-lib would take zub
  from **55 → 177 transitive crates** (`miette`, `tera`, `kdl`, `regex`, …) — a 3×
  jump in tension with zub's "tiny, zero-dependency parser" ethos. Since help,
  completion, and runtime are all delegated to the `usage` binary, the lib's only
  possible in-process job was summary extraction — and a one-line `about` doesn't
  justify 122 crates. **Decision: do not link usage-lib.** See
  [Decision: no usage-lib dependency](#decision-no-usage-lib-dependency).

## Detection

Reuse the existing leading contiguous comment run after the shebang. Classify by
the first marker token seen:

- a line matching `<leader>@` → **zub** command (parse YAML as today)
- a line matching `<leader>USAGE` (`#`/`//`/`::` + `USAGE`/`[USAGE]`) →
  **usage** command (extract block, `Spec::from_str`)
- neither → default/no metadata

No new opt-in key is required. Keep the contiguous-block rule for both families
to preserve fast discovery (do not adopt usage's whole-file scan).

## Architecture changes

Generalize per-command metadata. Today `Command` holds `front: FrontMatter`.
Introduce a sum type:

```rust
enum CommandMeta {
    Zub(FrontMatter),
    Usage(UsageMeta),
}

struct UsageMeta {
    summary: Option<String>, // from the `about` directive; the only field zub reads
}
```

The `Usage` payload carries just the extracted summary — the only usage metadata
zub reads in-process (see [Decision: no usage-lib
dependency](#decision-no-usage-lib-dependency)). The accessors below are the only
surface the rest of zub sees.

Accessors both variants answer: `summary()`, `usage()`, `help()`,
`wants_completion()`, `eval`, `overrides`. `eval` and `overrides` are always
`false` for the `Usage` variant — they are zub-only and unsupported in
usage-style scripts. `usage()` and `help()` return `None` for usage commands
(their help is delegated to the `usage` binary, below). Each pipeline stage
adapts:

- **frontmatter → header module.** Rename/extend the parser to read the comment
  run, classify the family, and return `CommandMeta`. For usage, the only
  metadata zub needs in-process is the **summary** — see "Extracting the summary"
  below. No usage line is synthesized: the per-command help that would show it is
  delegated to `usage --help`.
- **index / discovery.** Build `CommandMeta` per leaf; everything downstream
  goes through the accessors.
- **help built-in.** For usage commands, exec the command with `--help` appended
  and let usage render its own help — no static `help:` text and no in-process
  rendering. This mirrors the existing `dynamic_help` path (run `cmd --help`) but
  drops the static-text prefix entirely, keeping help output identical to a
  standalone usage script.
- **completions built-in.** For usage commands, delegate to `usage complete-word`
  (caveat A) rather than calling `cmd --complete`. Removes the need for
  hand-written `--complete` branches — the biggest ergonomic win.
- **exec_external.** Unchanged: exec the file with verbatim args. The usage
  shebang does parsing + env injection at runtime. zub adds nothing here for
  usage commands beyond the `ZUB_*` env it already exports for all subcommands.

### Extracting the summary

The summary is the one piece of usage metadata zub needs in-process (for the
`commands` and namespace-help listings). usage's natural short-description field
maps one-to-one onto zub's `summary`: it is the **`about`** directive.

Grounded in `usage-lib` 3.5.0 (`spec/mod.rs:157`):

```rust
"about" => schema.about = Some(node.arg(0)?.ensure_string()?)
```

So `about` takes the first positional string argument of the directive:

```
#USAGE about "Greet a person, maybe loudly"
```

One asymmetry to be aware of: usage uses `about` for a spec's **top-level** short
description but `help` for the short description of nested `cmd` blocks
(`cmd.rs:185`). It does not affect zub — zub does its own nesting via the
filesystem (`libexec/db/migrate`), so **each script file is its own top-level
spec** and always uses `about`. zub never descends into usage's `cmd` nesting.

**The extraction (decided):** while reading the bounded header block, after
stripping the `#USAGE` marker take the first quoted string off the `about`
line — roughly `^\s*about\s+"(.*)"`, with basic `\"`/`\\` unescaping. This covers
the overwhelmingly common `about "…"` form. It does **not** handle KDL raw
strings (`r#"…"#`) or multiline values; those are vanishingly rare for a one-line
summary and are an accepted limitation (a command authored that way simply shows
no summary in listings — same as a missing `about`). A missing `about` likewise
means no summary, exactly like a zub command without `summary:`.

This is a few lines of string work in the existing header parser — no usage-lib,
no subprocess. See [Decision: no usage-lib
dependency](#decision-no-usage-lib-dependency).

## Decision: no usage-lib dependency

zub links **no** part of usage-lib. The integration is built entirely from
delegation + a tiny in-house summary parse:

| concern | how it's handled | who does it |
|---|---|---|
| argument parsing + `usage_*` env | usage shebang at runtime | `usage` binary |
| per-command help (`help <cmd>`) | exec `cmd --help` | `usage` binary |
| completion candidates | `usage complete-word` | `usage` binary |
| summary for listings | parse the `about` line | zub (in-house) |
| detection / nesting / dispatch | existing zub machinery | zub |

Rationale: the only usage metadata zub needs in-process is a one-line `about`
string, which does not justify usage-lib's ~122 transitive crates (caveat C).
Everything else is already a runtime responsibility of the `usage` binary, which
usage-style scripts depend on anyway (via the shebang). This keeps zub's
"tiny, zero-dependency parser" ethos intact and adds zero crates to `Cargo.toml`.

Trade-off accepted: exotic KDL string forms in a summary line are not understood
(see above). If full spec fidelity is ever needed in-process, the spike findings
record how to link usage-lib behind a Cargo feature — but that is explicitly out
of scope here.

## The runtime model: usage runs, zub frames

There is one model (see [Guiding decision](#guiding-decision)). zub reads only
the `about` line for the discovery/listing summary, and execs the script with
verbatim args. The script's usage shebang (`#!/usr/bin/env -S usage bash`) hands
parsing and `usage_*` injection to the `usage` binary at runtime; help and
completion are delegated to that same binary. zub never parses argv for usage
commands and never renders env vars.

**zub-as-runtime is an explicit non-goal.** Parsing argv in-process and injecting
`usage_*` ourselves (technically possible — see caveat B) would dilute the
"fully usage" feel and put zub on the hook for matching usage's env contract
exactly. We decline it. The capability is recorded in the spike findings only so
a future reversal is informed.

This collapses what an earlier draft split into two tiers: the design is the
"metadata-only" shape (verbatim exec; the only in-process work is the `about`
summary parse), and there is no second tier.

## Resolved decisions

- **`eval` / `override`** — unsupported for usage-style scripts. They are
  zub-only and would break the "fully usage" feel. The `Usage` variant reports
  both as `false`.
- **Runtime / env handling** — usage's, via the shebang. zub-as-runtime is a
  non-goal.
- **Shebang convention** — usage-style scripts use the usage shebang
  (`#!/usr/bin/env -S usage bash` or the appropriate language variant). The
  `usage` binary is a runtime dependency for them.
- **Completion** — zub's completion built-in delegates to `usage complete-word`
  for usage commands.
- **Help** — `help <cmd>` execs the command with `--help` appended and lets usage
  render it; no static `help:` text, no in-process rendering. Same shape as the
  existing `dynamic_help` path minus the static prefix.
- **No usage-lib dependency** — zub links no part of usage-lib. Summary is parsed
  in-house from the `about` line; everything else is delegated to the `usage`
  binary. See [Decision: no usage-lib
  dependency](#decision-no-usage-lib-dependency).
- **Missing-`usage`-binary UX** — not zub's problem; no special handling. The
  shebang covers it: with `usage` absent, `#!/usr/bin/env -S usage bash` makes
  `env` fail on exec and on `--help` with a clear `env: 'usage': No such file or
  directory` (exit 127), exactly as any script with a missing interpreter.
  Completion just degrades to nothing via the existing completion fallback.
  Discovery/listing is unaffected (it only reads the header). zub adds no custom
  message.

## Open questions

None blocking. The design decisions above are settled; what remains is
implementation (the header-module split, the `about` parse, and wiring the
`commands`/`help`/`completions` built-ins to the `Usage` variant).

## References

- usage scripts: <https://usage.jdx.dev/cli/scripts>
- usage spec reference: <https://usage.jdx.dev/spec/>
- `complete` directive: <https://usage.jdx.dev/spec/reference/complete>
- `usage-lib` API: <https://docs.rs/usage-lib/latest/usage/>
- zub front-matter parser: `src/command_meta.rs`
