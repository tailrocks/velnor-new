// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    Cell, CrateRegistryRead, CrateRegistryResponse, CrateReleaseAction, CrateReleaseRepairWrite,
    Digest, GitHubAdmissionRead, Github, JournalRegistryFake, OrchestratorError, RefCell, Registry,
    Sha256, Value, WorkspaceGithub, WorkspaceRegistry, fixture_archive, fixture_journal_plan,
    fixture_no_baseline, fs, json, make_fixture, make_workspace_order_fixture,
    prepare_crate_release, test_git, test_hex,
};

pub(super) fn reject_bad_completion_evidence(
    input: &crate::crate_release_controller::CrateReleaseObservation<
        '_,
        JournalRegistryFake,
        Github,
        Registry,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    let try_complete = || {
        crate::crate_release_journal::record_unprivileged_completion(
            input,
            crate::release::package_release_signer::fixture_measured_tools(
                input.journal_receipt.expected_tools.clone(),
            ),
        )
    };
    let archive = input.registry.archive.borrow().clone();
    *input.registry.archive.borrow_mut() = vec![0];
    assert!(try_complete().is_err());
    *input.registry.archive.borrow_mut() = archive;
    input.github.owner_match.set(false);
    assert!(try_complete().is_err());
    input.github.owner_match.set(true);
    input.github.tag_wrong.set(true);
    assert!(try_complete().is_err());
    input.github.tag_wrong.set(false);
    input.github.release_variant.set(5);
    assert!(try_complete().is_err());
    input.github.release_variant.set(0);
    let path = input.source.join("src/lib.rs");
    let bytes = fs::read(&path)?;
    fs::write(&path, b"modified source")?;
    assert!(try_complete().is_err());
    fs::write(&path, bytes)?;
    assert!(
        crate::crate_release_journal::inspect_journal(
            input.journal,
            input.identity,
            crate::crate_release_journal::JournalStage::Completed
        )?
        .is_none()
    );
    Ok(())
}

