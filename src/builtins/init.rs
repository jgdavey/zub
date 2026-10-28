use crate::builtins::Context;
use crate::index::Resolution;
use std::collections::BTreeMap;

/// Render the shell-integration script. `eval_commands` are the name
/// components of commands whose front-matter sets `eval: true`; the wrapper
/// `eval`s their stdout instead of running them normally.
pub fn render_init(ctx: &Context, shell: &str, eval_commands: &[Vec<String>]) -> String {
    let prog = &ctx.identity.name;
    let root = ctx.identity.root.to_string_lossy();
    let config = ctx.identity.config_path.to_string_lossy();

    let mut out = String::new();
    out.push_str(&format!("export PATH=\"${{PATH}}:{root}/bin\"\n"));

    match shell {
        "bash" => {
            out.push_str(&format!("source \"{root}/completions/{prog}.bash\"\n"));
        }
        "zsh" => {
            out.push_str(&format!("fpath=($fpath {root}/completions)\n"));
            out.push_str(&format!("autoload -U _{prog} _zub\n"));
            out.push_str(&format!("compdef _{prog} {prog}\n"));
        }
        _ => {}
    }

    let mut tree = EvalTree::default();
    for components in eval_commands {
        tree.insert(components);
    }
    let calls = Calls {
        run: format!("zub -C \"{config}\" \"$@\""),
        eval: format!(
            "local evaluate\n\
             evaluate=\"$(zub -C \"{config}\" \"$@\")\" && eval \"${{evaluate}}\""
        ),
    };
    out.push_str(&format!("_{prog}_wrapper() {{\n"));
    out.push_str(&indent(&tree.render(1, &calls), "  "));
    out.push_str("}\n");

    match shell {
        "bash" => out.push_str(&format!("alias {prog}=_{prog}_wrapper\n")),
        "zsh" => out.push_str(&format!("{prog}() {{ _{prog}_wrapper $@ }}\n")),
        _ => {}
    }

    out
}

/// The wrapper's two ways to run a command: plainly, or `eval`ing its stdout.
struct Calls {
    run: String,
    eval: String,
}

/// The eval commands as a tree of name components: a node with children is a
/// namespace, a node without is an eval command.
#[derive(Default)]
struct EvalTree(BTreeMap<String, EvalTree>);

impl EvalTree {
    fn insert(&mut self, components: &[String]) {
        if let Some((head, rest)) = components.split_first() {
            self.0.entry(head.clone()).or_default().insert(rest);
        }
    }

    /// A `case` on positional arg `depth` (1-based): an eval command `eval`s,
    /// a namespace recurses on the next arg, anything else runs plainly.
    fn render(&self, depth: usize, calls: &Calls) -> String {
        if self.0.is_empty() {
            return format!("{}\n", calls.run);
        }
        let mut out = format!("case \"${{{depth}}}\" in\n");
        for (name, child) in &self.0 {
            let body = if child.0.is_empty() {
                format!("{}\n", calls.eval)
            } else {
                child.render(depth + 1, calls)
            };
            out.push_str(&format!("  {})\n", shell_quote(name)));
            out.push_str(&indent(&body, "    "));
            out.push_str("    ;;\n");
        }
        out.push_str(&format!("  *)\n    {}\n    ;;\nesac\n", calls.run));
        out
    }
}

/// Single-quote `s` so a `case` pattern matches it literally.
fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines().map(|l| format!("{prefix}{l}\n")).collect()
}

/// Name components of every command declaring `eval: true` that its own name
/// actually dispatches to (a built-in can shadow a top-level command).
fn eval_commands(ctx: &Context) -> Vec<Vec<String>> {
    ctx.index
        .leaves()
        .into_iter()
        .filter(|c| c.meta.eval())
        .filter(|c| {
            matches!(ctx.index.resolve(&c.components),
                     Resolution::Command { command, .. } if command.path == c.path)
        })
        .map(|c| c.components.clone())
        .collect()
}

pub fn complete(_args: &[String], _ctx: &Context) -> i32 {
    0
}

