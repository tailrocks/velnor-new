// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct SupervisedScenario<'a> {
    execution: crate::crate_release_source_view::SupervisedSourceExecution,
    intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    git: &'a crate::crate_release_git::ReleaseGit,
    previous_intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    previous_sha: String,
    request: &'a GitHubAdmissionRequest,
    receipt: &'a crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    github: &'a Github,
    registry: &'a Registry,
    fixture: &'a tempfile::TempDir,
    original: &'a Path,
    root: &'a crate::crate_release_custody::PublisherRoot,
    package: &'a crate::CrateReleaseSourcePackage,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn supervised_fixture_scenario(
    scenario: SupervisedScenario<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::CrateSourceExecution as _;
    let SupervisedScenario {
        execution,
        intent,
        git,
        previous_intent,
        previous_sha,
        request,
        receipt,
        github,
        registry,
        fixture,
        original,
        root,
        package,
    } = scenario;
    if let Ok(preflight) = std::env::var("VELNOR_TEST_UPLOAD_PREFLIGHT") {
        let result = execution.credential_preflight(execution.root(), intent, git);
        if preflight == "missing" {
            assert!(result.is_err(), "missing credential accepted");
            return Ok(());
        }
        result?;
    }
    assert!(
        crate::UnprivilegedSourceExecution
            .historical_intent_reader()
            .is_err()
    );
    let (_, mut prior) =
        super::fixture_journal_plan(execution.root(), previous_intent, request, receipt)?;
    prior.identity.run_id = 11;
    prior.identity.workflow_sha.clone_from(&previous_sha);
    prior.workflow_run_url = "https://github.com/example/demo/actions/runs/11".into();
    let (_, mut current) = super::fixture_journal_plan(execution.root(), intent, request, receipt)?;
    current.predecessor = Some(prior.identity.clone());
    let history_api = super::HistoricalAuthority {
        base: github,
        workflow: previous_sha,
        older: None,
        event: "workflow_dispatch",
        defect: std::cell::Cell::new(0),
    };
    let reader = execution.historical_intent_reader()?;
    if check_supervised_fault(FaultFixture {
        execution: &execution,
        intent,
        git,
        request,
        receipt,
        registry,
        original,
        prior: &prior,
        current: &current,
        history_api: &history_api,
        reader: &reader,
    })? {
        return Ok(());
    }
    reader.verify_transition(&prior, &current, request, &history_api)?;
    reader.verify_prior(&prior, request, &history_api)?;
    supervised_finish_scenario(FinishFixture {
        execution,
        intent,
        git,
        request,
        receipt,
        github,
        registry,
        fixture,
        original,
        root,
        package,
        prior,
        current,
        history_api,
    })
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
type FixtureObservation<'a> = crate::crate_release_controller::CrateReleaseObservation<
    'a,
    super::JournalRegistryFake,
    super::HistoricalAuthority<'a>,
    Registry,
