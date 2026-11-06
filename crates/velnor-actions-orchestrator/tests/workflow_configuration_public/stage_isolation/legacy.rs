// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use super::support::{
    LEGACY_OWNER, LEGACY_STAGE, USER_FILE, assert_no_invocation_stage, generate_in_place,
    generator_repo, make_foreign_legacy_stage, make_legacy_stage, make_symlink_owner_legacy_stage,
    snapshot, snapshot_content,
};

#[test]
fn in_place_generation_ignores_legacy_stage_and_preserves_published_content()
-> Result<(), Box<dyn Error>> {
    let repo = generator_repo(false)?;
    make_legacy_stage(&repo, LEGACY_OWNER)?;
    let legacy = repo.path().join(LEGACY_STAGE);
    let legacy_before = snapshot(&legacy)?;
    let first_files = generate_in_place(&repo)?;
    assert_ne!(first_files, Vec::<String>::new());
    assert_eq!(snapshot(&legacy)?, legacy_before);
    assert_no_invocation_stage(&repo)?;

    let custom_directory = repo.path().join(".github/ISSUE_TEMPLATE");
    fs::create_dir(&custom_directory)?;
    fs::set_permissions(&custom_directory, fs::Permissions::from_mode(0o750))?;
    fs::write(custom_directory.join("config.yml"), USER_FILE)?;

    let second_files = generate_in_place(&repo)?;
    assert_eq!(second_files, first_files);
    assert_eq!(fs::read(custom_directory.join("config.yml"))?, USER_FILE);
    assert_eq!(
        fs::metadata(&custom_directory)?.permissions().mode() & 0o7777,
        0o750
    );
    let published = repo.path().join(".github");
    let published_after_second = snapshot_content(&published)?;
    let third_files = generate_in_place(&repo)?;
    assert_eq!(third_files, first_files);
    assert_eq!(snapshot_content(&published)?, published_after_second);
    assert_eq!(snapshot(&legacy)?, legacy_before);
    assert_no_invocation_stage(&repo)?;
    assert!(!repo.path().join(".github.velnor-generate.lock").exists());
    Ok(())
}

#[test]
fn in_place_generation_never_traverses_legacy_stage_symlinks() -> Result<(), Box<dyn Error>> {
    let repo = generator_repo(false)?;
    let root = repo.path();
    let sentinel = root.join("external-legacy-stage");
    fs::create_dir(&sentinel)?;
    fs::set_permissions(&sentinel, fs::Permissions::from_mode(0o700))?;
    fs::write(sentinel.join("sentinel"), b"must remain untouched\n")?;
    let sentinel_before = snapshot(&sentinel)?;
    std::os::unix::fs::symlink(&sentinel, root.join(LEGACY_STAGE))?;

    generate_in_place(&repo)?;

    assert!(
        fs::symlink_metadata(root.join(LEGACY_STAGE))?
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(root.join(LEGACY_STAGE))?, sentinel);
    assert_eq!(
        snapshot(&root.join("external-legacy-stage"))?,
        sentinel_before
    );
    assert_no_invocation_stage(&repo)?;
    Ok(())
}

#[test]
fn in_place_generation_ignores_foreign_and_symlinked_legacy_owner_records()
-> Result<(), Box<dyn Error>> {
    let foreign_repo = generator_repo(false)?;
    make_foreign_legacy_stage(&foreign_repo)?;
    let foreign_stage = foreign_repo.path().join(LEGACY_STAGE);
    let foreign_before = snapshot(&foreign_stage)?;
    generate_in_place(&foreign_repo)?;
    assert_eq!(snapshot(&foreign_stage)?, foreign_before);
    assert_no_invocation_stage(&foreign_repo)?;

    let symlink_repo = generator_repo(false)?;
    make_symlink_owner_legacy_stage(&symlink_repo)?;
    let symlink_stage = symlink_repo.path().join(LEGACY_STAGE);
    let owner_sentinel = symlink_repo.path().join("owner-sentinel");
    let stage_before = snapshot(&symlink_stage)?;
    let sentinel_before = fs::read(&owner_sentinel)?;
    generate_in_place(&symlink_repo)?;
    assert_eq!(snapshot(&symlink_stage)?, stage_before);
    assert_eq!(fs::read(&owner_sentinel)?, sentinel_before);
    assert_no_invocation_stage(&symlink_repo)?;
    Ok(())
}

#[test]
fn in_place_generation_refuses_an_existing_generation_lock_without_mutation()
-> Result<(), Box<dyn Error>> {
    let repo = generator_repo(false)?;
    let github = repo.path().join(".github");
    fs::create_dir(&github)?;
    fs::write(github.join("user.yml"), USER_FILE)?;
    let before = snapshot(&github)?;
    let lock = repo.path().join(".github.velnor-generate.lock");
    fs::create_dir(&lock)?;

    let error =
        generate_in_place(&repo).expect_err("existing lock must refuse concurrent generation");
    assert!(error.to_string().contains("concurrent_generate"));
    assert_eq!(snapshot(&github)?, before);
    assert!(lock.is_dir());

    fs::remove_dir(lock)?;
    generate_in_place(&repo)?;
    assert!(github.join("user.yml").exists());
    Ok(())
}
