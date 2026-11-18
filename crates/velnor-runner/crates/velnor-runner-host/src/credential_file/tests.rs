// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use super::load;
use crate::config::KeychainReference;

type R = Result<(), Box<dyn std::error::Error>>;

const TOKEN: &str = "ghp_FileCredentialSecret0123456789";

fn write(dir: &Path, name: &str, body: &str, mode: u32) -> std::io::Result<std::path::PathBuf> {
    let path = dir.join(name);
    std::fs::write(&path, body)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))?;
    Ok(path)
}

#[test]
fn mode_0600_owned_file_loads_and_strips_newline() -> R {
    let dir = tempfile::tempdir()?;
    let path = write(dir.path(), "token", &format!("{TOKEN}\n"), 0o600)?;
    assert_eq!(load(&path)?.as_slice(), TOKEN.as_bytes());
    Ok(())
}

#[test]
fn wrong_modes_are_refused_without_leaking_the_value() -> R {
    let dir = tempfile::tempdir()?;
    for mode in [0o644, 0o640, 0o604, 0o660, 0o400, 0o700, 0o777, 0o000] {
        let path = write(dir.path(), &format!("m{mode:o}"), TOKEN, mode)?;
        let Err(error) = load(&path) else {
            return Err("accepted".into());
        };
        assert!(
            !format!("{error} {error:?}").contains(TOKEN),
            "mode {mode:o}"
        );
    }
    Ok(())
}

#[test]
fn missing_file_and_directory_are_refused() -> R {
    let dir = tempfile::tempdir()?;
    assert!(load(&dir.path().join("absent")).is_err());
    assert!(load(dir.path()).is_err());
    Ok(())
}

#[test]
fn symlink_is_refused_even_when_target_is_valid() -> R {
    let dir = tempfile::tempdir()?;
    let target = write(dir.path(), "real", TOKEN, 0o600)?;
    let link = dir.path().join("link");
    symlink(&target, &link)?;
    assert!(load(&link).is_err());
    Ok(())
}

#[test]
fn empty_oversize_and_non_token_content_are_refused_without_leaking() -> R {
    let dir = tempfile::tempdir()?;
    let big = format!("{TOKEN}{}", "a".repeat(5000));
    for (name, body) in [
        ("empty", String::new()),
        ("big", big),
        ("space", format!("{TOKEN} tail")),
    ] {
        let path = write(dir.path(), name, &body, 0o600)?;
        let Err(error) = load(&path) else {
            return Err("accepted".into());
        };
        assert!(!format!("{error} {error:?}").contains(TOKEN), "{name}");
    }
    Ok(())
}

#[test]
fn reference_round_trips_and_never_prints_the_value() -> R {
    let dir = tempfile::tempdir()?;
    let path = write(dir.path(), "token", TOKEN, 0o600)?;
    let text = format!("file:{}", path.display());
    let reference = KeychainReference::parse(&text)?;
    assert_eq!(reference.to_string(), text);
    assert!(reference.keychain_parts().is_none());
    assert!(!format!("{reference:?} {reference}").contains(TOKEN));
    assert_eq!(reference.load()?.as_slice(), TOKEN.as_bytes());
    Ok(())
}

#[test]
fn reference_load_fails_closed_on_bad_mode() -> R {
    let dir = tempfile::tempdir()?;
    let path = write(dir.path(), "token", TOKEN, 0o644)?;
    let reference = KeychainReference::parse(&format!("file:{}", path.display()))?;
    assert!(reference.load().is_err());
    Ok(())
}

#[test]
fn relative_traversal_and_empty_file_references_do_not_parse() {
    for text in [
        "file:",
        "file:relative/token",
        "file:/a/../etc/token",
        "file:/",
    ] {
        assert!(KeychainReference::parse(text).is_err(), "{text}");
    }
    assert!(KeychainReference::parse("keychain:svc/acct").is_ok());
}
