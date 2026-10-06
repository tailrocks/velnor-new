//! Complete immutable read-only Git source snapshot closure.

use std::collections::BTreeMap;

use velnor_actions_contract::{CompiledSupportSource, ContractError, generated_source};

use crate::OrchestratorError;
use crate::release_emit::release_steps::JobInputs;

#[path = "release_original_source_origin_context.rs"]
mod origin_context;

const PREFIX: &str = ".github/velnor/";
const MODULES: [(&str, &str); 7] = [
    (
        "release_reconcile_common.py",
        include_str!("release_reconcile_common.py"),
    ),
    (
        "release_forge_publish_read.py",
        include_str!("release_forge_publish_read.py"),
    ),
    (
        "release_source_tree.py",
        include_str!("release_source_tree.py"),
    ),
    (
        "release_source_snapshot.py",
        include_str!("release_source_snapshot.py"),
    ),
    (
        "release_source_snapshot_output.py",
        include_str!("release_source_snapshot_output.py"),
    ),
    (
        "release_original_source_origin.py",
        include_str!("release_original_source_origin.py"),
    ),
    (
        "release_source_snapshot_entry.py",
        include_str!("release_source_snapshot_entry.py"),
    ),
];

/// Newly owned support records; shared modules retain their existing owner record.
pub(super) fn support_sources(
    inputs: &JobInputs<'_>,
    version: &str,
) -> Result<Vec<CompiledSupportSource>, OrchestratorError> {
    let mut sources: Vec<_> = MODULES[2..]
        .iter()
        .map(|(name, source)| {
            CompiledSupportSource::compiled(&format!("{PREFIX}{name}"), source, version)
        })
        .collect::<Result<_, _>>()?;
    sources.push(CompiledSupportSource::compiled(
        &format!("{PREFIX}release_original_source_origin_context.py"),
        &origin_context::context_source(inputs)?,
        version,
    )?);
    Ok(sources)
}

/// Exact marked zero-argument execution source, including the fixed output writer.
pub(super) fn execution_source(
    inputs: &JobInputs<'_>,
    version: &str,
) -> Result<String, OrchestratorError> {
    Ok(generated_source(
        version,
        &execution_body(inputs, version)?,
    )?)
}

/// Complete Bash body; generation freezes every marked Python module before import.
pub(super) fn execution_body(
    inputs: &JobInputs<'_>,
    version: &str,
) -> Result<String, OrchestratorError> {
    let mut closure = BTreeMap::new();
    for (name, source) in MODULES {
        let record = CompiledSupportSource::compiled(&format!("{PREFIX}{name}"), source, version)?;
        closure.insert(name, record.source().to_owned());
    }
    let context_name = "release_original_source_origin_context.py";
    let context = CompiledSupportSource::compiled(
        &format!("{PREFIX}{context_name}"),
        &origin_context::context_source(inputs)?,
        version,
    )?;
    closure.insert(context_name, context.source().to_owned());
    let mut names: Vec<_> = MODULES.iter().map(|(name, _)| *name).collect();
    names.insert(names.len() - 1, context_name);
    let sources = serde_json::to_string(&closure)
        .map(|json| json.replace('$', "\\u0024"))
        .map_err(|_| ContractError::identity("release_source_snapshot", "module_encoding"))?;
    let names = serde_json::to_string(&names)
        .map_err(|_| ContractError::identity("release_source_snapshot", "order_encoding"))?;
    Ok(format!(
        "set -euo pipefail\npython3 -I -S - \"$@\" <<'VELNOR_RELEASE_COMPILED_BODY'\nCOMPILED_SOURCES = {sources}\n{}\nVELNOR_RELEASE_COMPILED_BODY\n",
        super::sealed_launcher(&names, "source_snapshot_main"),
    ))
}
