// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, FIXTURE_SPARSE_INDEX, JournalRegistryFake, MixedGithub, MixedRegistry, RefCell,
    fixture_journal_plan, fs, git, make_fixture, make_workspace_order_fixture,
    prepare_crate_release, set_workspace_journal_packages, test_git,
};
use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
use crate::crate_release_journal::{
    JournalCompletion, JournalFile, JournalFileKind, JournalMutation, JournalMutationKind,
    JournalMutationPhase, JournalMutationResult, JournalOperation, JournalOwnership,
    JournalPublishedPackage, JournalReceipt, fixture_mutation_record_bytes,
    fixture_stage_record_bytes, fixture_storage::seed_started_record,
};
use std::{collections::BTreeMap, path::Path, process::Command};

const LIMIT: usize = 32 * 1024 * 1024;

#[path = "crate_release_controller_capacity_normalization.rs"]
mod normalization;
#[path = "crate_release_controller_capacity_overflow.rs"]
mod overflow;
#[path = "crate_release_controller_capacity_schema.rs"]
mod schema;

fn assert_capacity_stops_before_callbacks<G, R>(
    input: &CrateReleaseObservation<'_, JournalRegistryFake, G, R>,
    journal: &JournalRegistryFake,
) where
    G: crate::crate_release_github::GitHubAdmissionRead
        + crate::crate_release_controller::CrateReleaseRepairWrite,
    R: crate::crate_release_registry::CrateRegistryRead,
{
    let calls = Cell::new(0);
    let result = coordinate_release(
        input,
        |_| {
            calls.set(calls.get() + 1);
            Err(crate::internal::internal("capacity_fixture_stop"))
        },
        |_| panic!("unexpected upload"),
        || {},
    );
    assert!(result.is_err());
    assert_eq!(calls.get(), 0);
    assert_eq!(journal.write_calls.get(), 0);
}

fn add_tracked_paths(
    root: &Path,
    package: &str,
    first: usize,
    count: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;
    let directory = root
        .join(package)
        .join("a".repeat(80))
        .join("b".repeat(80))
        .join("c".repeat(40));
    let component = "e".repeat(160);
    fs::create_dir_all(directory.join(&component))?;
    fs::write(directory.join(&component).join("keep"), b"x")?;
    fs::write(directory.join("target"), b"x")?;
    let target = format!("{component}/../{component}/../{component}/../target");
    for index in first..first + count {
        symlink(
            &target,
            directory.join(format!("{}-{index:06}", "d".repeat(12))),
        )?;
    }
    Ok(())
}

fn commit_paths(root: &Path) -> Result<String, Box<dyn std::error::Error>> {
    git(root, &["add", "-A"])?;
    git(root, &["commit", "-q", "-s", "-m", "capacity fixture"])?;
    git(root, &["rev-parse", "HEAD"])
}

fn package_files(
    root: &Path,
    package: &str,
) -> Result<Vec<JournalFile>, Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO"))
        .args([
            "package",
            "--list",
            "--locked",
            "--offline",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            package,
        ])
        .current_dir(root)
        .env("CARGO_HOME", home.path())
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?
        .lines()
        .map(|path| JournalFile {
            path: path.to_owned(),
            sha256: "f".repeat(64),
            kind: JournalFileKind::Regular { executable: false },
        })
        .collect())
}

fn observed_size(
    receipt: &JournalReceipt,
    index: usize,
    files: Vec<JournalFile>,
    features: BTreeMap<String, Vec<String>>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let package = &receipt.packages[index];
    let JournalOwnership::Existing { owners } = &package.owners else {
        return Err("owners".into());
    };
    let mutation = JournalMutation {
        sequence: 2,
        package: package.name.clone(),
        version: package.version.clone(),
        kind: JournalMutationKind::Upload,
        semver_evidence: None,
        phase: JournalMutationPhase::Observed {
            result: JournalMutationResult::Upload {
                registry_checksum: "f".repeat(64),
                downloaded_checksum: "f".repeat(64),
                owners: owners.clone(),
                features,
                files,
            },
        },
    };
    Ok(fixture_mutation_record_bytes(receipt, mutation)?.len())
}

fn intent_size(
    receipt: &JournalReceipt,
    name: &str,
    version: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mutation = JournalMutation {
        sequence: 1,
        package: name.into(),
        version: version.into(),
        kind: JournalMutationKind::Upload,
        semver_evidence: Some(super::fixture_no_baseline()),
        phase: JournalMutationPhase::Intended,
    };
    Ok(fixture_mutation_record_bytes(receipt, mutation)?.len())
}

fn workspace_github(inner: super::WorkspaceGithub) -> MixedGithub {
    MixedGithub {
        inner,
        tags: [Cell::new(false), Cell::new(false)],
        releases: [Cell::new(false), Cell::new(false)],
        writes: RefCell::new(Vec::new()),
    }
}

fn feature_map(
    root: &Path,
    manifest: &str,
) -> Result<BTreeMap<String, Vec<String>>, Box<dyn std::error::Error>> {
    let value: toml::Value = toml::from_str(&fs::read_to_string(root.join(manifest))?)?;
    Ok(value
        .get("features")
        .cloned()
        .ok_or("missing features")?
        .try_into()?)
}

fn verify_archive_limits(
    root: &Path,
    package: &str,
    version: &str,
    listed: &[JournalFile],
) -> Result<(), Box<dyn std::error::Error>> {
    let home = tempfile::tempdir()?;
    let target = tempfile::tempdir()?;
    let output = Command::new(env!("CARGO"))
        .args([
            "package",
            "--locked",
            "--offline",
            "--no-verify",
            "--manifest-path",
            "Cargo.toml",
            "--package",
            package,
            "--target-dir",
        ])
        .arg(target.path())
        .current_dir(root)
        .env("CARGO_HOME", home.path())
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    let archive_path = target
        .path()
        .join(format!("package/{package}-{version}.crate"));
    assert!(fs::metadata(&archive_path)?.len() <= 10 * 1024 * 1024);
    let mut size_decoder = flate2::read::GzDecoder::new(fs::File::open(&archive_path)?);
    assert!(std::io::copy(&mut size_decoder, &mut std::io::sink())? <= 512 * 1024 * 1024);
    let decoder = flate2::read::GzDecoder::new(fs::File::open(archive_path)?);
    let mut archive = tar::Archive::new(decoder);
    let mut names = std::collections::BTreeSet::new();
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        let relative = path
            .strip_prefix(format!("{package}-{version}"))?
            .to_str()
            .ok_or("archive path")?
            .to_owned();
        if matches!(relative.as_str(), "Cargo.toml" | ".cargo_vcs_info.json") {
            assert!(entry.size() <= 1024 * 1024);
        }
        assert!(entry.header().entry_type().is_file());
        assert!(names.insert(relative));
    }
    assert!(names.len() <= 30_000);
    assert_eq!(names, listed.iter().map(|file| file.path.clone()).collect());
    Ok(())
}
