use serde::Deserialize;

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
pub struct FrontMatter {
    pub summary: Option<String>,
    pub usage: Option<String>,
    pub help: Option<String>,
    #[serde(default)]
    pub complete: bool,
    #[serde(rename = "override", default)]
    pub overrides: bool,
}

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

const LEADERS: [&str; 4] = ["//", "--", "#", ";"];

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

/// Collect the YAML payload from a line iterator: skip a leading shebang, then
/// gather contiguous marker lines, stopping at the first line that is not one.
fn extract_block<I: Iterator<Item = String>>(mut lines: I) -> String {
    let mut current = lines.next();
    if let Some(first) = &current {
        if first.starts_with("#!") {
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
    block
}

fn parse_block(block: &str) -> FrontMatter {
    if block.trim().is_empty() {
        return FrontMatter::default();
    }
    serde_yaml::from_str(block).unwrap_or_default()
}

/// Parse front-matter from an in-memory string.
pub fn parse_str(source: &str) -> FrontMatter {
    let block = extract_block(source.lines().map(|l| l.to_string()));
    parse_block(&block)
}

/// Parse front-matter from a file, reading only the header region (the lazy
/// `lines()` iterator stops being polled once the block ends).
pub fn parse_file(path: &Path) -> std::io::Result<FrontMatter> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let block = extract_block(reader.lines().map_while(Result::ok));
    Ok(parse_block(&block))
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
        assert_eq!(parse_str("#!/bin/sh\necho hi\n"), FrontMatter::default());
    }

    #[test]
    fn malformed_yaml_returns_default() {
        let src = "#@ : : : not yaml\n";
        assert_eq!(parse_str(src), FrontMatter::default());
    }

    #[test]
    fn override_key_maps_to_overrides() {
        let fm = parse_str("#@ override: true\n");
        assert!(fm.overrides);
    }
}
