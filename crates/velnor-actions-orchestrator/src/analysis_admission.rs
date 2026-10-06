//! Fresh admission for repositories whose registered detector has no Rust work.

use std::path::Path;

use velnor_actions_contract::{FileIndex, VelnorConfig};

use super::super::{PlanRequest, internal_contract, plan_prepared};
use super::{check_freshness, needs_cargo, require_plan_rust_covered};
use crate::OrchestratorError;
use crate::inventory::InventoryProvider;
use crate::prepare::prepare_with_inventory;

/// An empty registered Rust detector proves fresh preparation needs no Cargo.
pub(super) fn plan_without_rust(
    request: &PlanRequest,
    root: &Path,
    config: &VelnorConfig,
    index: &FileIndex,
) -> Result<Option<String>, OrchestratorError> {
    if !velnor_actions_rust::discover_stack_candidates(index).is_empty() {
        return Ok(None);
    }
    let prep = prepare_with_inventory(root, InventoryProvider::FreshWithoutCargo)?;
    // Configuration must still be the same input used for detector admission.
    if velnor_actions_contract::canonical_json_bytes(&prep.config).map_err(internal_contract)?
        != velnor_actions_contract::canonical_json_bytes(config).map_err(internal_contract)?
    {
        return Err(needs_cargo("analysis_configuration_changed"));
    }
    let response = plan_prepared(request.clone(), &prep)?;
    require_plan_rust_covered(&response)?;
    check_freshness(&prep)?;
    Ok(Some(response))
}
