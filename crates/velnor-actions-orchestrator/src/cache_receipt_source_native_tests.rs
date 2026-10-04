use super::*;
use velnor_actions_contract::{
    HelperInvocation, SourceBoundHelper, SourceBoundOperation, StepKind,
};
use velnor_actions_workflow_renderer::{
    WorkflowDocumentContext, cache_producer_workflow::admit_cache_producer_recipe,
};

fn owner(bun: bool) -> (NativeReceiptRecipe, MiseSetup) {
    let sources = vec![NativeNpmSource {
        name: "typescript".into(),
        version: "5.6.3".into(),
        resolved: "https://registry.npmjs.org/typescript/-/typescript-5.6.3.tgz".into(),
        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==".into(),
    }];
    let setup = crate::test_mise::setup("2026.9.16", &"a".repeat(64));
    let selection = ToolProducerSelection {
        tasks: vec!["stack/node/example/test/default".into()],
        cargo_fallback: false,
        unconditional: false,
    };
    let inputs = if bun {
        NativeSourceInputs::Bun(&sources)
    } else {
        NativeSourceInputs::Npm(&sources)
    };
    let owner = owner_recipe(
        inputs,
        &ToolCatalog::pinned(),
        "ubuntu-26.04",
        &setup,
        env!("CARGO_PKG_VERSION"),
        &selection,
    )
    .expect("actual native factory");
    (owner, setup)
}

fn context(owner: &NativeReceiptRecipe) -> WorkflowDocumentContext {
    WorkflowDocumentContext {
        generator_version: owner.version.clone(),
        source_helpers: owner.records.clone(),
        native_pages_approvals: Vec::new(),
        native_publish_approvals: Vec::new(),
        action_credential_approvals: Vec::new(),
    }
}

#[test]
fn actual_npm_and_bun_factories_reconstruct_complete_native_recipe() {
    for bun in [false, true] {
        let (owner, setup) = owner(bun);
        let recipe = admit_cache_producer_recipe(&owner.job, &setup, &context(&owner))
            .expect("shape admission");
        validate(&recipe, &owner).expect("exact owner computation");
    }
}

#[test]
fn native_candidate_bounds_and_shape_are_closed_before_factory_authority() {
    assert!(validate_candidates(&[]).is_err());
    let invalid = NativeNpmSource {
        name: "package".into(),
        version: "1.0.0".into(),
        resolved: "https://private.invalid/package.tgz".into(),
        integrity: "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==".into(),
    };
    assert!(validate_candidates(&[invalid]).is_err());
}

#[test]
fn native_environment_is_compared_as_part_of_the_complete_owner_record() {
    let (owner, _) = owner(false);
    let expected = owner
        .records
        .iter()
        .find(|record| {
            record.invocation().descriptor().operation()
                == SourceBoundOperation::NpmPublicSourceProducer
        })
        .expect("native producer source");
    let mut environment = expected.environment().clone();
    environment.insert("VELNOR_SOURCE_IDENTITY".into(), "foreign identity".into());
    let changed = expected.clone().with_environment(environment);
    require_record(&changed, &owner.records).expect_err("environment grants no authority");
}

#[test]
fn captured_record_vector_cannot_replace_fresh_factory_reconstruction() {
    let (mut owner, setup) = owner(false);
    let recipe =
        admit_cache_producer_recipe(&owner.job, &setup, &context(&owner)).expect("shape admission");
    owner.records.pop();
    let error =
        validate(&recipe, &owner).expect_err("captured vector alone cannot grant authority");
    assert!(
        error
            .to_string()
            .contains("native_producer_factory_changed")
    );
}

#[test]
fn correctly_tagged_arbitrary_native_source_cannot_pass_owner_admission() {
    for bun in [false, true] {
        let (owner, setup) = owner(bun);
        let operation = if bun {
            SourceBoundOperation::BunSourceProducer
        } else {
            SourceBoundOperation::NpmPublicSourceProducer
        };
        let expected = owner
            .records
            .iter()
            .find(|record| record.invocation().descriptor().operation() == operation)
            .expect("native producer source");
        let source =
            velnor_actions_contract::generated_source(&owner.version, "echo caller computation\n")
                .expect("valid marker");
        let descriptor = SourceBoundHelper::compiled(
            operation,
            operation.path(),
            &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
        )
        .expect("correct operation and digest");
        let invocation = HelperInvocation::compiled(
            descriptor,
            expected.invocation().args().to_vec(),
            expected.invocation().installed_selectors().to_vec(),
        )
        .expect("correct literal shape");
        let forged = CompiledSourceHelper::compiled(invocation, source)
            .expect("valid generic binding")
            .with_environment(expected.environment().clone());
        forged
            .validate_binding()
            .expect("generic binding remains valid");
        require_record(&forged, &owner.records).expect_err("owner rejects foreign bytes");
        let mut changed = owner.job.clone();
        for step in &mut changed.steps {
            if let StepKind::SourceBoundHelper { invocation, .. } = &mut step.kind {
                if invocation == expected.invocation() {
                    *invocation = forged.invocation().clone();
                }
            }
        }
        let mut registry = context(&owner);
        registry.source_helpers.retain(|record| record != expected);
        registry.source_helpers.push(forged);
        if let Ok(recipe) = admit_cache_producer_recipe(&changed, &setup, &registry) {
            validate(&recipe, &owner).expect_err("shape cannot grant owner authority");
        }
    }
}
