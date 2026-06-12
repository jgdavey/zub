use serde::Deserialize;

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct FrontMatter {
    pub summary: Option<String>,
    pub usage: Option<String>,
    pub help: Option<String>,
    /// The interpreter from the script's shebang line (the text after `#!`,
    /// e.g. `/usr/bin/env bash`), captured at parse time. This is *not* a
    /// front-matter key — it is derived from the shebang and reserved for
    /// future use, so `serde` skips it.
    #[serde(skip)]
    pub interpreter: Option<String>,
    #[serde(default)]
    pub complete: bool,
    #[serde(default)]
    pub eval: bool,
    /// When true, `help <cmd>` prints the static `help` text (if any) and then
    /// runs the command with `--help` appended, letting the script emit the
    /// rest of its help. When false (the default), only the static text shows.
    #[serde(default)]
    pub dynamic_help: bool,
    #[serde(rename = "override", default)]
    pub overrides: bool,
}

use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

const LEADERS: [&str; 5] = ["#", ";;", ";", "//", "--"];

/// Comment leaders that introduce a usage directive. These mirror usage-lib's
/// extractor regex `^(?:#|//|::)(?:USAGE| ?\[USAGE\])`.
const USAGE_LEADERS: [&str; 3] = ["#", "//", "::"];

/// The keyword forms a usage directive may take after its leader: bare or
/// bracketed, each optionally preceded by a single space — `USAGE`, `[USAGE]`,
/// and ` [USAGE]`. usage-lib recognizes all of these. Order matters only in
/// that longer prefixes can't shadow shorter ones here, and each are mutually
/// exclusive (their first byte after the leader is `U`, ` `, or `[`).
const USAGE_KEYWORDS: [&str; 3] = ["USAGE", "[USAGE]", " [USAGE]"];

/// A concrete usage sigil: a [leader](USAGE_LEADERS) paired with a
/// [keyword](USAGE_KEYWORDS) form. The first directive line of a block fixes the
/// sigil; the rest of the block must repeat that exact form to continue it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct UsageSigil {
    leader: &'static str,
    keyword: &'static str,
}

/// One subcommand's parsed header. A command is authored in exactly one of two
/// styles, distinguished by its marker sigil (see [`parse_command_str`]):
///
/// - [`CommandMeta::Zub`] — zub `#@` YAML front-matter (the script parses its
///   own args; see [`FrontMatter`]).
/// - [`CommandMeta::Usage`] — a [usage](https://usage.jdx.dev) `#USAGE` spec.
///   The script stays fully usage at runtime (its shebang hands parsing and
///   `usage_*` env to the `usage` binary); zub reads only the one-line summary
///   in-process and delegates help/completion to `usage`.
#[derive(Debug, Clone, PartialEq)]
pub enum CommandMeta {
    Zub(FrontMatter),
    Usage(UsageMeta),
}

impl Default for CommandMeta {
    fn default() -> Self {
        CommandMeta::Zub(FrontMatter::default())
    }
}

impl CommandMeta {
    /// The one-line summary, if documented (zub `summary`, else usage `about`).
    pub fn summary(&self) -> Option<String> {
        match self {
            CommandMeta::Zub(front) => front.summary.clone(),
            CommandMeta::Usage(usage) => usage.summary.clone(),
        }
    }

    /// The static usage line, if any. `None` for usage commands — their help
    /// (usage line included) is rendered by the `usage` binary on `--help`.
    pub fn usage(&self) -> Option<String> {
        match self {
            CommandMeta::Zub(front) => front.usage.clone(),
            CommandMeta::Usage(_) => None,
        }
    }

    /// The static long-form help, if any. `None` for usage commands (delegated).
    pub fn help(&self) -> Option<String> {
        match self {
            CommandMeta::Zub(front) => front.help.clone(),
            CommandMeta::Usage(_) => None,
        }
    }

    /// Whether this command overrides a built-in of the same name. Usage
    /// commands cannot — `override` is a zub-only concept.
    pub fn overrides(&self) -> bool {
        matches!(self, CommandMeta::Zub(front) if front.overrides)
    }

