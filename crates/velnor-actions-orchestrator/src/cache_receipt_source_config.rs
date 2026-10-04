//! Metadata comes solely from an opaque independently admitted pure producer.
use super::{OrchestratorError, invalid};
use serde_json::{Value, json};
use velnor_actions_contract::StepKind;
use velnor_actions_mise::catalog::qualification::{
    DistributionHost, DistributionTool, QualifiedDistribution,
};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;

pub(super) fn configuration(recipe: &CacheProducerRecipe) -> Result<Value, OrchestratorError> {
    let original = recipe.original();
    let super::role_config::ProducerMetadata {
        role,
        descriptor,
        catalog,
        save,
    } = super::role_config::metadata(original)?;
    let transport = original
        .steps
        .iter()
        .find(|step| step.id.as_ref() == Some(save))
        .ok_or_else(|| invalid("missing_admitted_transport"))?;
    let StepKind::Action { with, .. } = &transport.kind else {
        return Err(invalid("missing_admitted_transport"));
    };
    let layout = super::transport_layout::derive(recipe)?;
    let helper_closure = recipe
        .source_helpers()
        .iter()
        .map(|record| {
            json!({
                "invocation": record.invocation(), "environment": record.environment(),
                "source_sha256": record.invocation().descriptor().source_sha256(),
            })
        })
        .collect::<Vec<_>>();
    let digest = velnor_actions_contract::cache_producer_recipe_digest(original)?;
    Ok(json!({
        "schema": 1, "role": role, "recipe_sha256": digest,
        "descriptor_sha256": sha(&descriptor)?, "helper_sha256": sha(&helper_closure)?,
        "catalog_sha256": velnor_actions_contract::compiled_source_sha256(catalog.as_bytes()),
        "allowed_roots": layout.payload_roots(), "optional_roots": layout.optional_roots(),
        "transport_layout": layout.descriptor(),
        "source_compatibility_projection": super::projection::rust_source(recipe)?,
        "cache_key_expression": with.get("key").ok_or_else(|| invalid("missing_admitted_key"))?,
        "producer_job_name": original.display_name,
        "predicate_type": "https://velnor.dev/cache-producer/v1",
    }))
}

pub(super) fn consumer_configuration(
    recipe: &CacheProducerRecipe,
    producer: &Value,
) -> Result<Value, OrchestratorError> {
    let mut consumer = producer.clone();
    consumer["gh_distribution"] = gh(recipe.original().runs_on.as_str())?;
    consumer["publication"] = Value::Null;
    // No qualified policy object or shared archive projection can be issued yet.
    consumer["materializer_source_binding"] = Value::Null;
    consumer["archive_projection"] = Value::Null;
    consumer["trusted_root"] = trusted_root()?;
    Ok(consumer)
}

fn trusted_root() -> Result<Value, OrchestratorError> {
    let root = super::trust_root::qualified_public_receipt_root()
        .ok_or_else(|| invalid("public_trust_root_unqualified"))?;
    let text = |bytes: &'static [u8]| {
        std::str::from_utf8(bytes).map_err(|_| invalid("trust_root_encoding"))
    };
    Ok(json!({
        "raw": text(root.raw_bytes())?, "raw_sha256": root.raw_sha256(),
        "verifier": text(root.verifier_bytes())?, "verifier_sha256": root.verifier_sha256(),
        "qualification": text(root.qualification_bytes())?,
        "qualification_sha256": root.qualification_sha256(),
        "supports_public": root.supports_visibility("public"),
    }))
}

fn sha(value: &impl serde::Serialize) -> Result<String, OrchestratorError> {
    Ok(velnor_actions_contract::compiled_source_sha256(
        velnor_actions_contract::canonical_json_str(value)?.as_bytes(),
    ))
}

fn gh(label: &str) -> Result<Value, OrchestratorError> {
    let target = velnor_actions_contract::tool_target_for_runner_label(label)
        .ok_or_else(|| invalid("unknown_host"))?;
    let (host, machine) = match target {
        "x86_64-unknown-linux-gnu" => (DistributionHost::LinuxAmd64, "x86_64"),
        "aarch64-unknown-linux-gnu" => (DistributionHost::LinuxArm64, "aarch64"),
        "aarch64-apple-darwin" => return Ok(Value::Null),
        _ => return Err(invalid("verifier_host_unqualified")),
    };
    let distribution = QualifiedDistribution::qualify_native(DistributionTool::Gh, host, "2.102.0")
        .map_err(|error| OrchestratorError::Contract {
            problem: error.to_string(),
        })?;
    Ok(
        json!({ "tool": "gh", "version": distribution.version(), "machine": machine,
            "binary_sha256": distribution.binary_sha256(),
            "qualification_sha256": distribution.qualification_digest(),
            "asset_url": distribution.asset_url(), "archive_sha256": distribution.archive_sha256(),
            "binary_member": distribution.binary_member(), "source_commit": distribution.source_commit(),
            "source_repository": distribution.source_repository(),
            "source_tree": distribution.source_tree(),
        }),
    )
}
