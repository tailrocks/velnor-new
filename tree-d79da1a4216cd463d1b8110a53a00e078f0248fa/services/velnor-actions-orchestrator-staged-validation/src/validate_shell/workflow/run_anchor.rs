//! Resolve the scalar-anchor subset emitted for repeated workflow `run:` values.

use std::collections::BTreeMap;

use velnor_actions_orchestrator_core::OrchestratorError;

use super::super::shellcheck_fail;

/// Resolve a `run` value emitted by the workflow renderer.
///
/// The renderer emits only scalar anchors and aliases, with aliases referring
/// to earlier `run` values. Malformed, duplicate, and forward aliases fail
/// closed rather than reaching shellcheck as YAML text.
pub(super) fn resolve_run_scalar(
    value: &str,
    anchors: &mut BTreeMap<String, String>,
) -> Result<String, OrchestratorError> {
    let value = value.trim();
    if let Some(anchor_value) = value.strip_prefix('&') {
        let Some((name, scalar)) = anchor_value.split_once(' ') else {
            return Err(shellcheck_fail("run_scalar_anchor_malformed"));
        };
        let scalar = scalar.trim();
        if !valid_anchor_name(name) || scalar.is_empty() || scalar.starts_with(['|', '>']) {
            return Err(shellcheck_fail("run_scalar_anchor_malformed"));
        }
        if anchors.contains_key(name) {
            return Err(shellcheck_fail("run_scalar_anchor_duplicate"));
        }
        anchors.insert(name.to_owned(), scalar.to_owned());
        return Ok(scalar.to_owned());
    }
    if let Some(name) = value.strip_prefix('*') {
        if !valid_anchor_name(name) {
            return Err(shellcheck_fail("run_scalar_alias_malformed"));
        }
        return anchors
            .get(name)
            .cloned()
            .ok_or_else(|| shellcheck_fail("run_scalar_alias_unresolved"));
    }
    Ok(value.to_owned())
}

fn valid_anchor_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
