//! Cache-service permissions render as literal typed authority.
use super::impl_renderer_fixtures::{checkout_pin, fixture_ir, job};
use velnor_actions_contract::CacheMode;
use velnor_actions_workflow_renderer::{
    WorkflowDocumentContext, checkout_step, render_workflow_document,
};

#[test]
fn workflows_default_read_and_consumers_render_only_safe_overrides() {
    let mut consumer = job(
        "consumer",
        "Consumer",
        Vec::new(),
        vec![checkout_step(&checkout_pin()).expect("pin")],
    );
    let ctx = WorkflowDocumentContext {
        generator_version: "0.1.0".to_owned(),
        source_helpers: Vec::new(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    for mode in [None, Some(CacheMode::Read), Some(CacheMode::None)] {
        consumer.1.cache_mode = mode;
        let ir = fixture_ir(vec![consumer.clone()]);
        let text = render_workflow_document(&ir, &ctx).expect("safe cache mode");
        assert!(text.contains("\ncache-mode: read\n"));
        let count = text.matches("cache-mode:").count();
        assert_eq!(count, if mode.is_some() { 2 } else { 1 });
        if let Some(mode) = mode {
            assert!(text.contains(&format!("    cache-mode: {}\n", mode.as_str())));
        }
        assert!(!text.contains("cache-mode: ${{"));
    }
    for mode in [CacheMode::Write, CacheMode::WriteOnly] {
        consumer.1.cache_mode = Some(mode);
        assert!(render_workflow_document(&fixture_ir(vec![consumer.clone()]), &ctx).is_err());
    }
}

#[test]
fn native_documents_cannot_bypass_structural_producer_admission() {
    use velnor_actions_contract::{
        SourceProducer, SourceProducerRole, StepId, ToolProducerSelection,
    };
    let id = |value| StepId::new(value).expect("step id");
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
    let mut producer = job(
        "source",
        "Source",
        Vec::new(),
        vec![checkout_step(&checkout_pin()).expect("pin")],
    );
    producer.1.cache_mode = Some(CacheMode::Write);
    producer.1.condition = Some(source.condition());
    producer.1.source_producer = Some(source);
    let ir = fixture_ir(vec![producer]);
    ir.validate()
        .expect("metadata alone satisfies neutral contract");
    let ctx = WorkflowDocumentContext {
        generator_version: "0.1.0".to_owned(),
        source_helpers: Vec::new(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    let error = render_workflow_document(&ir, &ctx).expect_err("writer needs structural admission");
    assert!(
        error
            .to_string()
            .contains("cache_writer_requires_strict_admission")
    );
}
