use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::action_step;
use velnor_actions_workflow_renderer::release_gates::{GIT_TOKEN_ENV, GIT_TOKEN_REF};
use velnor_actions_workflow_renderer::release_tree::RELEASE_SOURCE_DIR;

use crate::OrchestratorError;
use crate::workflow::CHECKOUT_USES;

const SOURCE_CHECKOUT_NAME: &str = "Checkout exact source";

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

fn persist_word(persist: bool) -> String {
    if persist { "true" } else { "false" }.to_owned()
}

pub(crate) fn forge_env() -> BTreeMap<String, String> {
    BTreeMap::from([(GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned())])
}

pub(crate) fn source_manifest(manifest: &str) -> String {
    format!("{RELEASE_SOURCE_DIR}/{manifest}")
}
