use super::*;
use std::error::Error;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::process::Command;

type TestResult = Result<(), Box<dyn Error>>;

#[test]
fn installed_tar_and_gtar_aliases_execute_the_verified_helper() -> TestResult {
    let (directory, executable, digest) = staged_script()?;
    let aliases = fs::canonicalize(directory.path().join("velnor/bin"))?;
    install_aliases(&executable, &aliases, &digest).map_err(std::io::Error::other)?;

    for alias in ALIAS_NAMES {
        let output = Command::new(aliases.join(alias))
            .arg("--version")
            .output()?;
        assert!(output.status.success());
        assert_eq!(output.stdout, format!("{alias}\n").as_bytes());
    }
    let source = fs::metadata(&executable)?;
    let tar = fs::metadata(aliases.join("tar"))?;
    let gtar = fs::metadata(aliases.join("gtar"))?;
    assert_eq!(source.ino(), tar.ino());
    assert_eq!(source.ino(), gtar.ino());
    assert_eq!(source.dev(), tar.dev());
    assert_eq!(source.dev(), gtar.dev());
    Ok(())
}

#[test]
fn collision_preserves_foreign_alias_and_removes_only_our_partial_link() -> TestResult {
    let (directory, executable, digest) = staged_script()?;
    let aliases = fs::canonicalize(directory.path().join("velnor/bin"))?;
    let foreign = aliases.join("gtar");
    fs::write(&foreign, b"foreign executable")?;
    let before = fs::read(&foreign)?;

    assert!(install_aliases(&executable, &aliases, &digest).is_err());
    assert_eq!(fs::read(&foreign)?, before);
    assert!(!aliases.join("tar").exists());
    assert_eq!(fs::metadata(&executable)?.nlink(), 1);
    Ok(())
}

#[test]
fn digest_mismatch_leaves_both_aliases_absent() -> TestResult {
    let (directory, executable, _) = staged_script()?;
    let aliases = fs::canonicalize(directory.path().join("velnor/bin"))?;

    assert_eq!(
        install_aliases(&executable, &aliases, &"0".repeat(64)),
        Err("cache_tar_launcher_digest_mismatch")
    );
    assert!(!aliases.join("tar").exists());
    assert!(!aliases.join("gtar").exists());
    Ok(())
}

fn staged_script() -> Result<(tempfile::TempDir, std::path::PathBuf, String), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let aliases = directory.path().join("velnor/bin");
    fs::create_dir_all(&aliases)?;
    let aliases = fs::canonicalize(aliases)?;
    let executable = aliases.join("velnor-actions-0.1.0");
    let contents = b"#!/bin/sh\nprintf '%s\\n' \"${0##*/}\"\n";
    fs::write(&executable, contents)?;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    let digest = lower_hex(&Sha256::digest(contents));
    Ok((directory, executable, digest))
}
