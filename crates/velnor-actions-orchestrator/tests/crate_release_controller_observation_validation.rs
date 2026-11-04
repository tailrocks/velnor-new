// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn reject_observed_approval_revocation(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::record_verified_mutation;
    use crate::crate_release_journal::inspect_journal_mutation;
    obs.github.owner_match.set(false);
    assert!(record_verified_mutation(obs).is_err());
    obs.github.owner_match.set(true);
    obs.github.revoke_on_tag_read.set(true);
    assert!(record_verified_mutation(obs).is_err());
    assert!(inspect_journal_mutation(obs.journal, obs.identity, 2)?.is_none());
    obs.github.approved.set(true);
    Ok(())
}

fn reject_tampered_source(
    obs: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = obs.source.join("src/lib.rs");
    fs::write(&path, "tampered")?;
    assert!(
        velnor_actions_orchestrator::crate_release_controller::record_verified_mutation(obs)
            .is_err()
    );
    fs::write(path, "pub fn demo() {}")?;
    Ok(())
}

fn reject_wrong_authentication(
    obs: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) {
    let mut wrong_auth = obs.journal_receipt.clone();
    assert!(super::observation::verify_binding(obs).is_ok());
    wrong_auth.authentication =
        velnor_actions_orchestrator::crate_release_journal::JournalAuthentication::BootstrapToken;
    let wrong = velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation {
        journal_receipt: &wrong_auth,
        ..*obs
    };
    assert!(super::observation::verify_binding(&wrong).is_err());
}

fn fixture_journal_plan(
    root: &Path,
    intent: &CrateReleaseIntent,
    request: &GitHubAdmissionRequest,
    admission: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseAdmissionReceipt,
) -> Result<
    (
        velnor_actions_orchestrator::crate_release_journal::JournalIdentity,
        velnor_actions_orchestrator::crate_release_journal::JournalReceipt,
    ),
    Box<dyn std::error::Error>,
> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalAuthentication, JournalIdentity, JournalOwner, JournalOwnerKind, JournalPackage,
        JournalReceipt, JournalRegistry, JournalToolPins,
    };
    let identity = JournalIdentity {
        repository: request.repository.clone(),
        workflow_sha: request.workflow_sha.clone(),
        source_sha: request.source_sha.clone(),
        run_id: request.run_id,
        run_attempt: request.run_attempt,
    };
    let intent_sha256 = test_hex(&Sha256::digest(
        velnor_actions_contract::canonical_json_bytes(intent)?,
    ));
    let mut base_intent = intent.clone();
    base_intent.journal_predecessor = None;
    base_intent.recovery = None;
    let base_intent_sha256 = test_hex(&Sha256::digest(
        velnor_actions_contract::canonical_json_bytes(&base_intent)?,
    ));
    let plan = JournalReceipt {
        semver_evidence: admission.semver_evidence.clone(),
        predecessor: None,
        identity: identity.clone(),
        base_intent_sha256,
        intent_sha256,
        registry: JournalRegistry::CratesIo,
        generator_version: "0.1.72".into(),
        release_plz_version: "0.4.3".into(),
        rust_version: "1.97.1".into(),
        expected_tools: JournalToolPins {
            generator_sha256: "1".repeat(64),
            release_plz_sha256: "2".repeat(64),
            rust_manifest_sha256: "3".repeat(64),
        },
        authentication: JournalAuthentication::TrustedPublishing,
        required_check_id: admission.github.required_check_id,
        trusted_reviewer_id: 139_017,
        workflow_run_url: "https://github.com/example/demo/actions/runs/12".into(),
        source_files: velnor_actions_orchestrator::journal_source_inventory(
            root,
            &request.source_sha,
            test_git(),
        )?,
        packages: vec![JournalPackage {
            name: "demo".into(),
            version: "0.1.1".into(),
            manifest: "Cargo.toml".into(),
            tag: "demo-v0.1.1".into(),
            owners: crate::crate_release_journal::JournalOwnership::Existing {
                owners: vec![JournalOwner {
                    id: 370_532,
                    login: "owner".into(),
                    kind: JournalOwnerKind::User,
                }],
            },
        }],
    };
    Ok((identity, plan))
}

