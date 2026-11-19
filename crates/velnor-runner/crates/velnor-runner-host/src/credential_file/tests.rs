// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use super::{Identity, load, permits};
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
    for mode in [
        0o644, 0o604, 0o660, 0o666, 0o700, 0o777, 0o000, 0o200, 0o1600, 0o2640, 0o4600,
    ] {
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
fn symlink_to_a_valid_file_is_followed_and_the_target_is_checked() -> R {
    let dir = tempfile::tempdir()?;
    let target = write(dir.path(), "real", TOKEN, 0o440)?;
    let link = dir.path().join("link");
    symlink(&target, &link)?;
    assert_eq!(load(&link)?.as_slice(), TOKEN.as_bytes());

    let loose = write(dir.path(), "loose", TOKEN, 0o644)?;
    let bad = dir.path().join("bad-link");
    symlink(&loose, &bad)?;
    assert!(load(&bad).is_err());

    let dangling = dir.path().join("dangling");
    symlink(dir.path().join("gone"), &dangling)?;
    assert!(load(&dangling).is_err());
    Ok(())
}

#[test]
fn kubelet_secret_volume_layout_loads() -> R {
    let dir = tempfile::tempdir()?;
    let stamp = dir.path().join("..2026_11_19");
    std::fs::create_dir(&stamp)?;
    write(&stamp, "token", TOKEN, 0o440)?;
    symlink("..2026_11_19", dir.path().join("..data"))?;
    symlink("..data/token", dir.path().join("token"))?;
    assert_eq!(
        load(&dir.path().join("token"))?.as_slice(),
        TOKEN.as_bytes()
    );
    Ok(())
}

#[test]
fn group_readable_0440_and_0400_files_owned_by_the_caller_load() -> R {
    let dir = tempfile::tempdir()?;
    for mode in [0o440, 0o400, 0o640] {
        let path = write(dir.path(), &format!("ok{mode:o}"), TOKEN, mode)?;
        assert_eq!(load(&path)?.as_slice(), TOKEN.as_bytes(), "{mode:o}");
    }
    Ok(())
}

fn pod() -> Identity {
    Identity {
        euid: 10001,
        egid: 10001,
        groups: vec![10001, 20000],
    }
}

#[test]
fn permits_root_owned_secret_volume_files_in_the_pods_group() {
    assert!(permits(0o100_440, 0, 10001, &pod()));
    assert!(permits(0o100_440, 0, 20000, &pod()));
    assert!(permits(0o100_600, 10001, 5, &pod()));
    assert!(permits(0o100_400, 10001, 5, &pod()));
    let supplementary = Identity {
        euid: 10001,
        egid: 1,
        groups: vec![10001],
    };
    assert!(permits(0o100_440, 0, 10001, &supplementary));
}

#[test]
fn permits_refuses_other_users_foreign_groups_and_loose_modes() {
    assert!(!permits(0o100_600, 4242, 10001, &pod()));
    assert!(!permits(0o100_440, 4242, 10001, &pod()));
    assert!(!permits(0o100_440, 0, 4242, &pod()));
    assert!(!permits(0o100_640, 10001, 4242, &pod()));
    for mode in [
        0o644, 0o604, 0o660, 0o664, 0o666, 0o001, 0o770, 0o2640, 0o4640,
    ] {
        assert!(!permits(0o100_000 | mode, 0, 10001, &pod()), "{mode:o}");
        assert!(!permits(0o100_000 | mode, 10001, 10001, &pod()), "{mode:o}");
    }
    assert!(!permits(0o100_000, 10001, 10001, &pod()));
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

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
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

#[cfg(not(target_os = "linux"))]
#[test]
fn file_references_are_rejected_outside_linux() {
    assert!(KeychainReference::parse("file:/run/secrets/token").is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn a_file_with_an_extended_acl_is_refused() -> R {
    let dir = tempfile::tempdir()?;
    let path = write(dir.path(), "token", TOKEN, 0o640)?;
    let mut acl = 2_u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (1_u16, 6_u16, u32::MAX),
        (2, 4, 12345),
        (4, 4, u32::MAX),
        (0x10, 4, u32::MAX),
        (0x20, 0, u32::MAX),
    ] {
        acl.extend(tag.to_le_bytes());
        acl.extend(perm.to_le_bytes());
        acl.extend(id.to_le_bytes());
    }
    let file = std::fs::File::open(&path)?;
    if rustix::fs::fsetxattr(
        &file,
        "system.posix_acl_access",
        &acl,
        rustix::fs::XattrFlags::empty(),
    )
    .is_err()
    {
        return Ok(());
    }
    assert!(load(&path).is_err());
    Ok(())
}
