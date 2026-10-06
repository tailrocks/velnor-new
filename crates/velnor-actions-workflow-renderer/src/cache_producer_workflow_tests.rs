use super::*;

#[path = "cache_producer_workflow_fixture.rs"]
mod fixture;

#[test]
fn pure_recipe_admission_freezes_original_and_complete_source_registry() {
    let (original, setup, records) = fixture::fixture();
    let context = WorkflowDocumentContext {
        generator_version: "0.1.0".into(),
        source_helpers: records,
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    let recipe = admit_cache_producer_recipe(&original, &setup, &context).expect("pure recipe");
    assert_eq!(recipe.original(), &original);
    assert_eq!(recipe.source_helpers().len(), 5);
    let mut changed = original.clone();
    changed.steps.pop();
    assert!(admit_cache_producer_recipe(&changed, &setup, &context).is_err());
    let mut missing = context.clone();
    missing.source_helpers.pop();
    assert!(admit_cache_producer_recipe(&original, &setup, &missing).is_err());
    let mut ambiguous = context;
    ambiguous.source_helpers.push(fixture::fixture_record());
    assert!(admit_cache_producer_recipe(&original, &setup, &ambiguous).is_err());
}

#[test]
fn existing_foreign_attestation_permission_is_rejected_before_role_admission() {
    let mut job: Job = serde_json::from_value(serde_json::json!({
        "display_name": "Foreign signer", "runs_on": "ubuntu-24.04",
        "timeout_minutes": 10, "steps": [], "needs": []
    }))
    .expect("job");
    job.permissions = Some(velnor_actions_contract::Permissions {
        contents: PermissionLevel::None,
        actions: PermissionLevel::None,
        attestations: PermissionLevel::Write,
        ..velnor_actions_contract::Permissions::default()
    });
    let setup = crate::setup::fixture::mise_setup("2026.9.18", &"a".repeat(64));
    let error = admit_original(&job, &setup, &[]).expect_err("foreign permission");
    assert!(error.to_string().contains("foreign_authority"));
}
