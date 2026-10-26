// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use velnor_actions_orchestrator::{
    CheckoutState, EntryKind, ProjectionEntry, RealMtime, RestoreDecision, authenticate_entry,
    decide, install_staged_file, install_staged_symlink, is_git_path, reconcile,
    restore_directory_times, restore_times, sha256_hex, sort_deepest_first, staged_path,
    validate_link_target, validate_link_target_in_root, validate_relative_path, validate_sha256,
    verify_times,
};

const ARCHIVED_SECONDS: i64 = 946_684_800;
const ARCHIVED_NANOS: u32 = 123_456_789;

fn mtime() -> RealMtime {
    RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: ARCHIVED_NANOS,
    }
}

fn file_entry(path: &str, bytes: &[u8], mode: u32) -> ProjectionEntry {
    ProjectionEntry {
        path: path.to_owned(),
        kind: EntryKind::File,
        mode,
        content_sha256: sha256_hex(bytes),
        link_target: None,
        mtime: Some(mtime()),
    }
}

fn dir_entry(path: &str, mode: u32, stamp: RealMtime) -> ProjectionEntry {
    ProjectionEntry {
        path: path.to_owned(),
        kind: EntryKind::Directory,
        mode,
        content_sha256: sha256_hex(&[]),
        link_target: None,
        mtime: Some(stamp),
    }
}

fn link_entry(path: &str, target: &str) -> ProjectionEntry {
    ProjectionEntry {
        path: path.to_owned(),
        kind: EntryKind::Symlink,
        mode: 0o777,
        content_sha256: sha256_hex(target.as_bytes()),
        link_target: Some(target.to_owned()),
        mtime: Some(mtime()),
    }
}

fn write_checkout_file(
    root: &Path,
    path: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<PathBuf, Box<dyn Error>> {
    let full = root.join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full, bytes)?;
    fs::set_permissions(&full, fs::Permissions::from_mode(mode))?;
    Ok(full)
}

fn read_mtime(path: &Path) -> Result<(i64, u32), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    let modified = metadata.modified()?;
    let elapsed = modified.duration_since(std::time::UNIX_EPOCH)?;
    let seconds = i64::try_from(elapsed.as_secs())
        .map_err(|_| io::Error::other("mtime seconds out of range"))?;
    Ok((seconds, elapsed.subsec_nanos()))
}

fn check_semantic_ignores_mtime() -> Result<(), Box<dyn Error>> {
    let bytes = b"projection bytes";
    let first = file_entry("src/lib.rs", bytes, 0o644);
    first.validate()?;
    let mut second = first.clone();
    second.mtime = Some(RealMtime {
        seconds: 1_700_000_000,
        nanoseconds: 1,
    });
    second.validate()?;
    if !first.semantic_matches(&second) {
        return Err(io::Error::other("semantic match must ignore mtime").into());
    }
    let mut changed = first.clone();
    changed.content_sha256 = sha256_hex(b"other bytes");
    if first.semantic_matches(&changed) {
        return Err(io::Error::other("semantic match missed content change").into());
    }
    if EntryKind::parse("file")? != EntryKind::File
        || EntryKind::parse("directory")? != EntryKind::Directory
        || EntryKind::parse("symlink")? != EntryKind::Symlink
    {
        return Err(io::Error::other("entry kind parse drifted").into());
    }
    Ok(())
}

fn check_unchanged_file_roundtrip() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let bytes = b"unchanged workspace input\n";
    let declared = file_entry("src/lib.rs", bytes, 0o644);
    write_checkout_file(&checkout, "src/lib.rs", bytes, 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
        return Err(io::Error::other("matching checkout rejected").into());
    }
    let staged_file = write_checkout_file(&staged, "src/lib.rs", bytes, 0o644)?;
    reconcile(&declared, &declared)?;
    if decide(Some(&declared), Some(&declared), CheckoutState::Unchanged)
        != RestoreDecision::Restore
    {
        return Err(io::Error::other("unchanged input not scheduled").into());
    }
    let destination = staged_path(&checkout, "src/lib.rs")?;
    let outcome = install_staged_file(&staged_file, &destination, &declared)?;
    if !outcome.mtime_restored {
        return Err(io::Error::other("file mtime not restored").into());
    }
    if fs::read(&destination)? != bytes {
        return Err(io::Error::other("restored bytes drifted").into());
    }
    verify_times(&destination, &mtime())?;
    if read_mtime(&destination)? != (ARCHIVED_SECONDS, ARCHIVED_NANOS) {
        return Err(io::Error::other("nanoseconds not preserved").into());
    }
    if staged_file.exists() {
        return Err(io::Error::other("staged file not moved").into());
    }
    Ok(())
}