#[test]
pub(super) fn serial_coordinator_leaves_uncertain_upload_pending_when_not_visible()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{
        CrateReleaseObservation, UploadAttempt, coordinate_release,
    };
    use crate::crate_release_journal::{
        JournalMutationPhase, fixture_storage::seed_started_record, inspect_journal_mutation,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan)?;
    let input = CrateReleaseObservation {
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
    let calls = Cell::new(0);
    assert!(
        coordinate_release(
            &input,
            |name| crate::release::crate_release_semver::fixture_verified_semver(
                input.admission,
                name
            ),
            |_| {
                calls.set(calls.get() + 1);
                Ok(UploadAttempt::Uncertain)
            },
            || {}
        )
        .is_err()
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(github.writes.get(), 0);
    assert!(matches!(
        inspect_journal_mutation(&journal, &identity, 1)?
            .expect("intent")
            .phase,
        JournalMutationPhase::Intended
    ));
    assert!(inspect_journal_mutation(&journal, &identity, 2)?.is_none());
    Ok(())
}

#[test]
pub(super) fn serial_coordinator_recovers_pending_visible_upload_without_uploading_again()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::crate_release_controller::{CrateReleaseObservation, coordinate_release};
    use crate::crate_release_journal::{
        JournalIntendedMutation, JournalMutationKind, fixture_storage::seed_started_record,
        record_journal_intent,
    };
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let (intent, request, github, registry) = make_fixture(root)?;
    github.approved.set(true);
    github.release_variant.set(4);
    let admission = prepare_crate_release(&intent, &request, &github, &registry, root)?;
    let (identity, plan) = fixture_journal_plan(root, &intent, &request, &admission)?;
    let journal = JournalRegistryFake::default();
    seed_started_record(&journal, &identity, &plan)?;
    record_journal_intent(
        &journal,
        &identity,
        &plan,
        &JournalIntendedMutation {
            semver_evidence: Some(fixture_no_baseline()),
            sequence: 1,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::Upload,
        },
    )?;
    *registry.archive.borrow_mut() = fixture_archive(root)?;
    registry.absent.set(false);
    let input = CrateReleaseObservation {
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
    let result = coordinate_release(
        &input,
        |name| crate::release::crate_release_semver::fixture_verified_semver(input.admission, name),
        |_| panic!("must not upload an existing version"),
        || {},
    )?;
    assert_eq!(result.history.len(), 6);
    assert_eq!(github.writes.get(), 2);
    Ok(())
}

#[test]
pub(super) fn mixed_workspace_admission_retains_publish_and_repair()
-> Result<(), Box<dyn std::error::Error>> {
    let (temp, intent, request, github, registry) = make_workspace_order_fixture()?;
    let initial = prepare_crate_release(&intent, &request, &github, &registry, temp.path())?;
    let archive_dir = tempfile::tempdir()?;
    *registry.archive.borrow_mut() = crate::crate_release_controller::package_unprivileged(
        temp.path(),
        &intent,
        &initial.packages[0].source,
        archive_dir.path(),
        &request.source_sha,
        false,
        test_git(),
    )?;
    registry.visible.set(true);
    let mixed = prepare_crate_release(&intent, &request, &github, &registry, temp.path())?;
    assert!(matches!(
        mixed.packages[0].action,
        CrateReleaseAction::RepairTag { .. }
    ));
    assert_eq!(mixed.packages[1].action, CrateReleaseAction::Publish);
    assert_eq!(
        crate::release::crate_release_runtime::release_mode(&mixed),
        "mixed"
    );
    Ok(())
}

pub(super) struct MixedGithub {
    pub(super) inner: WorkspaceGithub,
    pub(super) tags: [Cell<bool>; 2],
    pub(super) releases: [Cell<bool>; 2],
    pub(super) writes: RefCell<Vec<String>>,
}
impl GitHubAdmissionRead for MixedGithub {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.inner.get(route)
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        for (i, suffix) in ["a", "b"].iter().enumerate() {
            let tag = format!("velnor_fixture_order_20261104_{suffix}-v0.1.1");
            if route.contains(&tag) {
                if route.contains("/releases/tags/") {
                    return Ok(self.releases[i].get().then(|| json!({"id":12+i,"tag_name":tag,"draft":false,"prerelease":false,"html_url":format!("https://github.com/example/demo/releases/tag/{tag}")})));
                }
                return Ok(self.tags[i].get().then(|| json!({"ref":format!("refs/tags/{tag}"),"object":{"type":"commit","sha":self.inner.0.sha}})));
            }
        }
        self.inner.get_optional(route)
    }
}
impl CrateReleaseRepairWrite for MixedGithub {
    fn create_tag(&self, _: &str, tag: &str, _: &str) -> Result<(), OrchestratorError> {
        let i = usize::from(tag.contains("_b-v"));
        self.writes.borrow_mut().push(format!("tag-{i}"));
        self.tags[i].set(true);
        Ok(())
    }
    fn create_release(
        &self,
        _: &str,
        tag: &str,
        _: &str,
        _: &str,
    ) -> Result<(), OrchestratorError> {
        let i = usize::from(tag.contains("_b-v"));
        self.writes.borrow_mut().push(format!("release-{i}"));
        self.releases[i].set(true);
        Ok(())
    }
}
pub(super) struct MixedRegistry {
    pub(super) first: WorkspaceRegistry,
    pub(super) second: RefCell<Option<Vec<u8>>>,
}
impl CrateRegistryRead for MixedRegistry {
    fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
        let name = "velnor_fixture_order_20261104_b";
        if path.contains(name) && self.second.borrow().is_some() {
            let archive = self.second.borrow();
            let bytes = archive.as_ref().expect("present");
            if path == format!("/crates/{name}/{name}-0.1.1.crate") {
                return Ok(CrateRegistryResponse {
                    status: 200,
                    body: bytes.clone(),
                });
            }
            if path == format!("/api/v1/crates/{name}/0.1.1") {
                return Ok(CrateRegistryResponse { status: 200, body: json!({"version":{"id":4,"crate":name,"num":"0.1.1","yanked":false,"checksum":test_hex(&Sha256::digest(bytes)),"features":{"pty":[],"base":[],"default":["base"]}}}).to_string().into_bytes() });
            }
            if path == format!("/api/v1/crates/{name}") {
                return Ok(CrateRegistryResponse { status: 200, body: json!({"crate":{"id":name,"name":name,"versions":[3,4]},"versions":[{"id":3,"crate":name,"num":"0.1.0","yanked":false},{"id":4,"crate":name,"num":"0.1.1","yanked":false}]}).to_string().into_bytes() });
            }
        }
        self.first.get(path)
    }
}
