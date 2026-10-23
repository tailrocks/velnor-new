use std::cell::Cell;
use std::error::Error;
use std::time::Instant;

use serde_json::Value;
use velnor_actions_contract::canonical_json_bytes;

use super::super::context::ContextValues;
use super::super::{Collector, Invocation, bundle};
use super::spec;
use crate::OrchestratorError;
use crate::api::github_evidence_http::{
    ArtifactArchiveRequest, DownloadReceipt, EvidenceRoute, GitHubEvidenceRead, GitHubRepository,
};

#[derive(Default)]
struct FakeReader {
    json_calls: Cell<usize>,
}

impl GitHubEvidenceRead for FakeReader {
    fn json(
        &self,
        _repo: &GitHubRepository,
        _route: &EvidenceRoute,
        _token: &str,
        _deadline: Instant,
    ) -> Result<Vec<u8>, OrchestratorError> {
        self.json_calls.set(self.json_calls.get() + 1);
        Ok(b"{}".to_vec())
    }

    fn archive(
        &self,
        _request: ArtifactArchiveRequest<'_>,
    ) -> Result<DownloadReceipt, OrchestratorError> {
        Err(crate::internal::internal(
            "admission_test_unexpected_archive",
        ))
    }
}

fn valid_context() -> ContextValues {
    let spec = spec();
    let spec_text = String::from_utf8(canonical_json_bytes(&spec).expect("spec serializes"))
        .expect("canonical spec is UTF-8");
    ContextValues {
        internal_op: Some(velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION.to_owned()),
        spec_text: Some(spec_text),
        ref_name: Some("refs/heads/main".to_owned()),
        sha: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned()),
        repository: Some("tailrocks/velnor".to_owned()),
        repository_id: Some("1".to_owned()),
        run_id: Some("900".to_owned()),
        run_attempt: Some("1".to_owned()),
        token: Some("fixture-token".to_owned()),
        event_name: Some("schedule".to_owned()),
        job: Some("github-evidence-fetch-collector".to_owned()),
        workflow_ref: Some(
            "tailrocks/velnor/.github/workflows/nightly.yml@refs/heads/main".to_owned(),
        ),
    }
}

fn read_after_admission(
    context: ContextValues,
    reader: &FakeReader,
) -> Result<Value, Box<dyn Error>> {
    let invocation = Invocation::from_context(context)?;
    let temp = tempfile::tempdir()?;
    let bundle = bundle::BundleRoot::create_at(temp.path())?;
    let collector = Collector::new(reader, invocation, bundle);
    let (_, value) = collector.read_json::<Value>(&EvidenceRoute::Repository)?;
    Ok(value)
}

#[test]
fn valid_schedule_context_uses_shared_admission_before_fake_reader() {
    let reader = FakeReader::default();
    let value = read_after_admission(valid_context(), &reader).expect("admitted fake read");
    assert_eq!(value, serde_json::json!({}));
    assert_eq!(reader.json_calls.get(), 1);
}

#[test]
fn invalid_fetch_contexts_stop_before_fake_reader() {
    enum Field {
        Ref,
        Sha,
        Repository,
        RepositoryId,
        RunId,
        RunAttempt,
        Job,
        WorkflowRef,
    }

    let base = valid_context();
    let mut invalid = Vec::new();

    let mut context = base.clone();
    context.internal_op = None;
    invalid.push(context);
    let mut context = base.clone();
    context.internal_op = Some("private-fetch-v1".to_owned());
    invalid.push(context);
    let mut context = base.clone();
    context.spec_text = None;
    invalid.push(context);
    let mut context = base.clone();
    context.spec_text = Some("{}".to_owned());
    invalid.push(context);
    for event_name in ["pull_request", "workflow_dispatch", "push"] {
        let mut context = base.clone();
        context.event_name = Some(event_name.to_owned());
        invalid.push(context);
    }
    for (field, value) in [
        (Field::Ref, "refs/heads/feature"),
        (Field::Sha, "not-a-commit"),
        (Field::Repository, "fork/velnor"),
        (Field::RepositoryId, "0"),
        (Field::RunId, "0"),
        (Field::RunAttempt, "0"),
        (Field::Job, "build"),
        (
            Field::WorkflowRef,
            "tailrocks/velnor/.github/workflows/other.yml@refs/heads/main",
        ),
    ] {
        let mut context = base.clone();
        match field {
            Field::Ref => context.ref_name = Some(value.to_owned()),
            Field::Sha => context.sha = Some(value.to_owned()),
            Field::Repository => context.repository = Some(value.to_owned()),
            Field::RepositoryId => context.repository_id = Some(value.to_owned()),
            Field::RunId => context.run_id = Some(value.to_owned()),
            Field::RunAttempt => context.run_attempt = Some(value.to_owned()),
            Field::Job => context.job = Some(value.to_owned()),
            Field::WorkflowRef => context.workflow_ref = Some(value.to_owned()),
        }
        invalid.push(context);
    }
    let mut context = base;
    context.token = None;
    invalid.push(context);

    for context in invalid {
        let reader = FakeReader::default();
        assert!(read_after_admission(context, &reader).is_err());
        assert_eq!(reader.json_calls.get(), 0);
    }
}