fn check_directories_restore_after_children() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let bytes = b"nested input\n";
    let declared = file_entry("pkg/a/b/input.txt", bytes, 0o644);
    write_checkout_file(&checkout, "pkg/a/b/input.txt", bytes, 0o644)?;
    let staged_file = write_checkout_file(&staged, "pkg/a/b/input.txt", bytes, 0o644)?;
    let parent_stamp = RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: 111_111_111,
    };
    let child_stamp = RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: 222_222_222,
    };
    let parents = vec![
        (checkout.join("pkg"), parent_stamp),
        (checkout.join("pkg/a"), parent_stamp),
        (checkout.join("pkg/a/b"), child_stamp),
    ];
    restore_directory_times(&parents)?;
    let destination = staged_path(&checkout, "pkg/a/b/input.txt")?;
    install_staged_file(&staged_file, &destination, &declared)?;
    if read_mtime(&checkout.join("pkg/a/b"))? == (ARCHIVED_SECONDS, 222_222_222) {
        return Err(io::Error::other("child install must disturb parent mtime").into());
    }
    restore_directory_times(&parents)?;
    verify_times(&checkout.join("pkg"), &parent_stamp)?;
    verify_times(&checkout.join("pkg/a"), &parent_stamp)?;
    verify_times(&checkout.join("pkg/a/b"), &child_stamp)?;
    Ok(())
}

fn check_changed_and_git_inputs_stay_current() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    fs::create_dir_all(&checkout)?;
    let declared = file_entry("build/input.txt", b"archived", 0o644);
    let current = write_checkout_file(&checkout, "build/input.txt", b"changed", 0o644)?;
    let before = read_mtime(&current)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(io::Error::other("changed checkout accepted").into());
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Changed)
        != RestoreDecision::KeepCurrent
    {
        return Err(io::Error::other("changed input not kept").into());
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Missing)
        != RestoreDecision::KeepCurrent
    {
        return Err(io::Error::other("missing input resurrected").into());
    }
    if fs::read(&current)? != b"changed" || read_mtime(&current)? != before {
        return Err(io::Error::other("current file disturbed").into());
    }
    let git_entry = ProjectionEntry {
        path: ".git/objects/pack".to_owned(),
        kind: EntryKind::File,
        mode: 0o644,
        content_sha256: sha256_hex(b"pack"),
        link_target: None,
        mtime: Some(mtime()),
    };
    if !is_git_path(&git_entry.path) {
        return Err(io::Error::other("git path not detected").into());
    }
    if decide(Some(&git_entry), Some(&git_entry), CheckoutState::Unchanged)
        != RestoreDecision::KeepCurrent
    {
        return Err(io::Error::other("git metadata scheduled").into());
    }
    if decide(Some(&declared), None, CheckoutState::Unchanged) != RestoreDecision::Discard {
        return Err(io::Error::other("cached-only path kept").into());
    }
    if decide(None, Some(&declared), CheckoutState::Unchanged) != RestoreDecision::KeepCurrent {
        return Err(io::Error::other("absent staged input scheduled").into());
    }
    Ok(())
}

fn check_symlink_restores_target_without_mtime() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let declared = link_entry("config/link", "../shared/target");
    write_checkout_file(&checkout, "shared/target", b"shared", 0o644)?;
    let checkout_link = checkout.join("config/link");
    fs::create_dir_all(checkout.join("config"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("../shared/target", &checkout_link)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Unchanged {
        return Err(io::Error::other("matching link rejected").into());
    }
    let staged_link = staged.join("config/link");
    fs::create_dir_all(staged.join("config"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("../shared/target", &staged_link)?;
    let destination = staged_path(&checkout, "config/link")?;
    fs::remove_file(&checkout_link)?;
    let outcome = install_staged_symlink(&staged_link, &checkout, &declared)?;
    if outcome.mtime_restored {
        return Err(io::Error::other("link mtime claimed").into());
    }
    if fs::read_link(&destination)? != Path::new("../shared/target") {
        return Err(io::Error::other("link target drifted").into());
    }
    if restore_times(&destination, &mtime()).is_ok() {
        return Err(io::Error::other("link metadata followed").into());
    }
    Ok(())
}

#[path = "gc1_t10_a.rs"]
mod gc1_t10_a;
