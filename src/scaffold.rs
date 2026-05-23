use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::config::Config;

/// Create a new zub program tree at `target`: `zub.yml` (with `root` omitted),
/// an executable self-locating `bin/<name>` shim, and empty
/// `libexec`/`completions`/`share` directories.
pub fn create_program(target: &Path, name: &str) -> io::Result<()> {
    if target.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", target.display()),
        ));
    }
    fs::create_dir_all(target.join("bin"))?;
    fs::create_dir_all(target.join("libexec"))?;
    fs::create_dir_all(target.join("completions"))?;
    fs::create_dir_all(target.join("share"))?;

    let config = Config {
        name: name.to_string(),
        root: None,
        version: None,
        description: Some("your description".to_string()),
    };
    let config_file = fs::File::create(target.join("zub.yml"))?;
    serde_yaml::to_writer(io::BufWriter::new(config_file), &config)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

    let shim = "#!/bin/sh\n\
                here=\"$(cd \"$(dirname \"$0\")/..\" && pwd)\"\n\
                exec zub -C \"$here/zub.yml\" \"$@\"\n";
    let shim_path = target.join("bin").join(name);
    fs::write(&shim_path, shim)?;
    fs::set_permissions(&shim_path, fs::Permissions::from_mode(0o755))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_program_tree() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        assert!(target.join("zub.yml").exists());
        assert!(target.join("libexec").is_dir());
        assert!(target.join("completions").is_dir());
        assert!(target.join("share").is_dir());

        let cfg = fs::read_to_string(target.join("zub.yml")).unwrap();
        assert!(cfg.contains("name: rush"));
        assert!(!cfg.contains("root"));
    }

    #[test]
    fn writes_executable_self_locating_shim() {
        let work = tempdir().unwrap();
        let target = work.path().join("rush");
        create_program(&target, "rush").unwrap();

        let shim_path = target.join("bin").join("rush");
        let shim = fs::read_to_string(&shim_path).unwrap();
        assert!(shim.contains("exec zub -C \"$here/zub.yml\" \"$@\""));

        let mode = fs::metadata(&shim_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111, "shim should be executable");
    }

    #[test]
    fn refuses_existing_directory() {
        let work = tempdir().unwrap();
        let target = work.path().join("taken");
        fs::create_dir(&target).unwrap();
        assert!(create_program(&target, "taken").is_err());
    }
}
