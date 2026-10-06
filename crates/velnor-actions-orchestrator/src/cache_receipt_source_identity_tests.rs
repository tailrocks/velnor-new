//! Consumer publication changes must never rewrite a producer signing program.
use super::*;
use velnor_actions_contract::ToolProducerSelection;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_workflow_renderer::{
    WorkflowDocumentContext, cache_producer_workflow::admit_cache_producer_recipe,
};

fn recipe() -> CacheProducerRecipe {
    let sources = vec![crate::workloads::cache_eligibility::NativeNpmSource {
        name: "typescript".into(), version: "5.6.3".into(),
        resolved: "https://registry.npmjs.org/typescript/-/typescript-5.6.3.tgz".into(),
        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==".into(),
    }];
    let setup = crate::test_mise::setup("2026.9.16", &"a".repeat(64));
    let selection = ToolProducerSelection {
        tasks: vec!["stack/node/example/test/default".into()],
        cargo_fallback: false,
        unconditional: false,
    };
    let owner = native::owner_recipe(
        native::NativeSourceInputs::Npm(&sources),
        &ToolCatalog::pinned(),
        "ubuntu-26.04",
        &setup,
        env!("CARGO_PKG_VERSION"),
        &selection,
    )
    .expect("native source owner");
    let context = WorkflowDocumentContext {
        generator_version: env!("CARGO_PKG_VERSION").into(),
        source_helpers: owner.source_helpers().to_vec(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    };
    admit_cache_producer_recipe(owner.original(), &setup, &context).expect("admitted recipe")
}

#[test]
fn producer_registry_excludes_consumer_authority_and_unsigned_descriptor_has_no_self_hash() {
    let recipe = recipe();
    let descriptor = config::configuration(&recipe).expect("unsigned descriptor");
    for key in [
        "policy_sha256",
        "publication",
        "gh_distribution",
        "trusted_root",
    ] {
        assert!(descriptor.get(key).is_none());
    }
    let modules = body::producer_modules();
    assert_eq!(modules.len(), 9);
    for name in [
        "cache_receipt_policy",
        "cache_receipt_gh",
        "cache_receipt_api",
        "cache_receipt",
    ] {
        assert!(!modules.contains_key(name));
    }
    assert!(!runtime::ENTRYPOINT.contains("_verify"));
    assert!(!runtime::ENTRYPOINT.contains("qualified_policy"));
}

#[test]
fn consumer_activation_and_root_upgrades_leave_producer_records_byte_identical() {
    let recipe = recipe();
    let sources = compile_sources(&recipe).expect("draft closed source");
    let mut configuration = config::configuration(&recipe).expect("unsigned descriptor");
    let modules = body::producer_modules();
    configuration["policy_sha256"] =
        serde_json::json!(velnor_actions_contract::compiled_source_sha256(
            velnor_actions_contract::canonical_json_str(&serde_json::json!({
                "unsigned_producer_descriptor": configuration,
                "producer_closure_sha256": body::closure_sha256(&modules, false).expect("closure"),
                "attest_uses": "actions/attest@1e69f48acb82d1966a394da916b4c1698aa569d6",
            }))
            .expect("canonical descriptor")
            .as_bytes()
        ));
    let mut consumer = config::consumer_configuration(&recipe, &configuration).expect("consumer");
    consumer["publication"] =
        serde_json::json!({"signer_sha":"f".repeat(40),"caller_sha":"e".repeat(40)});
    consumer["trusted_root"] = serde_json::json!({"verifier_sha256":"d".repeat(64)});
    consumer["gh_distribution"]["qualification_sha256"] = serde_json::json!("c".repeat(64));
    let mut consumer_modules =
        body::consumer_modules(&consumer, &modules).expect("consumer modules");
    consumer_modules
        .get_mut("cache_receipt_policy")
        .expect("policy")
        .push_str("\n# changed immutable published consumer policy\n");
    let verify = body::record(
        &recipe,
        SourceBoundOperation::CacheReceiptVerify,
        "verify",
        &consumer,
        &consumer_modules,
    )
    .expect("consumer record");
    assert_ne!(verify, *sources.verify());
    assert_ne!(
        body::consumer_closure_sha256(&consumer_modules, &consumer).expect("consumer closure"),
        sources.consumer_closure_sha256()
    );
    for (operation, command, expected) in [
        (
            SourceBoundOperation::CacheReceiptManifest,
            "manifest",
            sources.manifest(),
        ),
        (
            SourceBoundOperation::CacheReceiptBundle,
            "bundle",
            sources.bundle(),
        ),
    ] {
        assert_eq!(
            body::record(&recipe, operation, command, &configuration, &modules)
                .expect("frozen producer record"),
            *expected
        );
    }
    assert_eq!(
        body::closure_sha256(&modules, false).expect("producer closure"),
        sources.producer_closure_sha256()
    );
}

#[test]
fn transport_layout_is_identical_in_frozen_producer_and_consumer_descriptors() {
    let recipe = recipe();
    let sources = compile_sources(&recipe).expect("sources");
    let layout = sources.transport_layout();
    assert_eq!(
        sources.producer_descriptor()["transport_layout"],
        layout.descriptor()
    );
    let consumer = config::consumer_configuration(&recipe, sources.producer_descriptor())
        .expect("consumer descriptor");
    assert_eq!(consumer["transport_layout"], layout.descriptor());
    let paths = layout.transport_paths().split('\n').collect::<Vec<_>>();
    assert_eq!(paths.len(), layout.payload_roots().len() + 1);
    assert_eq!(
        paths[layout.evidence_index()],
        format!("${{{{ runner.temp }}}}/velnor/{}", layout.evidence_root())
    );
    assert_eq!(
        transport_layout::derive_for_descriptor(&recipe, &consumer).expect("exact consumer layout"),
        *layout
    );
    for record in [sources.manifest(), sources.bundle(), sources.verify()] {
        assert!(!record.source().contains("${{"));
    }
}

#[test]
fn non_cargo_recipe_cannot_claim_rust_source_compatibility_projection() {
    let recipe = recipe();
    assert!(
        projection::rust_source(&recipe)
            .expect("source owner projection")
            .is_none()
    );
    let configuration = config::configuration(&recipe).expect("frozen descriptor");
    assert!(configuration["source_compatibility_projection"].is_null());
}

#[test]
fn materializer_is_consumer_only_and_no_source_or_projection_authority_is_issued() {
    let recipe = recipe();
    let sources = compile_sources(&recipe).expect("closed consumer source");
    let consumer = config::consumer_configuration(&recipe, sources.producer_descriptor())
        .expect("consumer config");
    for key in ["materializer_source_binding", "archive_projection"] {
        assert!(sources.producer_descriptor().get(key).is_none());
        assert!(consumer[key].is_null());
    }
    let producer = body::producer_modules();
    let modules = body::consumer_modules(&consumer, &producer).expect("consumer modules");
    for name in [
        "cache_receipt_materialize_transaction",
        "cache_receipt_materialize",
    ] {
        assert!(!producer.contains_key(name));
        assert!(modules.contains_key(name));
    }
    for declaration in [
        "_COMPILED_SOURCE_BINDING = None",
        "_COMPILED_PROJECTION = None",
        "_COMPILED_PROJECTION_TYPE = None",
    ] {
        assert_eq!(
            modules["cache_receipt_materialize"]
                .matches(declaration)
                .count(),
            1
        );
    }
    assert!(!consumer_runtime::ENTRYPOINT.contains("MaterializationStop"));
}

#[test]
fn fresh_gh_registry_is_consumer_only_and_issues_no_runtime_authority() {
    let producer = body::producer_modules();
    let modules = body::consumer_modules(&serde_json::json!({"gh_distribution": null}), &producer)
        .expect("fixed cold consumer registry");
    for name in [
        "receipt_fresh_gh_download",
        "receipt_fresh_gh_archive",
        "receipt_fresh_gh",
    ] {
        assert!(!producer.contains_key(name));
        assert!(modules.contains_key(name));
    }
    for literal in [
        "_COMPILED_TRUSTED_ROOT = None",
        "_COMPILED_REPOSITORY = None",
    ] {
        assert!(modules["receipt_fresh_gh"].contains(literal));
    }
    assert!(consumer_runtime::ENTRYPOINT.contains("producer_policy_unqualified"));
}

#[test]
fn consumer_gh_projection_preserves_the_complete_sdk_source_identity() {
    use velnor_actions_mise::catalog::qualification::{
        DistributionHost, DistributionTool, QualifiedDistribution,
    };
    let recipe = recipe();
    let producer = config::configuration(&recipe).expect("producer descriptor");
    assert!(producer.get("gh_distribution").is_none());
    let consumer = config::consumer_configuration(&recipe, &producer).expect("consumer descriptor");
    let expected = QualifiedDistribution::qualify_native(
        DistributionTool::Gh,
        DistributionHost::LinuxAmd64,
        "2.102.0",
    )
    .expect("fixed SDK qualification");
    assert_eq!(
        consumer["gh_distribution"]["source_repository"],
        expected.source_repository()
    );
    assert_eq!(
        consumer["gh_distribution"]["source_commit"],
        expected.source_commit()
    );
    assert_eq!(
        consumer["gh_distribution"]["source_tree"],
        expected.source_tree()
    );
}
