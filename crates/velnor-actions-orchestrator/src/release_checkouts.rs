//! Anonymous exact-source checkout; no release credential persistence exists.
use crate::OrchestratorError;
use std::collections::BTreeMap;
use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::{action_step, release_tree::RELEASE_SOURCE_DIR};
const SOURCE_CHECKOUT_NAME: &str = "Checkout exact source";
/// Exact-source checkout into [`RELEASE_SOURCE_DIR`] (approved SHA pin).
///
/// # Errors
///
/// Returns render errors for invalid action refs (ruled out: pinned).
pub(crate) fn source_checkout(checkout_uses: &str, sha: &str) -> Result<Step, OrchestratorError> {
    Ok(action_step(
        SOURCE_CHECKOUT_NAME,
        checkout_uses,
        BTreeMap::from([
            ("persist-credentials".to_owned(), "false".to_owned()),
            ("fetch-depth".to_owned(), "0".to_owned()),
            ("path".to_owned(), RELEASE_SOURCE_DIR.to_owned()),
            ("ref".to_owned(), sha.to_owned()),
        ]),
    )?)
}
