//! Release checkout builders: policy tree plus exact-source tree.
//!
//! Every release job checks the policy tree out at the workspace root
//! (event commit: generated release-plz configs live there) while jobs
//! that run release-plz against the approved source add a second
//! checkout of the approved SHA into [`RELEASE_SOURCE_DIR`]. Policy
//! checkouts never pin `ref` (release-pr requires a branch); source
//! checkouts always do. Both fetch full history: release-plz needs tags
//! plus history for selection and changelogs. Only checkouts that must
//! push (preparation's policy checkout, publishers' source checkouts)
//! persist credentials.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::Step;
use velnor_actions_workflow_release::release_gates::{GIT_TOKEN_ENV, GIT_TOKEN_REF};
use velnor_actions_workflow_release::release_tree::RELEASE_SOURCE_DIR;
use velnor_actions_workflow_steps::action_step;

use crate::OrchestratorError;
use crate::workflow::CHECKOUT_USES;

/// Display name of the exact-source checkout step.
const SOURCE_CHECKOUT_NAME: &str = "Checkout exact source";

/// Policy checkout at the workspace root (event commit, never a `ref` pin).
///
/// # Errors
///
/// Returns render errors for invalid action refs (ruled out: pinned).
pub(crate) fn policy_checkout(persist: bool) -> Result<Step, OrchestratorError> {
    Ok(action_step(
        "Checkout",
        CHECKOUT_USES,
        BTreeMap::from([
            ("persist-credentials".to_owned(), persist_word(persist)),
            ("fetch-depth".to_owned(), "0".to_owned()),
        ]),
    )?)
}

/// Exact-source checkout into [`RELEASE_SOURCE_DIR`] (approved SHA pin).
///
/// # Errors
///
/// Returns render errors for invalid action refs (ruled out: pinned).
pub(crate) fn source_checkout(sha: &str, persist: bool) -> Result<Step, OrchestratorError> {
    Ok(action_step(
        SOURCE_CHECKOUT_NAME,
        CHECKOUT_USES,
        BTreeMap::from([
            ("persist-credentials".to_owned(), persist_word(persist)),
            ("fetch-depth".to_owned(), "0".to_owned()),
            ("path".to_owned(), RELEASE_SOURCE_DIR.to_owned()),
            ("ref".to_owned(), sha.to_owned()),
        ]),
    )?)
}

/// `persist-credentials` spelling for one checkout.
fn persist_word(persist: bool) -> String {
    if persist { "true" } else { "false" }.to_owned()
}

/// Forge-token env binding every release-plz step carries.
pub(crate) fn forge_env() -> BTreeMap<String, String> {
    BTreeMap::from([(GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned())])
}

/// `--manifest-path` argv value inside the exact-source checkout.
pub(crate) fn source_manifest(manifest: &str) -> String {
    format!("{RELEASE_SOURCE_DIR}/{manifest}")
}
