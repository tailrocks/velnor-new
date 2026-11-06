// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use tempfile::TempDir;

use super::filesystem::{CreationFault, CreationTarget, generate_with_creation_fault};
use super::support::{
    GENERATE_LOCK_NAME, assert_no_invocation_stage, generate_in_place, generator_repo,
    make_foreign_legacy_stage, snapshot_preserving_children,
};

fn assert_failed_acquisition_is_recoverable(
    repo: &TempDir,
    target: CreationTarget,
    faults: &[CreationFault],
) -> Result<(), Box<dyn Error>> {
    generate_in_place(repo)?;
    make_foreign_legacy_stage(repo)?;

    for fault in faults {
        let before = snapshot_preserving_children(repo.path())?;
        let error = generate_with_creation_fault(repo.path(), target, *fault)
            .err()
            .ok_or_else(|| {
                std::io::Error::other("the one-shot filesystem fault must fail public generation")
            })?;
        assert!(
            error.contains("injected_generation_") || error.contains("cross_mount_boundary"),
            "generation must report the injected failure, got: {error}"
        );
        assert_eq!(snapshot_preserving_children(repo.path())?, before);
        assert!(!repo.path().join(GENERATE_LOCK_NAME).exists());
        assert_no_invocation_stage(repo)?;

        let written = generate_in_place(repo)?;
        assert!(
            written
                .iter()
                .any(|path| path == ".github/workflows/ci.yml")
        );
        assert!(repo.path().join(".github").is_dir());
        assert!(!repo.path().join(GENERATE_LOCK_NAME).exists());
        assert_no_invocation_stage(repo)?;
        assert!(repo.path().join(".github.velnor-stage").is_dir());
        assert!(repo.path().join("legacy-sentinel").is_file());
    }
    Ok(())
}

#[test]
fn failed_generation_lock_acquisition_cleans_only_its_lock_and_retries()
-> Result<(), Box<dyn Error>> {
    let repo = generator_repo(false)?;
    assert_failed_acquisition_is_recoverable(
        &repo,
        CreationTarget::Lock,
        &[
            CreationFault::OpenDirectory,
            CreationFault::SetMode,
            CreationFault::Metadata,
            CreationFault::MountValidation,
        ],
    )
}

#[test]
fn failed_private_stage_acquisition_cleans_only_its_stage_and_retries() -> Result<(), Box<dyn Error>>
{
    let repo = generator_repo(false)?;
    assert_failed_acquisition_is_recoverable(
        &repo,
        CreationTarget::Stage,
        &[
            CreationFault::OpenDirectory,
            CreationFault::SetMode,
            CreationFault::Metadata,
            CreationFault::MountValidation,
        ],
    )
}