    /// Whether this is a shell-eval command. Usage commands never are — `eval`
    /// is a zub-only concept.
    pub fn eval(&self) -> bool {
        matches!(self, CommandMeta::Zub(front) if front.eval)
    }

    /// Whether zub should offer completion for this command. Zub commands opt in
    /// via `complete: true`; usage commands always do (completion is delegated
    /// to the `usage` binary).
    pub fn wants_completion(&self) -> bool {
        match self {
            CommandMeta::Zub(front) => front.complete,
            CommandMeta::Usage(_) => true,
        }
    }

    /// Whether `help <cmd>` should append `--help` and let the command emit the
    /// rest of its help. True for a zub `dynamic_help` command and for every
    /// usage command (whose help is rendered entirely by the `usage` binary).
    pub fn dynamic_help(&self) -> bool {
        match self {
            CommandMeta::Zub(front) => front.dynamic_help,
            CommandMeta::Usage(_) => true,
        }
    }

    /// Whether this command is authored as a usage `#USAGE` spec.
    pub fn is_usage(&self) -> bool {
        matches!(self, CommandMeta::Usage(_))
    }
}

/// The slice of a usage spec zub reads in-process: just the one-line summary
/// (the spec's `about` directive). Everything else a usage command needs —
/// argument parsing, help, completion — is delegated to the `usage` binary, so
/// no further fields are extracted. `None` when the spec declares no `about`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct UsageMeta {
    pub summary: Option<String>,
}

