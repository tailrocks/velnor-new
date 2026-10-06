//! Literal compiler-owned Python source modules inside the authenticated Bash body.
use super::{OrchestratorError, consumer_runtime, invalid, runtime};
use serde_json::Value;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;

const INITIALIZER: &str = "import json, sys, types\n_CONFIG, _SOURCES = json.loads(bytes.fromhex('__VELNOR_COMPILED_JSON_HEX__'))\n";

pub(super) fn producer_modules() -> BTreeMap<String, String> {
    use velnor_actions_mise::source_archive_inventory::{InventorySourceProgram, fixed_sources};
    let mut sources = BTreeMap::from([
        (
            "cache_receipt_common".into(),
            include_str!("cache_receipt_common.py").into(),
        ),
        (
            "cache_receipt_manifest".into(),
            include_str!("cache_receipt_manifest.py").into(),
        ),
    ]);
    sources.extend(
        fixed_sources(InventorySourceProgram::Archive)
            .into_iter()
            .map(|(name, source)| (name.to_owned(), source.to_owned())),
    );
    sources
}

pub(super) fn consumer_modules(
    configuration: &Value,
    producer: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let mut sources = producer.clone();
    sources.extend([
        (
            "cache_receipt_policy".into(),
            include_str!("cache_receipt_policy.py").into(),
        ),
        (
            "cache_receipt_virtual".into(),
            include_str!("cache_receipt_virtual.py").into(),
        ),
        (
            "cache_receipt_api".into(),
            include_str!("cache_receipt_api.py").into(),
        ),
        (
            "cache_receipt_gh".into(),
            include_str!("cache_receipt_gh.py").into(),
        ),
        (
            "receipt_fresh_gh_download".into(),
            include_str!("receipt_fresh_gh_download.py").into(),
        ),
        (
            "receipt_fresh_gh_archive".into(),
            include_str!("receipt_fresh_gh_archive.py").into(),
        ),
        (
            "receipt_fresh_gh".into(),
            include_str!("receipt_fresh_gh.py").into(),
        ),
        (
            "cache_receipt".into(),
            include_str!("cache_receipt.py").into(),
        ),
        (
            "cache_receipt_materialize_transaction".into(),
            include_str!("cache_receipt_materialize_transaction.py").into(),
        ),
        (
            "cache_receipt_materialize".into(),
            include_str!("cache_receipt_materialize.py").into(),
        ),
    ]);
    let distribution = configuration
        .get("gh_distribution")
        .ok_or_else(|| invalid("missing_gh_projection"))?;
    let literal = serde_json::to_string(
        &serde_json::to_string(distribution).map_err(|error| invalid(&error.to_string()))?,
    )
    .map_err(|error| invalid(&error.to_string()))?;
    let source = sources
        .get_mut("cache_receipt_gh")
        .ok_or_else(|| invalid("missing_gh_source"))?;
    if source.matches("_COMPILED_GH_DISTRIBUTION = None").count() != 1 {
        return Err(invalid("gh_projection_source_changed"));
    }
    *source = source.replace(
        "_COMPILED_GH_DISTRIBUTION = None",
        &format!("_COMPILED_GH_DISTRIBUTION = __import__('json').loads({literal})"),
    );
    Ok(sources)
}

pub(super) fn closure_sha256(
    sources: &BTreeMap<String, String>,
    consumer: bool,
) -> Result<String, OrchestratorError> {
    let (loader, entrypoint) = program(consumer);
    Ok(velnor_actions_contract::compiled_source_sha256(
        velnor_actions_contract::canonical_json_str(&serde_json::json!({
            "modules": sources, "loader": loader, "entrypoint": entrypoint,
            "shell_prelude": "set -euo pipefail", "launcher": "/usr/bin/python3 -I -S",
            "here_document": "VELNOR_CACHE_RECEIPT_PY", "initializer": INITIALIZER,
        }))?
        .as_bytes(),
    ))
}

pub(super) fn consumer_closure_sha256(
    sources: &BTreeMap<String, String>,
    configuration: &Value,
) -> Result<String, OrchestratorError> {
    Ok(velnor_actions_contract::compiled_source_sha256(
        velnor_actions_contract::canonical_json_str(&serde_json::json!({
            "consumer_program_sha256": closure_sha256(sources, true)?,
            "consumer_descriptor": configuration,
        }))?
        .as_bytes(),
    ))
}

