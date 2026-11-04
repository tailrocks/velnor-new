// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Github, JournalRegistryFake, OrchestratorError, Registry, fixture_archive,
    fixture_journal_plan, fixture_verify_release_package, make_fixture, prepare_crate_release,
    test_git, velnor_actions_orchestrator,
};

#[test]
pub(super) fn upload_intent_requires_fresh_admission_and_absent_publication()
-> Result<(), Box<dyn std::error::Error>> {
    use velnor_actions_orchestrator::crate_release_controller::CrateReleaseObservation;
    use velnor_actions_orchestrator::crate_release_journal::record_source_bound_start;
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    record_source_bound_start(&journal, &identity, &plan, root, test_git())?;
    let obs = CrateReleaseObservation {
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
    check_upload_semver_rejections(&obs)?;
    check_upload_authorization(&obs)
}

pub(super) fn check_upload_authorization(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use super::super::observation::record_admitted_upload_intent;
    use crate::crate_release_controller::verify_and_record_upload_intent;
    use crate::crate_release_journal::{JournalMutationPhase, inspect_journal_mutation};
    let (admission, github, registry, root, journal, identity) = (
        obs.admission,
        obs.github,
        obs.registry,
        obs.source,
        obs.journal,
        obs.identity,
    );
    let prove = || prove_upload_fixture(obs, "demo");
    assert!(prove_upload_fixture(obs, "other").is_err());
    let verified = prove()?;
    github.approved.set(false);
    assert!(
        record_admitted_upload_intent(
            obs,
            verified,
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    github.approved.set(true);
    let verified = prove()?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    assert!(
        record_admitted_upload_intent(
            obs,
            verified,
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    registry.absent.set(true);
    let verified = prove()?;
    github.revoke_on_tag_read.set(true);
    assert!(
        record_admitted_upload_intent(
            obs,
            verified,
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    assert!(inspect_journal_mutation(journal, identity, 1)?.is_none());
    github.approved.set(true);
    assert!(inspect_journal_mutation(journal, identity, 1)?.is_none());
    check_upload_identity_rejections(obs)?;
    assert!(inspect_journal_mutation(journal, identity, 1)?.is_none());
    assert!(
        verify_and_record_upload_intent(
            obs,
            "other",
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    assert!(inspect_journal_mutation(journal, identity, 1)?.is_none());
    verify_and_record_upload_intent(
        obs,
        "demo",
        crate::crate_release_semver::fixture_verified_semver(admission, "demo")?,
    )?;
    assert!(matches!(
        inspect_journal_mutation(journal, identity, 1)?
            .ok_or("missing intention")?
            .phase,
        JournalMutationPhase::Intended
    ));
    Ok(())
}

pub(super) fn prove_upload_fixture(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
    package: &str,
) -> Result<crate::crate_release_controller::VerifiedReleasePackage, OrchestratorError> {
    fixture_verify_release_package(
        obs.admission,
        obs.intent,
        obs.request,
        obs.github,
        obs.registry,
        obs.source,
        package,
        test_git(),
    )
}

pub(super) fn check_upload_identity_rejections(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use super::super::observation::record_admitted_upload_intent;
    use crate::crate_release_controller::CrateReleaseObservation;
    let (admission, intent, request, github, registry, root) = (
        obs.admission,
        obs.intent,
        obs.request,
        obs.github,
        obs.registry,
        obs.source,
    );
    let prove = || {
        fixture_verify_release_package(
            admission,
            intent,
            request,
            github,
            registry,
            root,
            "demo",
            test_git(),
        )
    };
    let verified = prove()?;
    let mut wrong_request = request.clone();
    wrong_request.source_sha = "d".repeat(40);
    let wrong = CrateReleaseObservation {
        request: &wrong_request,
        ..*obs
    };
    assert!(
        record_admitted_upload_intent(
            &wrong,
            verified,
            crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
        )
        .is_err()
    );
    let mutations: [fn(&mut super::super::VerifiedReleasePackage); 5] = [
        |proof| proof.package = "other".into(),
        |proof| proof.version = "0.1.2".into(),
        |proof| proof.intent_sha256 = "e".repeat(64),
        |proof| proof.source_sha = "f".repeat(40),
        |proof| proof.run_id += 1,
    ];
    for mutate in mutations {
        let mut verified = prove()?;
        mutate(&mut verified);
        assert!(
            record_admitted_upload_intent(
                obs,
                verified,
                crate::crate_release_semver::fixture_verified_semver(admission, "demo")?
            )
            .is_err()
        );
    }
    Ok(())
}

pub(super) fn check_upload_semver_rejections(
    obs: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::observation::record_admitted_upload_intent;
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, record_journal_intent,
    };
    for evidence in [
        None,
        Some(crate::crate_release_semver::SemverEvidence::Deferred {
            predecessors: vec!["other".into()],
        }),
    ] {
        let intended = JournalIntendedMutation {
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
            semver_evidence: evidence,
        };
        let before = obs.journal.write_calls.get();
        assert!(
            record_journal_intent(obs.journal, obs.identity, obs.journal_receipt, &intended)
                .is_err()
        );
        assert_eq!(obs.journal.write_calls.get(), before);
    }
    let mut forged = obs.admission.clone();
    forged.github.request.source_sha = "f".repeat(40);
    let semver = crate::crate_release_semver::fixture_verified_semver(&forged, "demo")?;
    let verified = fixture_verify_release_package(
        obs.admission,
        obs.intent,
        obs.request,
        obs.github,
        obs.registry,
        obs.source,
        "demo",
        test_git(),
    )?;
    let before = obs.journal.write_calls.get();
    assert!(record_admitted_upload_intent(obs, verified, semver).is_err());
    assert_eq!(obs.journal.write_calls.get(), before);
    Ok(())
}
