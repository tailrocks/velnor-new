// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn run_signed_child(
    intent: &CrateReleaseIntent,
    previous: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    prior: &str,
    policy: &str,
    workflow: &Path,
    runner: &Path,
) -> Result<(), Box<dyn Error>> {
    let checkout = runner.join("velnor-crate-release-source");
    let output = Command::new("git")
        .args(["clone", "-q", "--no-hardlinks", "--local"])
        .arg(workflow)
        .arg(&checkout)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git(&checkout, &["checkout", "-q", "--detach", prior])?;
    super::super::prepare_test_git_bundle(runner)?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "release::crate_release_runtime::tests::test_only_dispatch_and_loopback_transport",
        ])
        .env("VELNOR_TEST_CONTROLLER_SIGNED_CHILD", "1")
        .env("VELNOR_CONTROLLER_RECOVERY_CASE", "workspace")
        .env("GH_TOKEN", "fixture-token")
        .env(
            "VELNOR_TEST_ATTESTATION_ROOT",
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/attestation_fixtures/historical/trusted-root.jsonl"),
        )
        .env("VELNOR_CONTROLLER_INTENT", serde_json::to_string(intent)?)
        .env(
            "VELNOR_CONTROLLER_PREVIOUS",
            serde_json::to_string(previous)?,
        )
        .env("VELNOR_CONTROLLER_REQUEST", serde_json::to_string(request)?)
        .env("VELNOR_CONTROLLER_PRIOR", prior)
        .env("GITHUB_WORKSPACE", workflow)
        .env("RUNNER_TEMP", runner.canonicalize()?)
        .env("GITHUB_REPOSITORY", &request.repository)
        .env("GITHUB_EVENT_NAME", &request.event)
        .env("GITHUB_REF", "refs/heads/main")
        .env(
            "GITHUB_WORKFLOW_REF",
            format!(
                "{}/{}@refs/heads/main",
                request.repository, request.workflow_path
            ),
        )
        .env("GITHUB_WORKFLOW_SHA", policy)
        .env("GITHUB_RUN_ID", request.run_id.to_string())
        .env("GITHUB_RUN_ATTEMPT", request.run_attempt.to_string())
        .output()?;
    assert!(
        output.status.success(),
        "signed workspace child failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

pub(super) fn prepare_prior(
    previous: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    prior_sha: &str,
    source: &Path,
    git: &crate::crate_release_git::ReleaseGit,
    mixed: &mut super::super::super::MixedGithub,
    registry: &super::super::super::WorkspaceRegistry,
) -> Result<
    (
        crate::crate_release_journal::JournalReceipt,
        crate::crate_release_journal::JournalIntendedMutation,
        Vec<u8>,
    ),
    Box<dyn Error>,
> {
    let mut prior_request = request.clone();
    prior_request.event = "push".into();
    prior_request.workflow_sha = prior_sha.to_owned();
    prior_request.recovery_sha = None;
    mixed.inner.0.push_event.set(true);
    mixed.inner.0.policy_sha = prior_sha.to_owned();
    let original = super::super::super::prepare_crate_release(
        previous,
        &prior_request,
        &mixed.inner,
        registry,
        source,
    )?;
    let target = tempfile::tempdir()?;
    let first_archive = crate::crate_release_controller::package_unprivileged(
        source,
        previous,
        &original.packages[0].source,
        target.path(),
        &request.source_sha,
        false,
        git,
    )?;
    let (_, mut prior) =
        super::super::super::fixture_journal_plan(source, previous, &prior_request, &original)?;
    super::super::super::mixed_workspace::set_workspace_journal_packages(&mut prior, &original);
    prior.identity.run_id = 11;
    prior.identity.workflow_sha = prior_sha.to_owned();
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    prior
        .generator_version
        .clone_from(&previous.generator_version);
    prior
        .release_plz_version
        .clone_from(&previous.release_plz_version);
    prior.rust_version.clone_from(&previous.rust_version);
    super::super::pin_previous_tools(&mut prior, previous)?;
    let mut prior_upload = super::super::super::fixture_upload_intention();
    prior_upload.package = original.packages[0].source.name.clone();
    Ok((prior, prior_upload, first_archive))
}

pub(super) fn reject_forged_predecessors(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        crate::crate_release_journal_adapter::JournalAdapter<CustodyTransport<'_>>,
        HistoricalMixed<'_>,
        super::super::super::MixedRegistry,
    >,
    prior: &crate::crate_release_journal::JournalReceipt,
    prior_upload: &crate::crate_release_journal::JournalIntendedMutation,
    store: &super::super::super::JournalRegistryFake,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_journal::{
        fixture_storage::seed_started_record, record_journal_intent,
    };
    use crate::release::package_release_signer::{AdmittedStart, record_admitted_start};
    let mixed = input.github.mixed;
    let registry = input.registry;
    let request = input.request;
    let before = store.write_calls.get();
    let mut wrong_source = request.clone();
    wrong_source.source_sha = "f".repeat(40);
    let wrong = AdmittedStart {
        request: &wrong_source,
        ..*input
    };
    assert!(record_admitted_start(&wrong).is_err());
    assert_eq!(store.write_calls.get(), before);
    assert!(mixed.writes.borrow().is_empty());
    assert!(registry.second.borrow().is_none());
    for scenario in ["source", "selection"] {
        let forged_store = super::super::super::JournalRegistryFake::default();
        let forged_journal = crate::crate_release_journal_adapter::JournalAdapter::new(
            CustodyTransport(&forged_store),
        );
        let mut forged = prior.clone();
        if scenario == "source" {
            forged.identity.source_sha = "f".repeat(40);
        } else {
            let removed = forged.packages.pop().ok_or("missing second package")?;
            forged.semver_evidence.remove(&removed.name);
        }
        seed_started_record(&forged_journal, &forged.identity, &forged)
            .map_err(|error| format!("seed forged {scenario} start: {error}"))?;
        record_journal_intent(&forged_journal, &forged.identity, &forged, prior_upload)
            .map_err(|error| format!("seed forged {scenario} upload: {error}"))?;
        let before = forged_store.write_calls.get();
        let wrong = AdmittedStart {
            journal: &forged_journal,
            ..*input
        };
        assert!(
            record_admitted_start(&wrong).is_err(),
            "accepted forged {scenario}"
        );
        assert_eq!(
            forged_store.write_calls.get(),
            before,
            "wrote journal after forged {scenario}"
        );
        assert!(mixed.writes.borrow().is_empty());
        assert!(registry.second.borrow().is_none());
    }
    Ok(())
}

pub(super) fn coordinate_continuation(
    input: &crate::release::package_release_signer::AdmittedStart<
        '_,
        crate::crate_release_journal_adapter::JournalAdapter<CustodyTransport<'_>>,
        HistoricalMixed<'_>,
        super::super::super::MixedRegistry,
    >,
    identity: &crate::crate_release_journal::JournalIdentity,
    receipt: &crate::crate_release_journal::JournalReceipt,
    first_archive: Vec<u8>,
) -> Result<(), Box<dyn Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use std::cell::Cell;
    let mixed = input.github.mixed;
    let (git, journal, admission, intent, request, github, registry, source) = (
        input.git,
        input.journal,
        input.admission,
        input.intent,
        input.request,
        input.github,
        input.registry,
        input.source,
    );
    let server = super::super::super::SparseFixture::start(
        "velnor_fixture_order_20261104_a",
        first_archive,
    )?;
    super::super::super::FIXTURE_SPARSE_INDEX
        .with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let waits = Cell::new(0);
    let observation = CrateReleaseObservation {
        source_execution: &crate::UnprivilegedSourceExecution,
        git,
        journal,
        identity,
        journal_receipt: receipt,
        admission,
        intent,
        request,
        github,
        registry,
        source,
        sequence: 1,
    };
    let result = coordinate_release(
        &observation,
        |name| {
            super::super::super::mixed_workspace::mixed_verified_semver(
                admission, mixed, &server, name,
            )
        },
        |permit| {
            assert_eq!(permit.package, "velnor_fixture_order_20261104_b");
            assert_eq!(&*mixed.writes.borrow(), &["tag-0", "release-0"]);
            let target = tempfile::tempdir().map_err(|_| OrchestratorError::Contract {
                problem: "fixture".into(),
            })?;
            let archive = crate::crate_release_controller::package_unprivileged(
                source,
                intent,
                &admission.packages[1].source,
                target.path(),
                &request.source_sha,
                false,
                git,
            )?;
            *registry.second.borrow_mut() = Some(archive);
            Ok(UploadAttempt::ExitedSuccess)
        },
        || super::super::super::mixed_workspace::propagate_sparse_index(registry, &server, &waits),
    );
    super::super::super::FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    assert_eq!(waits.get(), 1);
    super::super::super::mixed_workspace::check_mixed_workspace_result(
        mixed,
        result?.packages.len(),
    );
    Ok(())
}

