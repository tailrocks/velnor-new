// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t10_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_semantic_ignores_mtime()?;
    check_unchanged_file_roundtrip()?;
    check_directories_restore_after_children()?;
    check_changed_and_git_inputs_stay_current()?;
    check_symlink_restores_target_without_mtime()?;
    let mut ordered = vec![
        PathBuf::from("a"),
        PathBuf::from("a/b/c"),
        PathBuf::from("a/b"),
    ];
    sort_deepest_first(&mut ordered);
    if ordered
        != vec![
            PathBuf::from("a/b/c"),
            PathBuf::from("a/b"),
            PathBuf::from("a"),
        ]
    {
        return Err(io::Error::other("directory order drifted").into());
    }
    Ok(())
}

fn reject_path(path: &str) -> Result<(), Box<dyn Error>> {
    if validate_relative_path(path).is_ok() {
        return Err(io::Error::other(format!("invalid path accepted: {path}")).into());
    }
    Ok(())
}

fn reject_link(target: &str) -> Result<(), Box<dyn Error>> {
    if validate_link_target(target).is_ok() {
        return Err(io::Error::other(format!("invalid link accepted: {target}")).into());
    }
    Ok(())
}

fn check_rejects_invalid_records() -> Result<(), Box<dyn Error>> {
    for path in [
        "",
        ".",
        "/absolute",
        "../escape",
        "a/../escape",
        "./dotted",
        "a/./dotted",
        "back\\slash",
        "nul\0byte",
        ".git",
        ".git/config",
        ".git/objects/x",
    ] {
        reject_path(path)?;
    }
    for target in ["", ".", "a/..", "/absolute", "nul\0byte"] {
        reject_link(target)?;
    }
    let root = Path::new("/checkout");
    validate_link_target_in_root(root, "config/link", "../shared/target")?;
    for (link, target) in [
        ("config/link", "../../escape"),
        ("top/link", "../../escape"),
        ("config/link", "/absolute"),
        ("config/link", "../shared/../../escape"),
    ] {
        if validate_link_target_in_root(root, link, target).is_ok() {
            return Err(io::Error::other(format!("escaping link admitted: {target}")).into());
        }
    }
    for digest in [
        "",
        "abc",
        &"g".repeat(64),
        &"A".repeat(64),
        &"b3".repeat(31),
    ] {
        if validate_sha256(digest).is_ok() {
            return Err(io::Error::other(format!("invalid digest accepted: {digest}")).into());
        }
    }
    let mut bad_mtime = file_entry("src/lib.rs", b"x", 0o644);
    bad_mtime.mtime = Some(RealMtime {
        seconds: 0,
        nanoseconds: 1_000_000_000,
    });
    if bad_mtime.validate().is_ok() {
        return Err(io::Error::other("nanosecond overflow accepted").into());
    }
    let mut bad_mode = file_entry("src/lib.rs", b"x", 0o644);
    bad_mode.mode = 0o1_000_000;
    if bad_mode.validate().is_ok() {
        return Err(io::Error::other("mode overflow accepted").into());
    }
    let mut file_with_link = file_entry("src/lib.rs", b"x", 0o644);
    file_with_link.link_target = Some("target".to_owned());
    if file_with_link.validate().is_ok() {
        return Err(io::Error::other("file with link accepted").into());
    }
    let mut link_without_target = link_entry("config/link", "../shared/target");
    link_without_target.link_target = None;
    if link_without_target.validate().is_ok() {
        return Err(io::Error::other("link without target accepted").into());
    }
    let mut dir_digest = dir_entry("pkg", 0o755, mtime());
    dir_digest.content_sha256 = sha256_hex(b"not empty");
    if dir_digest.validate().is_ok() {
        return Err(io::Error::other("directory content digest accepted").into());
    }
    if EntryKind::parse("bogus").is_ok() {
        return Err(io::Error::other("unknown kind accepted").into());
    }
    Ok(())
}

fn check_rejects_mismatched_state() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let checkout = temp.path().join("checkout");
    let staged = temp.path().join("staged");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(&staged)?;
    let declared = file_entry("src/lib.rs", b"declared", 0o644);
    write_checkout_file(&checkout, "src/lib.rs", b"other bytes", 0o644)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(io::Error::other("content mismatch missed").into());
    }
    write_checkout_file(&checkout, "src/lib.rs", b"declared", 0o600)?;
    if authenticate_entry(&checkout, &declared)? != CheckoutState::Changed {
        return Err(io::Error::other("mode mismatch missed").into());
    }
    let fresh = temp.path().join("fresh");
    fs::create_dir_all(&fresh)?;
    if authenticate_entry(&fresh, &declared)? != CheckoutState::Missing {
        return Err(io::Error::other("missing input misclassified").into());
    }
    fs::create_dir_all(fresh.join("src"))?;
    fs::create_dir_all(checkout.join("as-dir"))?;
    let as_dir = dir_entry("as-dir", 0o755, mtime());
    if authenticate_entry(&checkout, &as_dir)? != CheckoutState::Unchanged {
        return Err(io::Error::other("matching directory rejected").into());
    }
    let kind_swap = file_entry("as-dir", b"declared", 0o644);
    if authenticate_entry(&checkout, &kind_swap)? != CheckoutState::Changed {
        return Err(io::Error::other("kind swap missed").into());
    }
    let escaping = link_entry("config/link", "../../escape");
    if authenticate_entry(&checkout, &escaping).is_ok() {
        return Err(io::Error::other("escaping declared link admitted").into());
    }
    let diverted = link_entry("config/diverted", "../shared/target");
    let diverted_link = checkout.join("config/diverted");
    fs::create_dir_all(checkout.join("config"))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink("../other", &diverted_link)?;
    if authenticate_entry(&checkout, &diverted)? != CheckoutState::Changed {
        return Err(io::Error::other("diverted link missed").into());
    }
    let mut tampered = declared.clone();
    tampered.content_sha256 = sha256_hex(b"tampered");
    if reconcile(&tampered, &declared).is_ok() {
        return Err(io::Error::other("archive mismatch admitted").into());
    }
    let mut timeless = declared.clone();
    timeless.mtime = None;
    if reconcile(&timeless, &declared).is_ok() {
        return Err(io::Error::other("missing mtime admitted").into());
    }
    let staged_file = write_checkout_file(&staged, "src/lib.rs", b"tampered", 0o644)?;
    let destination = staged_path(&checkout, "src/lib.rs")?;
    if install_staged_file(&staged_file, &destination, &declared).is_ok() {
        return Err(io::Error::other("tampered staged file installed").into());
    }
    if fs::read(&destination)? != b"declared" {
        return Err(io::Error::other("failed install wrote destination").into());
    }
    let wrong_stamp = RealMtime {
        seconds: ARCHIVED_SECONDS,
        nanoseconds: 999_999_999,
    };
    if verify_times(&checkout.join("src/lib.rs"), &wrong_stamp).is_ok() {
        return Err(io::Error::other("wrong mtime verified").into());
    }
    Ok(())
}

#[test]
fn gc1_t10_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_invalid_records()?;
    check_rejects_mismatched_state()?;
    Ok(())
}