pub fn run(args: &[String], ctx: &Context) -> i32 {
    let prog = &ctx.identity.name;
    let mut iter = args.iter();
    let print = matches!(iter.next().map(String::as_str), Some("-"));
    let shell = iter
        .next()
        .cloned()
        .or_else(|| {
            std::env::var("SHELL").ok().map(|s| {
                std::path::Path::new(&s)
                    .file_name()
                    .map(|f| f.to_string_lossy().into_owned())
                    .unwrap_or(s)
            })
        })
        .unwrap_or_default();

    if !print {
        let profile = match shell.as_str() {
            "bash" => "~/.bash_profile",
            "zsh" => "~/.zshrc",
            _ => "your profile",
        };
        eprintln!("# Load {prog} automatically by adding");
        eprintln!("# the following to {profile}:");
        eprintln!();
        eprintln!(
            "eval \"$({}/bin/{prog} init -)\"",
            ctx.identity.root.to_string_lossy()
        );
        eprintln!();
        // Informational, not an error: this is `init`'s default output.
        return 0;
    }

    print!("{}", render_init(ctx, &shell, &eval_commands(ctx)));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_meta::FrontMatter;
    use crate::identity::{fixture, Identity};
    use crate::index::{self, Index};

    fn ctx() -> (Identity, Index) {
        (fixture("rush", "/opt/rush"), Index::default())
    }

    #[test]
    fn exports_root_and_path() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &[]);
        assert!(script.contains("export PATH=\"${PATH}:/opt/rush/bin\""));
    }

    #[test]
    fn bash_emits_completion_source_and_alias() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &[]);
        assert!(script.contains("/opt/rush/completions/rush.bash"));
        assert!(script.contains("alias rush=_rush_wrapper"));
    }

    #[test]
    fn zsh_emits_fpath_and_function() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let script = render_init(&ctx, "zsh", &[]);
        assert!(script.contains("fpath=($fpath /opt/rush/completions)"));
        assert!(script.contains("rush() { _rush_wrapper $@ }"));
    }

    fn names(names: &[&str]) -> Vec<Vec<String>> {
        names
            .iter()
            .map(|n| n.split(' ').map(String::from).collect())
            .collect()
    }

    #[test]
    fn eval_wrapper_evals_top_level_command() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &names(&["cd"]));
        assert!(script.contains("case \"${1}\" in\n    'cd')\n      local evaluate\n"));
        assert!(script.contains("evaluate=\"$(zub -C \"/opt/rush/zub.yml\" \"$@\")\""));
    }

    #[test]
    fn eval_wrapper_matches_nested_command_on_each_arg() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &names(&["db cd", "db x y"]));
        assert!(script.contains("    'db')\n      case \"${2}\" in\n        'cd')\n"));
        assert!(script.contains("        'x')\n          case \"${3}\" in\n            'y')\n"));
        // Only `db cd` evals; a bare `cd` falls through to a plain run.
        assert!(!script.contains("\n    'cd')"));
    }

    #[test]
    fn eval_wrapper_quotes_names() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &names(&["it's", "*"]));
        assert!(script.contains("'it'\\''s')"));
        assert!(script.contains("'*')"));
    }

    #[test]
    fn wrapper_without_eval_commands_runs_plainly() {
        let (id, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &[]);
        assert!(script.contains("_rush_wrapper() {\n  zub -C \"/opt/rush/zub.yml\" \"$@\"\n}\n"));
    }

    #[test]
    fn eval_commands_are_full_names_not_shadowed_by_builtins() {
        let eval = FrontMatter {
            eval: true,
            ..Default::default()
        };
        let id = fixture("rush", "/opt/rush");
        let cmds = Index::from_leaves(vec![
            index::leaf("db cd", eval.clone()),
            index::leaf("help", eval.clone()),
            index::leaf("who", FrontMatter::default()),
        ]);
        let ctx = Context {
            identity: &id,
            index: &cmds,
        };
        assert_eq!(eval_commands(&ctx), names(&["db cd"]));
    }
}
