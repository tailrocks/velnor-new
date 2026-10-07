//! Which schema 2 workflows to emit, plus the selectors they use.

use std::collections::BTreeSet;

use velnor_actions_contract_config::{
    RoutingWorkflow, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL,
};
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_steps::setup::MiseSetup;

use crate::generator_release_pins::GeneratorReleasePins;

/// Which schema 2 workflows to emit, plus the selectors they use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema2WorkflowRequest {
    /// Generator version for the marker.
    pub version: String,
    /// Hosted catalog label.
    pub hosted_label: String,
    /// Validated scale-set selector.
    pub scale_set: ScaleSetSelector,
    /// Workflows to emit. Empty emits nothing.
    pub workflows: BTreeSet<RoutingWorkflow>,
    /// Pinned tool inputs, required only when qualification is emitted.
    pub mbx_qualification: Option<MbxQualificationPins>,
    /// Orchestrator-resolved Mise setup and command vectors for generator release.
    pub generator_release: Option<GeneratorReleasePins>,
}

/// Exact tools used by the hosted MBX cache qualification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MbxQualificationPins {
    /// Resolved Mise action and binary pins.
    pub mise_setup: MiseSetup,
    /// Full-SHA candidate MBX Action ref for this unqualified experiment.
    /// It is separate from the production pin and generation never qualifies it.
    pub candidate_action_uses: String,
    /// Exact MBX tool version.
    pub mbx_version: String,
    /// Exact Rust toolchain version used by the qualification lane.
    pub rust_version: String,
}

impl Schema2WorkflowRequest {
    /// Canonical scale-set selector (`velnor`, then the scale-set name).
    ///
    /// # Errors
    ///
    /// Illegal labels fail.
    pub fn canonical_scale_set() -> Result<ScaleSetSelector, RenderError> {
        ScaleSetSelector::try_new(
            SCALE_SET_NAME,
            &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
        )
        .map_err(|err| RenderError::InvalidWorkflow(err.to_string()))
    }
}