pub(super) fn attach_semver_evidence(
    admission: &mut crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    source: &Path,
    registry: &super::super::super::MixedRegistry,
    git: &crate::crate_release_git::ReleaseGit,
) -> Result<(), Box<dyn Error>> {
    let evidence = crate::release::crate_release_semver::verify_one(
        source,
        admission,
        registry,
        &admission.packages[0],
        Path::new("/unused-checker"),
        git,
        &crate::UnprivilegedSourceExecution,
    )?;
    admission
        .semver_evidence
        .insert(admission.packages[0].source.name.clone(), evidence);
    admission.semver_evidence.insert(
        admission.packages[1].source.name.clone(),
        crate::release::crate_release_semver::SemverEvidence::Deferred {
            predecessors: vec![admission.packages[0].source.name.clone()],
        },
    );
    Ok(())
}

pub(super) fn make_mixed(
    request: &GitHubAdmissionRequest,
) -> Result<
    (
        super::super::super::MixedGithub,
        super::super::super::WorkspaceRegistry,
    ),
    Box<dyn Error>,
> {
    use std::cell::{Cell, RefCell};
    let (_, _, _, mut workspace_github, registry) =
        super::super::super::make_workspace_order_fixture()?;
    workspace_github.0.sha.clone_from(&request.source_sha);
    workspace_github
        .0
        .policy_sha
        .clone_from(&request.workflow_sha);
    workspace_github.0.push_event.set(false);
    let mixed = super::super::super::MixedGithub {
        inner: workspace_github,
        tags: [Cell::new(false), Cell::new(false)],
        releases: [Cell::new(false), Cell::new(false)],
        writes: RefCell::new(Vec::new()),
    };
    Ok((mixed, registry))
}
