use std::collections::BTreeMap;

use crate::OrchestratorError;

use super::shellcheck_fail;

pub(super) fn resolve_run_scalar(
    value: &str,
    anchors: &mut BTreeMap<String, String>,
) -> Result<String, OrchestratorError> {
    let value = value.trim();
    if let Some(definition) = value.strip_prefix('&') {
        let Some((name, scalar)) = definition.split_once(' ') else {
            return Err(shellcheck_fail("run_scalar_anchor_malformed"));
        };
        let scalar = scalar.trim();
        if !valid_name(name)
            || scalar.is_empty()
            || scalar.starts_with(['|', '>', '[', '{', '&', '*', '!'])
        {
            return Err(shellcheck_fail("run_scalar_anchor_malformed"));
        }
        if anchors.contains_key(name) {
            return Err(shellcheck_fail("run_scalar_anchor_duplicate"));
        }
        anchors.insert(name.to_owned(), scalar.to_owned());
        return Ok(scalar.to_owned());
    }
    if let Some(name) = value.strip_prefix('*') {
        if !valid_name(name) {
            return Err(shellcheck_fail("run_scalar_alias_malformed"));
        }
        return anchors
            .get(name)
            .cloned()
            .ok_or_else(|| shellcheck_fail("run_scalar_alias_unresolved"));
    }
    Ok(value.to_owned())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}