fn program(consumer: bool) -> (&'static str, &'static str) {
    if consumer {
        (consumer_runtime::LOADER, consumer_runtime::ENTRYPOINT)
    } else {
        (runtime::LOADER, runtime::ENTRYPOINT)
    }
}

pub(super) fn initializer(
    configuration: &Value,
    sources: &BTreeMap<String, String>,
) -> Result<String, OrchestratorError> {
    let encoded = velnor_actions_contract::canonical_json_str(&(configuration, sources))?;
    let hex = encoded
        .as_bytes()
        .iter()
        .flat_map(|byte| {
            const DIGITS: &[u8; 16] = b"0123456789abcdef";
            [
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 15)],
            ]
        })
        .map(char::from)
        .collect::<String>();
    Ok(INITIALIZER.replace("__VELNOR_COMPILED_JSON_HEX__", &hex))
}

pub(super) fn record(
    recipe: &CacheProducerRecipe,
    operation: SourceBoundOperation,
    command: &str,
    configuration: &Value,
    sources: &BTreeMap<String, String>,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    let mut embedded = configuration.clone();
    embedded
        .as_object_mut()
        .ok_or_else(|| invalid("configuration_shape"))?
        .remove("cache_key_expression");
    let initializer = initializer(&embedded, sources)?;
    let (loader, entrypoint) = program(operation == SourceBoundOperation::CacheReceiptVerify);
    let body = format!(
        "set -euo pipefail\n/usr/bin/python3 -I -S <<'VELNOR_CACHE_RECEIPT_PY'\n{initializer}_COMMAND = {command:?}\n{}\n{}\nVELNOR_CACHE_RECEIPT_PY\n",
        loader, entrypoint
    );
    let source =
        velnor_actions_workflow_renderer::marker::with_marker(recipe.generator_version(), &body)?;
    let descriptor = SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
    )?;
    let invocation = HelperInvocation::compiled(descriptor, Vec::new(), Vec::new())?;
    Ok(CompiledSourceHelper::compiled(invocation, source)?
        .with_environment(environment(operation, configuration)?))
}

fn environment(
    operation: SourceBoundOperation,
    config: &Value,
) -> Result<BTreeMap<String, String>, OrchestratorError> {
    let recipe = config["recipe_sha256"]
        .as_str()
        .ok_or_else(|| invalid("missing_recipe"))?;
    let mut env = BTreeMap::from([
        (
            "VELNOR_CACHE_PAYLOAD_ROOT".into(),
            "${{ runner.temp }}/velnor".into(),
        ),
        (
            "VELNOR_CACHE_RECEIPT_ROOT".into(),
            velnor_actions_contract::cache_receipt_root(recipe),
        ),
    ]);
    if operation == SourceBoundOperation::CacheReceiptManifest {
        env.extend([
            (
                "VELNOR_CACHE_RECEIPT_KEY".into(),
                config["cache_key_expression"]
                    .as_str()
                    .ok_or_else(|| invalid("missing_key"))?
                    .into(),
            ),
            (
                "VELNOR_CACHE_SOURCE_REPOSITORY".into(),
                "${{ github.repository }}".into(),
            ),
            (
                "VELNOR_CACHE_SOURCE_REPOSITORY_ID".into(),
                "${{ github.repository_id }}".into(),
            ),
            ("VELNOR_CACHE_SOURCE_SHA".into(), "${{ github.sha }}".into()),
            ("VELNOR_CACHE_RUN_ID".into(), "${{ github.run_id }}".into()),
            (
                "VELNOR_CACHE_RUN_ATTEMPT".into(),
                "${{ github.run_attempt }}".into(),
            ),
        ]);
    } else if operation == SourceBoundOperation::CacheReceiptBundle {
        env.insert(
            "VELNOR_CACHE_RECEIPT_BUNDLE_PATH".into(),
            "${{ steps.velnor-cache-receipt-attest.outputs.bundle-path }}".into(),
        );
    }
    Ok(env)
}
