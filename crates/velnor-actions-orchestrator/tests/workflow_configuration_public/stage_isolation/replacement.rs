// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::time::Duration;

use super::filesystem::{BoundaryPoint, start_generation_at};
use super::support::{
    GENERATE_LOCK_NAME, RemoveOnDrop, assert_no_invocation_stage, assert_no_invocation_stage_at,
    copy_checkout, distinct_checkout_paths, generate_in_place, generator_repo, snapshot,
};
use velnor_actions_orchestrator::{GenerateOptions, GenerationRename, generate, prepare};

#[test]
fn exchange_is_anchored_to_the_original_root_after_path_replacement() -> Result<(), Box<dyn Error>>
{
    let repo = generator_repo(false)?;
    generate_in_place(&repo)?;
    let custom = repo.path().join(".github/user-owned.yml");
    fs::write(&custom, b"keep original checkout data\n")?;

    let (mut generation, reached) = start_generation_at(
        repo.path(),
        BoundaryPoint::Rename(GenerationRename::Exchange),
    );
    reached.recv_timeout(Duration::from_secs(20))?;

    let (replacement, backup, _path_cleanup) = distinct_checkout_paths(&repo)?;
    let backup_cleanup = RemoveOnDrop(backup.clone());
    copy_checkout(repo.path(), &replacement)?;
    fs::rename(repo.path(), &backup)?;
    fs::rename(&replacement, repo.path())?;
    generate(&prepare(repo.path())?, &GenerateOptions::default())?;
    fs::write(
        repo.path().join(".github/replacement-sentinel"),
        b"replacement stays unchanged\n",
    )?;
    let replacement_lock = repo.path().join(".github.velnor-generate.lock");
    fs::create_dir(&replacement_lock)?;
    fs::write(replacement_lock.join("sentinel"), b"replacement lock\n")?;

    let stage_name = fs::read_dir(&backup)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|name| name.to_string_lossy().starts_with(".github.velnor-stage-"))
        .ok_or("paused invocation did not own a private staging directory")?;
    let replacement_stage = repo.path().join(&stage_name);
    fs::create_dir(&replacement_stage)?;
    fs::create_dir(replacement_stage.join(".github"))?;
    fs::write(
        replacement_stage.join(".github/path-based-exchange-sentinel"),
        b"path-based mutation target\n",
    )?;
    let replacement_before = snapshot(repo.path())?;

    let generation_result = generation.resume_and_join();
    assert_eq!(snapshot(repo.path())?, replacement_before);
    assert_eq!(generation_result, Ok(()));
    assert_eq!(
        fs::read(backup.join(".github/user-owned.yml"))?,
        b"keep original checkout data\n"
    );
    assert!(!backup.join(".github.velnor-generate.lock").exists());
    assert_no_invocation_stage_at(&backup)?;
    drop(backup_cleanup);
    Ok(())
}

#[test]
fn lock_cleanup_is_anchored_to_the_original_root_after_path_replacement()
-> Result<(), Box<dyn Error>> {
    let repo = generator_repo(false)?;
    generate_in_place(&repo)?;
    let (mut generation, reached) = start_generation_at(
        repo.path(),
        BoundaryPoint::RemoveDirectory(GENERATE_LOCK_NAME),
    );
    reached.recv_timeout(Duration::from_secs(20))?;

    let (replacement, backup, _path_cleanup) = distinct_checkout_paths(&repo)?;
    let backup_cleanup = RemoveOnDrop(backup.clone());
    copy_checkout(repo.path(), &replacement)?;
    fs::rename(repo.path(), &backup)?;
    fs::rename(&replacement, repo.path())?;
    generate(&prepare(repo.path())?, &GenerateOptions::default())?;
    let replacement_lock = repo.path().join(GENERATE_LOCK_NAME);
    fs::create_dir(&replacement_lock)?;
    fs::write(
        replacement_lock.join("sentinel"),
        b"replacement lock remains\n",
    )?;
    let replacement_before = snapshot(repo.path())?;

    let generation_result = generation.resume_and_join();
    assert_eq!(generation_result, Ok(()));
    assert_eq!(snapshot(repo.path())?, replacement_before);
    assert!(!backup.join(GENERATE_LOCK_NAME).exists());
    drop(backup_cleanup);
    Ok(())
}

#[test]
fn first_publication_does_not_replace_a_target_created_at_the_publish_boundary()
-> Result<(), Box<dyn Error>> {
    let repo = generator_repo(false)?;
    let (mut generation, reached) = start_generation_at(
        repo.path(),
        BoundaryPoint::Rename(GenerationRename::NoReplace),
    );
    reached.recv_timeout(Duration::from_secs(20))?;

    let github = repo.path().join(".github");
    fs::create_dir(&github)?;
    fs::write(
        github.join("user-created.yml"),
        b"preserve publish collision\n",
    )?;
    let before = snapshot(&github)?;

    let generation_result = generation.resume_and_join();
    assert!(generation_result.is_err());
    assert_eq!(snapshot(&github)?, before);
    assert!(!repo.path().join(GENERATE_LOCK_NAME).exists());
    assert_no_invocation_stage(&repo)?;
    Ok(())
}