/// An error parsing a command's front-matter.
#[derive(Debug)]
pub enum ParseError {
    /// The file could not be read.
    Read(io::Error),
    /// The header's marker lines were not valid YAML.
    Yaml(yaml_serde::Error),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Read(e) => write!(f, "{e}"),
            ParseError::Yaml(e) => write!(f, "malformed front-matter: {e}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Strip the marker (`<leader>@` plus one optional space) from a line.
/// Returns `(leader, remainder)` when the line is a marker line. When a leader
/// is already known, only that leader is accepted.
fn strip_marker(line: &str, known: &Option<String>) -> Option<(String, String)> {
    let leaders: Vec<&str> = match known {
        Some(l) => vec![l.as_str()],
        None => LEADERS.to_vec(),
    };
    for leader in leaders {
        let marker = format!("{leader}@");
        if let Some(rest) = line.strip_prefix(&marker) {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            return Some((leader.to_string(), rest.to_string()));
        }
    }
    None
}

/// Collect the YAML payload from a line iterator: capture and skip a leading
/// shebang, then gather contiguous marker lines, stopping at the first line that
/// is not one. Returns `(interpreter, yaml_block)`, where `interpreter` is the
/// text after `#!` (trimmed) when the first line is a shebang.
fn extract_block<I: Iterator<Item = String>>(mut lines: I) -> (Option<String>, String) {
    let mut current = lines.next();
    let mut interpreter = None;
    if let Some(first) = &current {
        if let Some(rest) = first.strip_prefix("#!") {
            let rest = rest.trim();
            if !rest.is_empty() {
                interpreter = Some(rest.to_string());
            }
            current = lines.next();
        }
    }

    let mut block = String::new();
    let mut leader: Option<String> = None;
    while let Some(line) = current {
        match strip_marker(&line, &leader) {
            Some((found, rest)) => {
                if leader.is_none() {
                    leader = Some(found);
                }
                block.push_str(&rest);
                block.push('\n');
                current = lines.next();
            }
            None => break,
        }
    }
    (interpreter, block)
}

fn parse_block(block: &str) -> Result<FrontMatter, yaml_serde::Error> {
    if block.trim().is_empty() {
        return Ok(FrontMatter::default());
    }
    yaml_serde::from_str(block)
}

/// Merge the captured shebang interpreter into a parsed block.
fn parse_with_interpreter(
    (interpreter, block): (Option<String>, String),
) -> Result<FrontMatter, yaml_serde::Error> {
    let mut front = parse_block(&block)?;
    front.interpreter = interpreter;
    Ok(front)
}

/// Parse front-matter from an in-memory string, surfacing a malformed YAML body
/// as an error.
pub fn try_parse_str(source: &str) -> Result<FrontMatter, yaml_serde::Error> {
    parse_with_interpreter(extract_block(source.lines().map(|l| l.to_string())))
}

/// Parse front-matter from a string, falling back to default front-matter when
/// the YAML body is malformed. Convenience for callers that don't surface the
/// error.
pub fn parse_str(source: &str) -> FrontMatter {
    try_parse_str(source).unwrap_or_default()
}

/// Parse front-matter from a file, reading only the header region (the lazy
/// `lines()` iterator stops being polled once the block ends). Surfaces a read
/// failure or a malformed YAML body as a [`ParseError`].
pub fn parse_file(path: &Path) -> Result<FrontMatter, ParseError> {
    let file = File::open(path).map_err(ParseError::Read)?;
    let reader = BufReader::new(file);
    parse_with_interpreter(extract_block(reader.lines().map_while(Result::ok)))
        .map_err(ParseError::Yaml)
}

// --- usage (`#USAGE`) support ---------------------------------------------

/// Which header style a script uses, decided by its opening marker (the marker
/// sigils are disjoint: `#@` is zub, `#USAGE` is usage). zub's `#@` must open the
/// first comment after the shebang; a `#USAGE` block may follow a preamble.
enum Family {
    Zub,
    Usage,
    /// No recognized marker — treated as a zub command with default docs.
    None,
}

/// Match a usage marker at the start of `line`, returning the sigil it used and
/// the directive text after it. `None` when the line is not a usage directive.
///
/// When `locked` is `Some`, only that exact sigil is accepted — this is how a
/// block enforces a consistent format after its first line fixes one. When
/// `None`, any of the [`USAGE_LEADERS`] × [`USAGE_KEYWORDS`] forms matches.
fn match_usage_marker(line: &str, locked: Option<UsageSigil>) -> Option<(UsageSigil, &str)> {
    if let Some(sigil) = locked {
        return line
            .strip_prefix(sigil.leader)
            .and_then(|rest| rest.strip_prefix(sigil.keyword))
            .map(|content| (sigil, content));
    }
    for leader in USAGE_LEADERS {
        let Some(rest) = line.strip_prefix(leader) else {
            continue;
        };
        for keyword in USAGE_KEYWORDS {
            if let Some(content) = rest.strip_prefix(keyword) {
                return Some((UsageSigil { leader, keyword }, content));
            }
        }
    }
    None
}

/// Whether `line` is a blank comment under `leader` (the leader with only
/// whitespace after). usage tolerates these inside a block, so they don't end
/// it. `None` checks every leader; `Some(l)` only `l` (a locked block continues
/// only under its own leader).
fn is_blank_comment_under(line: &str, leader: Option<&str>) -> bool {
    let leaders: &[&str] = match &leader {
        Some(l) => std::slice::from_ref(l),
        None => &USAGE_LEADERS,
    };
    leaders.iter().any(|leader| {
        line.strip_prefix(leader)
            .is_some_and(|rest| rest.trim().is_empty())
    })
}

/// Whether `line` is a blank comment under any recognized leader.
fn is_blank_comment(line: &str) -> bool {
    is_blank_comment_under(line, None)
}

/// Whether `line` is preamble a `#USAGE` block is allowed to follow: a fully
/// blank line or any comment line (a license header, a stray comment, …).
/// usage-lib finds the block by scanning the *whole* file; we mirror that
/// leniency for the realistic preamble (blanks and comments) but stop at the
/// first line of actual code, so a header-less script is never read past its
/// top — preserving zub's fast-discovery invariant without a whole-file scan.
fn is_usage_preamble(line: &str) -> bool {
    line.trim().is_empty() || USAGE_LEADERS.iter().any(|leader| line.starts_with(leader))
}

/// Whether `line` opens or continues a header block of either family.
fn is_marker(line: &str) -> bool {
    strip_marker(line, &None).is_some() || match_usage_marker(line, None).is_some()
}

/// Advance past a leading shebang line, returning the first line after it (the
/// first comment line, on which classification turns).
fn skip_shebang<'a, I: Iterator<Item = &'a str>>(lines: &mut I) -> Option<&'a str> {
    match lines.next() {
        Some(first) if first.starts_with("#!") => lines.next(),
        other => other,
    }
}

/// Classify a script by the header it opens with.
///
/// zub stays strict: a `#@` block must open on the first comment line after the
/// shebang. usage is lenient, like usage-lib: a `#USAGE` block may follow a
/// blank/comment preamble, so we scan past it — stopping at the first line of
/// real code (where a header-less script ends our scan).
fn classify(source: &str) -> Family {
    let mut lines = source.lines();
    let mut current = skip_shebang(&mut lines);
    // zub's `#@` is authoritative only as the opening comment.
    if current.is_some_and(|line| strip_marker(line, &None).is_some()) {
        return Family::Zub;
    }
    while let Some(line) = current {
        if match_usage_marker(line, None).is_some() {
            return Family::Usage;
        }
        if !is_usage_preamble(line) {
            break;
        }
        current = lines.next();
    }
    Family::None
}

/// Extract the summary (the `about` directive's value) from a usage header.
/// Skips a leading blank/comment preamble to find the `#USAGE` block (mirroring
/// usage-lib), then reads only the contiguous block. `None` when no `about` is
/// declared, or when its value is not a plain double-quoted string (KDL
/// raw/multiline strings are an accepted blind spot — see the design doc).
fn extract_usage_summary(source: &str) -> Option<String> {
    let mut lines = source.lines();
    let mut current = skip_shebang(&mut lines);
    // Advance to the block's opening directive, past any preamble. The sigil it
    // uses (e.g. `# [USAGE]`) is fixed for the rest of the block.
    let sigil = loop {
        let line = current?;
        if let Some((sigil, _)) = match_usage_marker(line, None) {
            break sigil;
        }
        if !is_usage_preamble(line) {
            return None; // code before any usage directive — not a usage header
        }
        current = lines.next();
    };
    // Read the contiguous block, accepting only the locked sigil. A blank
    // comment under the same leader continues it; anything else (code, or a
    // differently-formatted directive) ends it.
    while let Some(line) = current {
        match match_usage_marker(line, Some(sigil)) {
            Some((_, directive)) => {
                if let Some(summary) = summary_from_directive(directive) {
                    return Some(summary);
                }
            }
            None if is_blank_comment_under(line, Some(sigil.leader)) => {}
            None => break,
        }
        current = lines.next();
    }
    None
}

/// If `directive` is an `about "<text>"` directive, return its text.
fn summary_from_directive(directive: &str) -> Option<String> {
    let rest = directive.trim_start().strip_prefix("about")?;
    // Require whitespace after `about` so `about_long`/`aboutx` don't match.
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    first_quoted_string(rest.trim_start())
}

/// Parse a leading double-quoted string, honoring `\"` and `\\` escapes. `None`
/// when `s` does not start with `"` (e.g. a KDL raw string) or is unterminated.
fn first_quoted_string(s: &str) -> Option<String> {
    let mut chars = s.chars();
    if chars.next()? != '"' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for c in chars {
        if escaped {
            out.push(c); // `\"` -> `"`, `\\` -> `\`, anything else kept verbatim
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    None
}

/// Parse a command's header from a string, classifying it as zub or usage by
/// marker sigil. A malformed zub YAML body surfaces as an error; a usage block
/// never errors (a missing/unparsable `about` just yields no summary).
pub fn try_parse_command_str(source: &str) -> Result<CommandMeta, yaml_serde::Error> {
    match classify(source) {
        Family::Usage => Ok(CommandMeta::Usage(UsageMeta {
            summary: extract_usage_summary(source),
        })),
        Family::Zub | Family::None => Ok(CommandMeta::Zub(try_parse_str(source)?)),
    }
}

/// Parse a command's header from a string, falling back to default zub
/// front-matter when the (zub) YAML body is malformed.
pub fn parse_command_str(source: &str) -> CommandMeta {
    try_parse_command_str(source).unwrap_or_default()
}

/// Read only a script's header region — the shebang plus the contiguous run of
/// comment markers (and blank comments) after it — so classification and
/// parsing never read past the front-matter, regardless of file size.
fn read_header(path: &Path) -> io::Result<String> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let mut out = String::new();

    let Some(first) = lines.next().transpose()? else {
        return Ok(out);
    };
    let after_shebang = if first.starts_with("#!") {
        out.push_str(&first);
        out.push('\n');
        match lines.next().transpose()? {
            Some(line) => line,
            None => return Ok(out),
        }
    } else {
        first
    };

    // Find the line that opens the header block. A zub `#@` block must open on
    // the first comment after the shebang. A `#USAGE` block may also follow a
    // blank/comment preamble (usage-lib tolerates it), so we skip that preamble
    // — but stop at the first line of real code, so a header-less script is not
    // read past its top (a bare shebang already pushed above keeps the
    // interpreter). The preamble is dropped: `out` holds the shebang plus the
    // block, which is all classification and extraction need.
    let mut opener = after_shebang;
    if strip_marker(&opener, &None).is_none() {
        loop {
            if match_usage_marker(&opener, None).is_some() {
                break;
            }
            if !is_usage_preamble(&opener) {
                return Ok(out);
            }
            match lines.next().transpose()? {
                Some(line) => opener = line,
                None => return Ok(out),
            }
        }
    }

    out.push_str(&opener);
    out.push('\n');
    for line in lines {
        let line = line?;
        if is_marker(&line) || is_blank_comment(&line) {
            out.push_str(&line);
            out.push('\n');
        } else {
            break;
        }
    }
    Ok(out)
}

/// Parse a command's header from a file, reading only the header region.
/// Surfaces a read failure or a malformed zub YAML body as a [`ParseError`].
pub fn parse_command_file(path: &Path) -> Result<CommandMeta, ParseError> {
    let header = read_header(path).map_err(ParseError::Read)?;
    try_parse_command_str(&header).map_err(ParseError::Yaml)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalars_and_flags() {
        let src = "\
#!/usr/bin/env bash
#@ summary: Check who's logged in
#@ usage: rush who
#@ complete: true

who
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("Check who's logged in"));
        assert_eq!(fm.usage.as_deref(), Some("rush who"));
        assert!(fm.complete);
        assert!(!fm.overrides);
    }

    #[test]
    fn preserves_block_scalar_indentation() {
        let src = "\
#!/usr/bin/env bash
#@ help: |
#@   line one
#@     deeper
";
        let fm = parse_str(src);
        assert_eq!(fm.help.as_deref(), Some("line one\n  deeper\n"));
    }

    #[test]
    fn stops_at_first_non_marker_line() {
        let src = "\
#!/usr/bin/env bash
#@ summary: kept
echo not part of header
#@ usage: ignored
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("kept"));
        assert_eq!(fm.usage, None);
    }

