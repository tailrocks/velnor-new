use super::super::super::Scratch;
use super::{find_executable_in_path, write_executable};
use std::env;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[test]
fn relative_path_skips_non_executables_and_returns_absolute_target() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new("relative-path")?;
    let blocked = scratch.path().join("blocked");
    let tools = scratch.path().join("tools");
    fs::create_dir_all(&blocked)?;
    fs::create_dir_all(&tools)?;

    let non_executable = blocked.join("python3");
    fs::write(&non_executable, "not executable")?;
    fs::set_permissions(&non_executable, fs::Permissions::from_mode(0o644))?;
    let executable = tools.join("python3");
    write_executable(&executable, "#!/bin/sh\nexit 0\n")?;

    let search_path = env::join_paths([Path::new("blocked"), Path::new("tools")])?;
    let resolved = find_executable_in_path("python3", &search_path, scratch.path())?;

    assert_eq!(resolved, executable.canonicalize()?);
    assert!(resolved.is_absolute());
    Ok(())
}