fn verify_metadata_observations(
    prior: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    github: &Github,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::{
        CrateReleaseObservation, record_verified_mutation,
    };
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind as Kind, JournalMutationPhase as Phase,
        JournalMutationResult as ResultKind, inspect_journal_mutation, record_journal_intent,
    };
    let mut intent = JournalIntendedMutation {
        semver_evidence: None,
        sequence: 3,
        package: "demo".into(),
        version: "0.1.1".into(),
        kind: Kind::CreateTag,
    };
    record_journal_intent(
        prior.journal,
        prior.identity,
        prior.journal_receipt,
        &intent,
    )?;
    let tag_observation = CrateReleaseObservation {
        sequence: 4,
        ..*prior
    };
    github.tag_exists.set(true);
    github.tag_wrong.set(true);
    assert!(record_verified_mutation(&tag_observation).is_err());
    assert!(inspect_journal_mutation(prior.journal, prior.identity, 4)?.is_none());
    github.tag_wrong.set(false);
    record_verified_mutation(&tag_observation)?;
    let tag = inspect_journal_mutation(prior.journal, prior.identity, 4)?
        .ok_or("missing tag evidence")?;
    assert!(
        matches!(tag.phase, Phase::Observed { result: ResultKind::Tag { ref name, ref target_sha } } if name == "demo-v0.1.1" && target_sha == &prior.request.source_sha)
    );
    intent.sequence = 5;
    intent.kind = Kind::CreateRelease;
    record_journal_intent(
        prior.journal,
        prior.identity,
        prior.journal_receipt,
        &intent,
    )?;
    let release_observation = CrateReleaseObservation {
        sequence: 6,
        ..*prior
    };
    github.release_variant.set(4);
    assert!(record_verified_mutation(&release_observation).is_err());
    assert!(inspect_journal_mutation(prior.journal, prior.identity, 6)?.is_none());
    github.release_variant.set(0);
    record_verified_mutation(&release_observation)?;
    let release = inspect_journal_mutation(prior.journal, prior.identity, 6)?
        .ok_or("missing release evidence")?;
    assert!(
        matches!(release.phase, Phase::Observed { result: ResultKind::Release { id: 12, ref tag, ref url } } if tag == "demo-v0.1.1" && url == "https://github.com/example/demo/releases/tag/demo-v0.1.1")
    );
    verify_reconciled_candidate(prior, github)?;
    Ok(())
}

fn test_hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        result.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
    }
    result
}

fn verify_upload_result(
    obs: &velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_journal::{
        JournalMutationPhase, JournalMutationResult, inspect_journal_mutation,
    };
    let observed =
        inspect_journal_mutation(obs.journal, obs.identity, 2)?.ok_or("missing observation")?;
    let JournalMutationPhase::Observed {
        result:
            JournalMutationResult::Upload {
                registry_checksum,
                downloaded_checksum,
                owners,
                features,
                files,
            },
    } = observed.phase
    else {
        return Err("wrong observed result".into());
    };
    let archive = obs.registry.archive.borrow();
    let expected = test_hex(&Sha256::digest(archive.as_slice()));
    assert_eq!(registry_checksum, expected);
    assert_eq!(downloaded_checksum, expected);
    assert_eq!(
        owners,
        journal_existing_owners(&obs.journal_receipt.packages[0].owners)
    );
    assert_eq!(features.get("demo-feature"), Some(&Vec::new()));
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive.as_slice()));
    let mut expected_files = Vec::new();
    for entry in tar.entries()? {
        use std::io::Read as _;
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry
            .path()?
            .to_string_lossy()
            .strip_prefix("demo-0.1.1/")
            .ok_or("bad prefix")?
            .to_owned();
        let executable = entry.header().mode()? & 0o111 != 0;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        expected_files.push(
            velnor_actions_orchestrator::crate_release_journal::JournalFile {
                path,
                sha256: test_hex(&Sha256::digest(&bytes)),
                kind:
                    velnor_actions_orchestrator::crate_release_journal::JournalFileKind::Regular {
                        executable,
                    },
            },
        );
    }
    expected_files.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(files, expected_files);
    drop(archive);
    Ok(())
}

