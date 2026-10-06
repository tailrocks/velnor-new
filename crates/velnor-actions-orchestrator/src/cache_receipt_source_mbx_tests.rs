//! MBX structural recipes alone never grant signing source authority.
use super::*;
use velnor_actions_contract::{
    MbxCacheDomain, MbxExportDescriptor, MbxOwnerIdentity, ToolProducerSelection,
};
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{
    WorkflowDocumentContext, cache_producer_workflow::admit_cache_producer_recipe,
};

fn fixture() -> (CacheProducerRecipe, crate::mbx_producer::DraftMbxProducer) {
    let task = "stack/rust/default/check/default".to_owned();
    let descriptor = MbxExportDescriptor {
        domain: MbxCacheDomain::Validation,
        producer_job_id: "rust-default".into(),
        runs_on: "ubuntu-26.04".into(),
        target: "x86_64-unknown-linux-gnu".into(),
        workspace_roots: vec![".".into()],
        configuration_digest: velnor_actions_contract::digest_b3(b"configuration"),
        task_digests: [(task.clone(), velnor_actions_contract::digest_b3(b"task"))].into(),
        owner: MbxOwnerIdentity {
            version: "0.9.0".into(),
            binary_sha256: "a".repeat(64),
            qualification_identity: "b".repeat(64),
            source_sha: "c".repeat(40),
        },
        action_sha: "d".repeat(40),
    };
    let setup = crate::test_mise::setup("2026.9.16", &"a".repeat(64));
    let owner = crate::mbx_producer::draft_mbx_producer(
        &descriptor,
        &ToolProducerSelection {
            tasks: vec![task],
            cargo_fallback: false,
            unconditional: false,
        },
        &ToolCatalog::pinned(),
        &setup,
        env!("CARGO_PKG_VERSION"),
    )
    .expect("fixed unsupported MBX source owner");
    let context = WorkflowDocumentContext {
        generator_version: owner.generator_version().into(),
        source_helpers: owner.source_helpers().to_vec(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    let recipe = admit_cache_producer_recipe(owner.original(), &setup, &context)
        .expect("structural MBX recipe");
    (recipe, owner)
}

#[test]
fn mbx_requires_independent_source_capability_and_never_activates_publication() {
    let (recipe, owner) = fixture();
    assert!(draft_cache_receipt_sources(&recipe).is_err());
    let sources = draft_cache_receipt_sources_with_mbx(&recipe, &owner)
        .expect("whole owner reconstructed before source generation");
    assert_eq!(sources.producer_descriptor()["role"], "mbx-validation");
    assert_eq!(sources.transport_layout().transport_paths().len(), 2);
    assert!(qualified_cache_receipt_publication().is_none());
    let draft =
        crate::cache_producer_workflow::render_cache_producer_workflow_draft(&recipe, &sources)
            .expect("closed unsupported publication candidate");
    assert!(draft.file.bytes.contains("actions: read"));
    assert!(draft.file.bytes.contains("id-token: write"));
    assert!(draft.file.bytes.contains("attestations: write"));
    assert!(
        sources
            .verify()
            .source()
            .contains("producer_policy_unqualified")
    );
    for record in owner.source_helpers() {
        assert!(!record.source().contains("admitted=true"));
        assert!(!record.source().contains("verified=true"));
    }
}

#[test]
fn neutral_recipe_digest_requires_a_role_and_binds_all_permissions() {
    let (recipe, _) = fixture();
    let original = recipe.original();
    velnor_actions_contract::cache_producer_recipe_digest(original).expect("neutral MBX digest");
    let mut missing = original.clone();
    missing.mbx_producer = None;
    assert!(velnor_actions_contract::cache_producer_recipe_digest(&missing).is_err());
    let mut wrong_permission = original.clone();
    wrong_permission
        .permissions
        .as_mut()
        .expect("permissions")
        .actions = velnor_actions_contract::PermissionLevel::Write;
    assert_ne!(
        velnor_actions_contract::cache_producer_recipe_digest(original).expect("original"),
        velnor_actions_contract::cache_producer_recipe_digest(&wrong_permission)
            .expect("neutral changed digest")
    );
}
