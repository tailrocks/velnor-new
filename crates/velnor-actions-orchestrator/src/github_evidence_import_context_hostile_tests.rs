use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_actions_contract::{GitHubEvidenceSnapshotV1, canonical_json_bytes};

use super::super::super::import::{ImportContext, import_with_context};
use super::support::FixtureReader;
use super::support::{collect, spec};

#[test]
fn valid_full_collection_imports_and_one_field_context_or_spec_mutations_fail_closed() {
    let capture = tempfile::tempdir().expect("capture root");
    let snapshot = collect(&FixtureReader::default(), &capture).expect("full captured bundle");
    let bundle = capture.path().join("velnor/github-evidence-bundle");
    let expected_spec = spec();

    let workspace = workspace();
    let context = valid_context(&snapshot);
    import_with_context(&expected_spec, &bundle, workspace.path(), &context)
        .expect("valid full collected bundle imports");
    assert!(
        workspace
            .path()
            .join(".velnor/evidence/index.json")
            .is_file()
    );
    assert_eq!(
        fs::read(workspace.path().join(".velnor/evidence/index.json")).expect("published index"),
        fs::read(bundle.join("index.json")).expect("source index")
    );

    for mutation in [
        ContextMutation::RepositoryId,
        ContextMutation::RepositoryName,
        ContextMutation::RunId,
        ContextMutation::RunAttempt,
        ContextMutation::Sha,
        ContextMutation::Reference,
        ContextMutation::Job,
        ContextMutation::WorkflowRepository,
        ContextMutation::WorkflowPath,
        ContextMutation::WorkflowReference,
        ContextMutation::Event,
        ContextMutation::GhTokenPresent,
    ] {
        let mut context = valid_context(&snapshot);
        mutate_context(&mut context, mutation);
        reject_without_publish(&expected_spec, &bundle, &context);
    }

    let mut changed_spec = expected_spec.clone();
    changed_spec.collector_suite_id = "other-suite".to_owned();
    reject_without_publish(&changed_spec, &bundle, &valid_context(&snapshot));

    let mut changed_spec = expected_spec;
    changed_spec.collector_workflow_path = ".github/workflows/other.yml".to_owned();
    reject_without_publish(&changed_spec, &bundle, &valid_context(&snapshot));
}

#[test]
fn actual_importer_rejects_malformed_duplicate_and_noncanonical_indexes() {
    for bytes in [b"{".as_slice(), br#"{"schema":1,"schema":1}"#.as_slice()] {
        let (capture, snapshot) = captured_bundle();
        let bundle = capture.path().join("velnor/github-evidence-bundle");
        fs::write(bundle.join("index.json"), bytes).expect("replace index");
        reject_bundle_without_publish(&bundle, &snapshot);
    }

    let (capture, snapshot) = captured_bundle();
    let bundle = capture.path().join("velnor/github-evidence-bundle");
    let index = fs::read(bundle.join("index.json")).expect("canonical index");
    let mut noncanonical = Vec::with_capacity(index.len() + 1);
    noncanonical.push(b' ');
    noncanonical.extend_from_slice(&index);
    fs::write(bundle.join("index.json"), noncanonical).expect("rewrite noncanonical index");
    reject_bundle_without_publish(&bundle, &snapshot);
}

#[test]
fn actual_importer_rejects_malformed_metadata_with_matching_indexed_digest() {
    let (capture, snapshot) = captured_bundle();
    let bundle = capture.path().join("velnor/github-evidence-bundle");
    let malformed = br#"{"id":1,"id":2}"#;
    fs::write(bundle.join("documents/repository.json"), malformed)
        .expect("replace repository metadata");
    let mut amended = snapshot;
    let document = amended
        .documents
        .iter_mut()
        .find(|document| document.path == "documents/repository.json")
        .expect("repository metadata entry");
    document.size_bytes = malformed.len() as u64;
    document.sha256 = digest(malformed);
    fs::write(
        bundle.join("index.json"),
        canonical_json_bytes(&amended).expect("canonical amended index"),
    )
    .expect("write amended index");
    reject_bundle_without_publish(&bundle, &amended);
}