    #[test]
    fn supports_other_comment_leaders() {
        let src = "\
#!/usr/bin/env node
//@ summary: a js command
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("a js command"));
    }

    #[test]
    fn supports_double_semicolon_lisp_leader() {
        // Idiomatic Clojure/Lisp top-level comments use `;;`.
        let src = "\
#!/usr/bin/env bb
;;@ summary: a babashka command
;;@ usage: rush greet
;;@ complete: true
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("a babashka command"));
        assert_eq!(fm.usage.as_deref(), Some("rush greet"));
        assert!(fm.complete);
        assert_eq!(fm.interpreter.as_deref(), Some("/usr/bin/env bb"));
    }

    #[test]
    fn supports_single_semicolon_lisp_leader() {
        let src = "\
#!/usr/bin/env clojure
;@ summary: a clojure command
;@ usage: rush run
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("a clojure command"));
        assert_eq!(fm.usage.as_deref(), Some("rush run"));
    }

    #[test]
    fn lisp_leader_preserves_block_scalar_indentation() {
        let src = "\
#!/usr/bin/env bb
;;@ help: |
;;@   line one
;;@     deeper
";
        let fm = parse_str(src);
        assert_eq!(fm.help.as_deref(), Some("line one\n  deeper\n"));
    }

    #[test]
    fn double_semicolon_leader_is_fixed_once_seen() {
        // The first marker fixes the leader to `;;`, so a following `;@` line is
        // not part of the same header and stops the block.
        let src = "\
;;@ summary: kept
;@ usage: ignored
";
        let fm = parse_str(src);
        assert_eq!(fm.summary.as_deref(), Some("kept"));
        assert_eq!(fm.usage, None);
    }

    #[test]
    fn empty_or_blockless_returns_default() {
        assert_eq!(parse_str(""), FrontMatter::default());
        // A shebang is captured as the interpreter; the rest stays default.
        assert_eq!(
            parse_str("#!/bin/sh\necho hi\n"),
            FrontMatter {
                interpreter: Some("/bin/sh".into()),
                ..Default::default()
            }
        );
    }

    #[test]
    fn malformed_yaml_returns_default() {
        let src = "#@ : : : not yaml\n";
        assert_eq!(parse_str(src), FrontMatter::default());
    }

    #[test]
    fn try_parse_str_surfaces_malformed_yaml() {
        let err = try_parse_str("#@ : : : not yaml\n").unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn try_parse_str_ok_on_valid_block() {
        let fm = try_parse_str("#!/bin/sh\n#@ summary: ok\n").unwrap();
        assert_eq!(fm.summary.as_deref(), Some("ok"));
        assert_eq!(fm.interpreter.as_deref(), Some("/bin/sh"));
    }

    #[test]
    fn override_key_maps_to_overrides() {
        let fm = parse_str("#@ override: true\n");
        assert!(fm.overrides);
    }

    #[test]
    fn eval_key_parses_and_defaults_false() {
        assert!(parse_str("#@ eval: true\n").eval);
        assert!(!parse_str("#@ summary: x\n").eval);
    }

    #[test]
    fn dynamic_help_key_parses_and_defaults_false() {
        assert!(parse_str("#@ dynamic_help: true\n").dynamic_help);
        assert!(!parse_str("#@ summary: x\n").dynamic_help);
    }

    #[test]
    fn captures_shebang_interpreter() {
        let fm = parse_str("#!/usr/bin/env bash\n#@ summary: x\n");
        assert_eq!(fm.interpreter.as_deref(), Some("/usr/bin/env bash"));
    }

    #[test]
    fn no_shebang_means_no_interpreter() {
        let fm = parse_str("#@ summary: x\n");
        assert_eq!(fm.interpreter, None);
    }

    #[test]
    fn interpreter_not_set_from_yaml_key() {
        // `interpreter` is derived from the shebang, never the YAML body.
        let fm = parse_str("#!/bin/sh\n#@ interpreter: /usr/bin/python3\n");
        assert_eq!(fm.interpreter.as_deref(), Some("/bin/sh"));
    }

    // --- usage (`#USAGE`) detection + summary extraction ---

    /// A usage command's summary comes from its `about` directive.
    #[test]
    fn usage_command_extracts_about_summary() {
        let src = "\
#!/usr/bin/env -S usage bash
#USAGE about \"Greet a person, maybe loudly\"
#USAGE flag \"-l --loud\" help=\"Shout\"
#USAGE arg \"<name>\"

echo hi
";
        let meta = parse_command_str(src);
        assert_eq!(
            meta,
            CommandMeta::Usage(UsageMeta {
                summary: Some("Greet a person, maybe loudly".into()),
            })
        );
    }

    #[test]
    fn usage_summary_skips_blank_comments_and_other_directives() {
        // `about` need not be the first directive; blank comments don't end the block.
        let src = "\
#!/usr/bin/env -S usage bash
#USAGE bin \"greet\"
#
#USAGE about \"hi there\"
";
        assert_eq!(extract_usage_summary(src).as_deref(), Some("hi there"));
    }

    #[test]
    fn usage_block_may_follow_a_blank_or_comment_preamble() {
        // usage-lib finds the block anywhere in the file; we tolerate the
        // realistic preamble (blank lines and leading comments) before it.
        let src = "\
#!/usr/bin/env -S usage bash

# a leading comment / license header
#USAGE about \"deferred block\"
";
        assert_eq!(
            parse_command_str(src),
            CommandMeta::Usage(UsageMeta {
                summary: Some("deferred block".into()),
            })
        );
    }

    #[test]
    fn code_before_usage_block_is_not_a_usage_header() {
        // We stop the scan at the first line of real code, so a `#USAGE` block
        // that only appears after code is not detected (bounds discovery cost).
        let src = "#!/usr/bin/env bash\nset -e\n#USAGE about \"too late\"\n";
        assert!(matches!(parse_command_str(src), CommandMeta::Zub(_)));
    }

    #[test]
    fn zub_block_stays_strict_after_a_preamble() {
        // The preamble leniency is usage-only: a `#@` block is still recognized
        // solely as the opening comment after the shebang.
        let src = "#!/bin/sh\n# leading comment\n#@ summary: ignored\n";
        match parse_command_str(src) {
            CommandMeta::Zub(fm) => assert_eq!(fm.summary, None),
            other => panic!("expected default zub, got {other:?}"),
        }
    }

    #[test]
    fn read_header_finds_block_after_preamble_but_stops_at_code() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();

        // A block after a comment/blank preamble is read from the file.
        let deferred = dir.path().join("deferred");
        let mut f = File::create(&deferred).unwrap();
        f.write_all(b"#!/usr/bin/env -S usage bash\n\n# header\n#USAGE about \"found\"\necho hi\n")
            .unwrap();
        assert_eq!(
            parse_command_file(&deferred).unwrap(),
            CommandMeta::Usage(UsageMeta {
                summary: Some("found".into()),
            })
        );

        // Code before the block stops the scan: no usage header is read.
        let buried = dir.path().join("buried");
        let mut f = File::create(&buried).unwrap();
        f.write_all(b"#!/usr/bin/env bash\nset -e\n#USAGE about \"buried\"\n")
            .unwrap();
        assert!(matches!(
            parse_command_file(&buried).unwrap(),
            CommandMeta::Zub(_)
        ));
    }

    #[test]
    fn usage_without_about_has_no_summary() {
        let src = "#!/usr/bin/env -S usage bash\n#USAGE flag \"-l --loud\"\n";
        assert_eq!(
            parse_command_str(src),
            CommandMeta::Usage(UsageMeta { summary: None })
        );
    }

    #[test]
    fn usage_about_unescapes_quotes() {
        assert_eq!(
            summary_from_directive(r#"about "say \"hi\"""#).as_deref(),
            Some(r#"say "hi""#)
        );
    }

    #[test]
    fn usage_raw_string_about_is_a_blind_spot() {
        // A KDL raw-string value is not understood — yields no summary (accepted).
        assert_eq!(summary_from_directive("about r#\"hi\"#"), None);
        // `about_long` must not be mistaken for `about`.
        assert_eq!(summary_from_directive(r#"about_long "x""#), None);
    }

    /// The directive content after each leader × keyword sigil form.
    fn usage_content(line: &str) -> Option<&str> {
        match_usage_marker(line, None).map(|(_, content)| content)
    }

    #[test]
    fn double_slash_and_double_colon_usage_leaders() {
        assert_eq!(usage_content("//USAGE about \"x\""), Some(" about \"x\""));
        assert_eq!(usage_content("::USAGE about \"x\""), Some(" about \"x\""));
        assert_eq!(usage_content("#@ summary: x"), None);
    }

    #[test]
    fn all_sigil_forms_are_recognized() {
        // `#USAGE`, `#[USAGE]`, `# [USAGE]` all introduce a directive and yield
        // the same trailing content (matching usage-lib; bare `# USAGE` does not).
        for line in [
            "#USAGE about \"x\"",
            "#[USAGE] about \"x\"",
            "# [USAGE] about \"x\"",
        ] {
            assert_eq!(usage_content(line), Some(" about \"x\""), "for {line:?}");
        }
        // The spaced bare form is not a usage directive (usage-lib omits it).
        assert_eq!(usage_content("# USAGE about \"x\""), None);
    }

    #[test]
    fn each_sigil_form_extracts_a_summary() {
        for opener in ["#USAGE", "#[USAGE]", "# [USAGE]"] {
            let src = format!("#!/usr/bin/env -S usage bash\n{opener} about \"hi\"\n");
            assert_eq!(
                extract_usage_summary(&src).as_deref(),
                Some("hi"),
                "for opener {opener:?}"
            );
        }
    }

    #[test]
    fn first_sigil_form_locks_the_rest_of_the_block() {
        // The opener uses `# [USAGE]`; a later bare `#USAGE` line is a different
        // sigil, so it ends the block and its `about` is not read.
        let src = "\
#!/usr/bin/env -S usage bash
# [USAGE] bin \"greet\"
#USAGE about \"wrong form\"
";
        assert_eq!(extract_usage_summary(src), None);

        // When every line repeats the locked form, the block reads through.
        let src = "\
#!/usr/bin/env -S usage bash
# [USAGE] bin \"greet\"
# [USAGE] about \"right form\"
";
        assert_eq!(extract_usage_summary(src).as_deref(), Some("right form"));
    }

    /// The two sigils are disjoint: a zub `#@` script is never read as usage,
    /// and a `#USAGE` script is never read as zub YAML.
    #[test]
    fn zub_and_usage_sigils_are_disjoint() {
        match parse_command_str("#!/bin/sh\n#@ summary: a zub command\n") {
            CommandMeta::Zub(fm) => assert_eq!(fm.summary.as_deref(), Some("a zub command")),
            other => panic!("expected zub, got {other:?}"),
        }
        assert!(matches!(
            parse_command_str("#!/usr/bin/env -S usage bash\n#USAGE about \"u\"\n"),
            CommandMeta::Usage(_)
        ));
    }

    #[test]
    fn no_marker_is_default_zub() {
        assert_eq!(
            parse_command_str("#!/bin/sh\necho hi\n"),
            CommandMeta::Zub(FrontMatter {
                interpreter: Some("/bin/sh".into()),
                ..Default::default()
            })
        );
    }

    #[test]
    fn try_parse_command_surfaces_malformed_zub_yaml() {
        assert!(try_parse_command_str("#@ : : : not yaml\n").is_err());
        // A usage block never errors.
        assert!(try_parse_command_str("#USAGE about \"x\"\n").is_ok());
    }

    // --- CommandMeta accessors ---

    #[test]
    fn usage_meta_accessors_disable_zub_only_features() {
        let meta = CommandMeta::Usage(UsageMeta {
            summary: Some("Greet a person".into()),
        });
        assert!(meta.is_usage());
        assert_eq!(meta.summary().as_deref(), Some("Greet a person"));
        assert_eq!(meta.usage(), None); // help (usage line included) is delegated
        assert_eq!(meta.help(), None);
        assert!(!meta.overrides()); // zub-only
        assert!(!meta.eval()); // zub-only
        assert!(meta.wants_completion()); // always, via the usage binary
        assert!(meta.dynamic_help()); // help is always delegated to `--help`
    }

    #[test]
    fn zub_meta_reflects_front_matter_flags() {
        let meta = CommandMeta::Zub(FrontMatter {
            summary: Some("s".into()),
            usage: Some("u".into()),
            help: Some("h".into()),
            complete: true,
            eval: true,
            dynamic_help: true,
            overrides: true,
            ..Default::default()
        });
        assert!(!meta.is_usage());
        assert_eq!(meta.summary().as_deref(), Some("s"));
        assert_eq!(meta.usage().as_deref(), Some("u"));
        assert_eq!(meta.help().as_deref(), Some("h"));
        assert!(meta.overrides());
        assert!(meta.eval());
        assert!(meta.wants_completion());
        assert!(meta.dynamic_help());
    }
}
