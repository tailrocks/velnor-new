// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::release_gates::{GIT_TOKEN_ENV, GIT_TOKEN_REF};

pub(crate) fn forge_env() -> BTreeMap<String, String> {
    BTreeMap::from([(GIT_TOKEN_ENV.to_owned(), GIT_TOKEN_REF.to_owned())])
}

pub(crate) fn publish_policy_checkout()
-> Result<velnor_actions_contract::Step, crate::OrchestratorError> {
    Ok(velnor_actions_workflow_renderer::action_step(
        "Checkout",
        crate::workflow::CHECKOUT_USES,
        BTreeMap::from([
            ("persist-credentials".to_owned(), "true".to_owned()),
            ("fetch-depth".to_owned(), "0".to_owned()),
        ]),
    )?)
}
