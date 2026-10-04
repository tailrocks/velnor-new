use super::*;
use crate::{JobTimeout, SourceProducer, SourceProducerRole, StepId, ToolProducerSelection};

fn job() -> Job {
    Job {
        cache_mode: None,
        display_name: "Consumer".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps: Vec::new(),
    }
}

#[test]
fn schema_accepts_exact_four_literals_and_rejects_expressions() {
    for (literal, mode) in [
        ("read", CacheMode::Read),
        ("write", CacheMode::Write),
        ("write-only", CacheMode::WriteOnly),
        ("none", CacheMode::None),
    ] {
        assert_eq!(
            serde_json::from_value::<CacheMode>(serde_json::json!(literal)).expect("mode"),
            mode
        );
        assert_eq!(
            serde_json::to_value(mode).expect("serialized mode"),
            literal
        );
        assert_eq!(mode.as_str(), literal);
    }
    for literal in [
        "READ",
        "read-only",
        "${{ github.event_name == 'push' && 'write' || 'read' }}",
    ] {
        assert!(serde_json::from_value::<CacheMode>(serde_json::json!(literal)).is_err());
    }
}

#[test]
fn workflow_default_is_mandatory_and_cannot_grant_publication() {
    let mut value = serde_json::json!({
        "cache_mode": "read", "name": "CI", "run_name": null,
        "triggers": {"pull_request_types": [], "push_branches": [], "merge_group": false},
        "permissions": {}, "concurrency": {"group": "ci", "cancel_in_progress": "false"},
        "jobs": {"consumer": serde_json::to_value(job()).expect("job")}
    });
    // Use the exact contract permission shape rather than a permissive partial fixture.
    value["permissions"] =
        serde_json::to_value(crate::Permissions::default()).expect("permissions");
    let mut ir: WorkflowIr = serde_json::from_value(value.clone()).expect("read workflow");
    assert!(validate_workflow(&ir).is_ok());
    for mode in [CacheMode::Write, CacheMode::WriteOnly, CacheMode::None] {
        ir.cache_mode = mode;
        assert!(validate_workflow(&ir).is_err());
    }
    value.as_object_mut().expect("object").remove("cache_mode");
    assert!(serde_json::from_value::<WorkflowIr>(value).is_err());
}

#[test]
fn consumers_and_signers_cannot_obtain_cache_write_authority() {
    let mut job = job();
    for mode in [None, Some(CacheMode::Read), Some(CacheMode::None)] {
        job.cache_mode = mode;
        assert!(validate_job(&job).is_ok());
    }
    for mode in [CacheMode::Write, CacheMode::WriteOnly] {
        job.cache_mode = Some(mode);
        job.condition = Some(super::super::cache_trust::CACHE_SAVE_CONDITION.to_owned());
        assert!(validate_job(&job).is_err());
    }
}

#[test]
fn source_writer_requires_exact_canonical_protected_selection() {
    let mut job = job();
    let id = |value| StepId::new(value).expect("fixed step id");
    let source = SourceProducer {
        role: SourceProducerRole::Npm,
        selection: ToolProducerSelection {
            unconditional: true,
            ..ToolProducerSelection::default()
        },
        tool_cache: None,
        source_identity: "source-v1".to_owned(),
        verification_step: id("verify"),
        restore_step: id("restore"),
        save_step: id("save"),
        publication_step: id("publication"),
        report_step: id("report"),
    };
    job.cache_mode = Some(CacheMode::Write);
    job.condition = Some(source.condition());
    job.source_producer = Some(source);
    assert!(validate_job(&job).is_ok());
    job.condition = Some(super::super::cache_trust::CACHE_SAVE_CONDITION.to_owned());
    assert!(validate_job(&job).is_err());
    job.condition = Some("github.event_name == 'pull_request'".to_owned());
    assert!(validate_job(&job).is_err());
}
