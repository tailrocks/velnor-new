use super::types::{QualificationRuntimeIdentity, QualificationRuntimeIdentityRequirements};
use crate::errors::ContractError;

mod host;
mod rust;
mod shared;
mod tools;

pub(super) fn validate_abi_observations(
    requirements: &QualificationRuntimeIdentityRequirements,
    evidence: &QualificationRuntimeIdentity,
) -> Result<(), ContractError> {
    let families = shared::abi_families_in_order(evidence)?;
    host::validate_host_fact(evidence)?;
    rust::validate_rust_observation(requirements, evidence)?;
    tools::validate_tool_abi_facts(requirements, evidence)?;
    shared::require_abi_for_pins(requirements, &families)?;
    Ok(())
}
