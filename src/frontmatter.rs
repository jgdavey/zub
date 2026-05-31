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

const LEADERS: [&str; 4] = ["//", "--", "#", ";"];

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
}