#[cfg(unix)]
#[test]
fn actual_importer_rejects_symlink_and_special_bundle_files_before_publish() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    let (capture, snapshot) = captured_bundle();
    let bundle = capture.path().join("velnor/github-evidence-bundle");
    let document = bundle.join("documents/repository.json");
    let external = capture.path().join("external-repository.json");
    fs::copy(&document, &external).expect("copy link target");
    fs::remove_file(&document).expect("remove document");
    symlink(&external, &document).expect("create bundle symlink");
    reject_bundle_without_publish(&bundle, &snapshot);

    let capture = tempfile::tempdir_in("/tmp").expect("short socket capture root");
    let snapshot = collect(&FixtureReader::default(), &capture).expect("full captured bundle");
    let bundle = capture.path().join("velnor/github-evidence-bundle");
    let archive = bundle.join("archives/501.zip");
    fs::remove_file(&archive).expect("remove archive");
    let special = UnixListener::bind(&archive).expect("create local socket bundle entry");
    reject_bundle_without_publish(&bundle, &snapshot);
    drop(special);
}

#[derive(Clone, Copy)]
enum ContextMutation {
    RepositoryId,
    RepositoryName,
    RunId,
    RunAttempt,
    Sha,
    Reference,
    Job,
    WorkflowRepository,
    WorkflowPath,
    WorkflowReference,
    Event,
    GhTokenPresent,
}

fn mutate_context(context: &mut ImportContext, mutation: ContextMutation) {
    match mutation {
        ContextMutation::RepositoryId => context.repository_id += 1,
        ContextMutation::RepositoryName => context.repository_name = "fork".to_owned(),
        ContextMutation::RunId => context.run_id += 1,
        ContextMutation::RunAttempt => context.run_attempt += 1,
        ContextMutation::Sha => context.sha = "c".repeat(40),
        ContextMutation::Reference => context.reference = "refs/heads/release".to_owned(),
        ContextMutation::Job => context.job = "mise-task-other-suite".to_owned(),
        ContextMutation::WorkflowRepository => {
            context.workflow_repository = "tailrocks/fork".to_owned();
        }
        ContextMutation::WorkflowPath => context.workflow_path = "other.yml".to_owned(),
        ContextMutation::WorkflowReference => {
            context.workflow_reference = "refs/heads/release".to_owned();
        }
        ContextMutation::Event => context.event_name = "push".to_owned(),
        ContextMutation::GhTokenPresent => context.evidence_token_present = true,
    }
}

fn valid_context(snapshot: &GitHubEvidenceSnapshotV1) -> ImportContext {
    ImportContext {
        repository_owner: "tailrocks".to_owned(),
        repository_name: "velnor".to_owned(),
        repository_id: snapshot.repository.id,
        run_id: snapshot.acquisition.run_id,
        run_attempt: snapshot.acquisition.run_attempt,
        event_name: "schedule".to_owned(),
        job: "mise-task-collector".to_owned(),
        reference: "refs/heads/main".to_owned(),
        sha: snapshot.acquisition.collector_sha.clone(),
        workflow_repository: "tailrocks/velnor".to_owned(),
        workflow_path: "nightly.yml".to_owned(),
        workflow_reference: "refs/heads/main".to_owned(),
        evidence_token_present: false,
    }
}

fn captured_bundle() -> (tempfile::TempDir, GitHubEvidenceSnapshotV1) {
    let capture = tempfile::tempdir().expect("capture root");
    let snapshot = collect(&FixtureReader::default(), &capture).expect("full captured bundle");
    (capture, snapshot)
}

fn workspace() -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("workspace");
    fs::create_dir(workspace.path().join(".velnor")).expect("configuration directory");
    workspace
}

fn reject_without_publish(
    spec: &velnor_actions_contract::GitHubEvidenceFetchSpecV1,
    bundle: &Path,
    context: &ImportContext,
) {
    let workspace = workspace();
    assert!(import_with_context(spec, bundle, workspace.path(), context).is_err());
    assert_not_published(workspace.path());
}

fn reject_bundle_without_publish(bundle: &Path, snapshot: &GitHubEvidenceSnapshotV1) {
    let workspace = workspace();
    let context = valid_context(snapshot);
    assert!(import_with_context(&spec(), bundle, workspace.path(), &context).is_err());
    assert_not_published(workspace.path());
}

fn assert_not_published(workspace: &Path) {
    assert!(!workspace.join(".velnor/evidence").exists());
    assert!(!workspace.join(".velnor/evidence.importing").exists());
}

fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(output, "{byte:02x}").expect("format digest");
    }
    output
}
