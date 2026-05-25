use crate::builtins::commands;
use crate::builtins::Context;

/// Render the shell-integration script. `eval_commands` are the names of
/// commands whose front-matter sets `eval: true`; the wrapper `eval`s their
/// stdout instead of running them normally.
pub fn render_init(ctx: &Context, shell: &str, eval_commands: &[String]) -> String {
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

    let cases = if eval_commands.is_empty() {
        String::from("NO_EVAL_COMMANDS")
    } else {
        eval_commands.join("|")
    };
    out.push_str(&format!(
        "_{prog}_wrapper() {{\n\
         \x20 local command=\"$1\"\n\
         \x20 local evaluate=\n\
         \x20 if [ \"$#\" -gt 0 ]; then shift; fi\n\
         \x20 case \"$command\" in\n\
         \x20 {cases})\n\
         \x20   evaluate=`zub -C \"{config}\" \"$command\" \"$@\"` && eval \"${{evaluate}}\" ;;\n\
         \x20 *)\n\
         \x20   zub -C \"{config}\" \"$command\" \"$@\";;\n\
         \x20 esac\n\
         }}\n"
    ));

    match shell {
        "bash" => out.push_str(&format!("alias {prog}=_{prog}_wrapper\n")),
        "zsh" => out.push_str(&format!("{prog}() {{ _{prog}_wrapper $@ }}\n")),
        _ => {}
    }

    out
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
        return 1;
    }

    // Names of commands declaring `eval: true`, for the wrapper.
    let eval_commands = commands::collect(&["--eval".to_string()], ctx);
    print!("{}", render_init(ctx, &shell, &eval_commands));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::identity::Identity;
    use crate::index::Index;
    use std::path::PathBuf;

    fn ctx() -> (Identity, Option<Config>, Index) {
        let id = Identity {
            name: "rush".into(),
            root: PathBuf::from("/opt/rush"),
            local_root: None,
            config_path: PathBuf::from("/opt/rush/zub.yml"),
        };
        (id, None, Index::default())
    }

    #[test]
    fn exports_root_and_path() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &[]);
        assert!(script.contains("export PATH=\"${PATH}:/opt/rush/bin\""));
    }

    #[test]
    fn bash_emits_completion_source_and_alias() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &[]);
        assert!(script.contains("/opt/rush/completions/rush.bash"));
        assert!(script.contains("alias rush=_rush_wrapper"));
    }

    #[test]
    fn zsh_emits_fpath_and_function() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let script = render_init(&ctx, "zsh", &[]);
        assert!(script.contains("fpath=($fpath /opt/rush/completions)"));
        assert!(script.contains("rush() { _rush_wrapper $@ }"));
    }

    #[test]
    fn eval_wrapper_lists_eval_commands() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &["cd".to_string(), "push".to_string()]);
        assert!(script.contains("cd|push)"));
    }

    #[test]
    fn wrapper_evals_command_without_sh_prefix() {
        let (id, cfg, cmds) = ctx();
        let ctx = Context {
            identity: &id,
            config: &cfg,
            index: &cmds,
        };
        let script = render_init(&ctx, "bash", &["cd".to_string()]);
        assert!(script.contains("zub -C \"/opt/rush/zub.yml\" \"$command\""));
        assert!(!script.contains("sh-$command"));
    }
}
