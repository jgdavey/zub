use crate::builtins;
use crate::builtins::Context;
use crate::dispatch::BUILTINS;
use std::env;
use std::os::unix::process::CommandExt;
use std::process::Command;

/// Completion words after the command, plus the COMP_* values they imply.
#[derive(Debug, PartialEq)]
pub struct CompWords {
    pub words: Vec<String>,
    pub last: String,
    pub penult: Option<String>,
}

/// Compute the completion words and the COMP_LASTARG / COMP_PENULT they imply.
/// When `COMP_WORD` is unset/empty, an empty trailing word is appended (the
/// user is starting a fresh argument).
pub fn comp_words(args: &[String], comp_word: Option<String>) -> CompWords {
    let mut words: Vec<String> = args.to_vec();
    if comp_word.as_deref().unwrap_or("").is_empty() {
        words.push(String::new());
    }
    let last = words.last().cloned().unwrap_or_default();
    let penult = if words.len() > 1 {
        Some(words[words.len() - 2].clone())
    } else {
        None
    };
    CompWords { words, last, penult }
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    if args.first().map(String::as_str) == Some("--commands") {
        return print_summaries(ctx);
    }

    let Some(command) = args.first() else {
        eprintln!("usage: {} completions command [arg1 arg2...]", ctx.identity.name);
        return 1;
    };
    let rest = &args[1..];

    // Built-in completion runs in-process.
    if BUILTINS.contains(&command.as_str())
        && !ctx.commands.iter().any(|c| &c.name == command && c.front.overrides)
    {
        let mut a = vec!["--complete".to_string()];
        a.extend_from_slice(rest);
        return builtins::run(command, &a, ctx);
    }

    // External command: only those declaring `complete: true` participate.
    let Some(info) = ctx.commands.iter().find(|c| &c.name == command) else {
        return 42; // unknown command -> generic fallback
    };
    if !info.front.complete {
        return 42; // not completion-capable -> generic fallback
    }

    let comp_word = env::var("COMP_WORD").ok();
    let cw = comp_words(rest, comp_word);
    env::set_var("COMP_LASTARG", &cw.last);
    env::set_var("COMP_PENULT", cw.penult.unwrap_or_default());

    let mut exec_args = vec!["--complete".to_string()];
    exec_args.extend(cw.words);
    let err = Command::new(&info.path).args(&exec_args).exec();
    eprintln!("{}: failed to exec completion: {err}", ctx.identity.name);
    1
}

/// zsh-style `name[summary]` lines for top-level command completion.
fn print_summaries(ctx: &Context) -> i32 {
    for name in builtins::all_command_names(ctx) {
        let summary = ctx
            .commands
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.front.summary.clone())
            .or_else(|| {
                builtins::BUILTIN_DOCS
                    .iter()
                    .find(|b| b.name == name)
                    .map(|b| b.summary.to_string())
            });
        match summary {
            Some(s) => println!("{name}[{s}]"),
            None => println!("{name}"),
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_empty_word_when_comp_word_unset() {
        let cw = comp_words(&["sub".to_string()], None);
        assert_eq!(cw.words, vec!["sub".to_string(), "".to_string()]);
        assert_eq!(cw.last, "");
        assert_eq!(cw.penult.as_deref(), Some("sub"));
    }

    #[test]
    fn uses_words_as_is_when_comp_word_set() {
        let cw = comp_words(&["sub".to_string(), "fo".to_string()], Some("fo".to_string()));
        assert_eq!(cw.words, vec!["sub".to_string(), "fo".to_string()]);
        assert_eq!(cw.last, "fo");
        assert_eq!(cw.penult.as_deref(), Some("sub"));
    }

    #[test]
    fn single_word_has_no_penult() {
        let cw = comp_words(&[], Some("x".to_string()));
        assert_eq!(cw.penult, None);
    }
}