>;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn supervised_package_and_semver(
    observation: &FixtureObservation<'_>,
    execution: &crate::crate_release_source_view::SupervisedSourceExecution,
    fixture: &tempfile::TempDir,
    root: &crate::crate_release_custody::PublisherRoot,
    package: &crate::CrateReleaseSourcePackage,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    use crate::CrateSourceExecution as _;
    let metadata = execution.metadata(execution.root(), observation.intent, observation.git)?;
    if !metadata.contains("\"name\":\"demo\"") {
        return Err("package missing from supervised metadata".into());
    }
    let archive = execution.package(&crate::CratePackageSource::new(
        execution.root(),
        observation.intent,
        package,
        fixture.path(),
        &observation.request.source_sha,
        false,
        observation.git,
    ))?;
    let files = crate::crate_release_archive::read_archive(&archive, "demo-0.1.1/")?;
    if files.get("src/lib.rs").map(|entry| entry.1.as_slice())
        != Some(b"pub fn demo() {}".as_slice())
    {
        return Err("package bytes differ".into());
    }
    if std::env::var_os("VELNOR_TEST_UPLOAD_FULL").is_some() {
        fs::write(root.path().join("expected-demo.crate"), &archive)?;
    }
    let baseline = supervised_fixture_baseline()?;
    let checksum = crate::generator::sha256_hex(&baseline);
    let features: Vec<String> = Vec::new();
    execution.check_semver(&crate::CrateSemverSource {
        source: execution.root(),
        rust: &observation.intent.rust_version,
        manifest: &package.manifest,
        package: &package.name,
        baseline_version: "0.1.0",
        baseline_checksum: &checksum,
        baseline_archive: &baseline,
        features: &features,
        executable: &root.path().join("velnor-semver/cargo-semver-checks"),
        git: observation.git,
    })?;
    Ok((archive, baseline))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn supervised_maybe_upload(
    observation: &FixtureObservation<'_>,
    execution: &crate::crate_release_source_view::SupervisedSourceExecution,
    root: &crate::crate_release_custody::PublisherRoot,
    baseline: Vec<u8>,
) -> Result<bool, Box<dyn std::error::Error>> {
    if std::env::var_os("VELNOR_TEST_UPLOAD_FULL").is_some() {
        *observation.registry.baseline.borrow_mut() = baseline;
        execution.credential_preflight(execution.root(), observation.intent, observation.git)?;
        let verified = crate::crate_release_semver::verify_before_upload(
            execution.root(),
            observation.admission,
            observation.registry,
            root.path(),
            "demo",
            observation.git,
            execution,
        )?;
        let permit = crate::crate_release_controller::verify_and_record_upload_intent(
            observation,
            "demo",
            verified,
        )?;
        let attempt = crate::crate_release_controller::invoke_supervised_upload(
            observation,
            permit,
            execution,
        );
        if std::env::var_os("VELNOR_TEST_UPLOAD_DENY").is_some()
            || std::env::var_os("VELNOR_TEST_UPLOAD_DROP").is_some()
        {
            assert!(attempt.is_err(), "uncertain upload was accepted");
            let mutation = crate::crate_release_journal::inspect_journal_mutation(
                observation.journal,
                observation.identity,
                1,
            )?
            .ok_or("missing upload intent")?;
            assert!(matches!(
                mutation.phase,
                crate::crate_release_journal::JournalMutationPhase::Intended
            ));
            assert!(
                crate::crate_release_journal::inspect_journal(
                    observation.journal,
                    observation.identity,
                    crate::crate_release_journal::JournalStage::Completed,
                )?
                .is_none()
            );
            assert!(
                execution
                    .credential_preflight(execution.root(), observation.intent, observation.git)
                    .is_err()
            );
            return Ok(true);
        }
        let attempt = attempt?;
        assert!(matches!(
            attempt,
            crate::crate_release_controller::UploadAttempt::ExitedSuccess
        ));
    }
    Ok(false)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn seed_supervised_journal(
    journal: &super::JournalRegistryFake,
    prior: &crate::crate_release_journal::JournalReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    crate::crate_release_journal::fixture_storage::seed_started_record(
        journal,
        &prior.identity,
        prior,
    )?;
    crate::crate_release_journal::record_journal_intent(
        journal,
        &prior.identity,
        prior,
        &crate::crate_release_journal::JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: crate::crate_release_journal::JournalMutationKind::Upload,
        },
    )?;
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct FinishFixture<'a> {
    execution: crate::crate_release_source_view::SupervisedSourceExecution,
    intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    git: &'a crate::crate_release_git::ReleaseGit,
    request: &'a GitHubAdmissionRequest,
    receipt: &'a crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    github: &'a Github,
    registry: &'a Registry,
    fixture: &'a tempfile::TempDir,
    original: &'a Path,
    root: &'a crate::crate_release_custody::PublisherRoot,
    package: &'a crate::CrateReleaseSourcePackage,
    prior: crate::crate_release_journal::JournalReceipt,
    current: crate::crate_release_journal::JournalReceipt,
    history_api: super::HistoricalAuthority<'a>,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn supervised_finish_scenario(input: FinishFixture<'_>) -> Result<(), Box<dyn std::error::Error>> {
    let journal = super::JournalRegistryFake::default();
    let started = start_supervised_journal(&input, &journal)?;
    let FinishFixture {
        execution,
        intent,
        git,
        request,
        receipt,
        github,
        registry,
        fixture,
        root,
        package,
        prior,
        current,
        history_api,
        ..
    } = input;
    let observation = crate::crate_release_controller::CrateReleaseObservation {
        journal: &journal,
        identity: &current.identity,
        journal_receipt: &started.receipt,
        admission: receipt,
        intent,
        request,
        github: &history_api,
        registry,
        source: execution.root(),
        git,
        source_execution: &execution,
        sequence: 1,
    };
    let recovered =
        crate::crate_release_controller::observe_prior_recovery(&observation, &prior.identity)?;
    if recovered.len() != 1 {
        return Err("historical recovery was not observed".into());
    }
    let (archive, baseline) =
        supervised_package_and_semver(&observation, &execution, fixture, root, package)?;
    if supervised_maybe_upload(&observation, &execution, root, baseline)? {
        return Ok(());
    }
    registry.absent.set(false);
    *registry.archive.borrow_mut() = archive;
    github.tag_exists.set(true);
    github.release_variant.set(0);
    crate::crate_release_controller::coordinate_release(
        &observation,
        |_| {
            Err(crate::OrchestratorError::Contract {
                problem: "unexpected semver".into(),
            })
        },
        |_| {
            Err(crate::OrchestratorError::Contract {
                problem: "unexpected upload".into(),
            })
        },
        || {},
    )?;
    let frozen = crate::crate_release_controller::FrozenCompletion::prepare_supervised(
        &observation,
        crate::package_release_signer::fixture_measured_tools(
            started.receipt.expected_tools.clone(),
        ),
        &execution,
    )?;
    execution
        .finish(frozen)?
        .complete(&journal, &history_api, registry)?;
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn start_supervised_journal(
    input: &FinishFixture<'_>,
    journal: &super::JournalRegistryFake,
) -> Result<crate::crate_release_journal::JournalEvidence, Box<dyn std::error::Error>> {
    seed_supervised_journal(journal, &input.prior)?;
    let generator = crate::package_release_signer::fixture_attested_generator();
    let start = crate::package_release_signer::AdmittedStart {
        journal,
        github: &input.history_api,
        registry: input.registry,
        intent: input.intent,
        request: input.request,
        admission: input.receipt,
        source: input.execution.root(),
        git: input.git,
        source_execution: &input.execution,
        workflow_root: input.original,
        generator: &generator,
    };
    crate::package_release_signer::record_admitted_start(&start)?;
    Ok(crate::crate_release_journal::inspect_journal(
        journal,
        &input.current.identity,
        crate::crate_release_journal::JournalStage::Started,
    )?
    .ok_or("missing started journal")?)
}
