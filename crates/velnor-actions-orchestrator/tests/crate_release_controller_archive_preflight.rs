// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateReleaseIntent, GitHubAdmissionRequest, Github, JournalRegistryFake, Path, fixture_archive,
    fixture_journal_plan, fs, git, make_fixture, prepare_crate_release, test_git,
};

fn commit_source(
    root: &Path,
    intent: &mut CrateReleaseIntent,
    request: &mut GitHubAdmissionRequest,
    github: &mut Github,
) -> Result<(), Box<dyn std::error::Error>> {
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "commit",
            "-q",
            "-s",
            "-m",
            "archive boundary fixture",
            "-m",
            "Co-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let sha = git(root, &["rev-parse", "HEAD"])?;
    request.source_sha.clone_from(&sha);
    request.recovery_sha = Some(sha.clone());
    intent
        .recovery
        .as_mut()
        .ok_or("missing recovery")?
        .source_sha = sha.clone();
    github.sha = sha;
    Ok(())
}

#[test]
fn large_cargo_archive_within_source_limit_is_accepted() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let initial = fixture_archive(root)?;
    let baseline = tar::Archive::new(flate2::read::GzDecoder::new(initial.as_slice()))
        .entries()?
        .count();
    fs::create_dir(root.join("data"))?;
    for index in 0..(16_384 - baseline) {
        fs::write(root.join("data").join(format!("{index:05}")), b"x")?;
    }
    commit_source(root, &mut intent, &mut request, &mut github)?;

    let archive = fixture_archive(root)?;
    assert_eq!(
        tar::Archive::new(flate2::read::GzDecoder::new(archive.as_slice()))
            .entries()?
            .count(),
        16_384
    );
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    super::fixture_verify_release_package(
        &admission,
        &intent,
        &request,
        &github,
        &registry,
        root,
        "demo",
        test_git(),
    )?;

    Ok(())
}

#[test]
fn oversized_real_cargo_archive_cannot_receive_upload_intent()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, verify_and_record_upload_intent,
    };
    use crate::crate_release_journal::{
        fixture_storage::seed_started_record, inspect_journal_mutation,
    };

    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (mut intent, mut request, mut github, registry) = make_fixture(root)?;
    github.approved.set(true);
    fs::File::create(root.join("large-data"))?.set_len(256 * 1024 * 1024)?;
    commit_source(root, &mut intent, &mut request, &mut github)?;
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan)?;
    let obs = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git: test_git(),
        journal: &journal,
        identity: &identity,
        journal_receipt: &plan,
        admission: &admission,
        intent: &intent,
        request: &request,
        github: &github,
        registry: &registry,
        source: root,
        sequence: 1,
    };
    let evidence =
        crate::release::crate_release_semver::fixture_verified_semver(&admission, "demo")?;
    assert!(matches!(
        verify_and_record_upload_intent(&obs, "demo", evidence),
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_archive_gzip_trailing_or_large"
    ));
    assert!(inspect_journal_mutation(&journal, &identity, 1)?.is_none());
    Ok(())
}

#[test]
fn cargo_archive_entry_limit_accepts_boundary_and_rejects_excess()
-> Result<(), Box<dyn std::error::Error>> {
    use flate2::{Compression, write::GzEncoder};
    use std::io;

    fn archive(count: usize) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let encoder = GzEncoder::new(Vec::new(), Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        for index in 0..count {
            let mut header = tar::Header::new_gnu();
            header.set_size(0);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, format!("demo-0.1.1/{index:06}"), io::empty())?;
        }
        Ok(tar.into_inner()?.finish()?)
    }

    let limit = crate::release::crate_release_source::MAX_SOURCE_ENTRIES + 3;
    let valid = archive(limit)?;
    assert_eq!(
        crate::release::crate_release_archive::read_archive(&valid, "demo-0.1.1/")?.len(),
        limit
    );
    let excess = archive(limit + 1)?;
    assert!(matches!(
        crate::release::crate_release_archive::read_archive(&excess, "demo-0.1.1/"),
        Err(crate::OrchestratorError::Internal { problem }) if problem == "crate_archive_entry_type_or_count"
    ));
    Ok(())
}
