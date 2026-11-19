// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::{RenderedStep, Rule, Value};

/// An action that velnor-gen accepts at exactly one commit.
///
/// `tag` is the release the commit was resolved from; it appears in a trailing
/// comment of the rendered `uses` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedAction {
    pub action: &'static str,
    pub commit: &'static str,
    pub tag: &'static str,
}

pub const ATTEST_BUILD_PROVENANCE: PinnedAction = PinnedAction {
    action: "actions/attest-build-provenance",
    commit: "4d101475d8b20a2381f78447822ac1eab6504dd8",
    tag: "v4.2.2",
};
pub const ALINT: PinnedAction = PinnedAction {
    action: "asamarts/alint",
    commit: "5054852d00d47affc9ea71440240147d9999f4d3",
    tag: "v0.22.1",
};
pub const DOCKER_LOGIN: PinnedAction = PinnedAction {
    action: "docker/login-action",
    commit: "8aa5831fb8d48d8f76594c7ddde61ae58019f550",
    tag: "v4.8.0",
};
pub const DOCKER_SETUP_BUILDX: PinnedAction = PinnedAction {
    action: "docker/setup-buildx-action",
    commit: "f87e5991a6d7451dcb8d9637bfbc97413f497069",
    tag: "v4.4.1",
};
pub const DOCKER_BUILD_PUSH: PinnedAction = PinnedAction {
    action: "docker/build-push-action",
    commit: "c3c9e263c25d99ce0380d002d59b67737d91b0dc",
    tag: "v7.4.0",
};

pub const PINNED_ACTIONS: [PinnedAction; 5] = [
    ATTEST_BUILD_PROVENANCE,
    ALINT,
    DOCKER_LOGIN,
    DOCKER_SETUP_BUILDX,
    DOCKER_BUILD_PUSH,
];

/// Inputs that carry a credential. A literal value is refused: the value must
/// be an expression over `secrets`, `env` or the job token.
const CREDENTIAL_INPUTS: [(&str, &str); 3] = [
    (DOCKER_LOGIN.action, "password"),
    (DOCKER_BUILD_PUSH.action, "github-token"),
    (ATTEST_BUILD_PROVENANCE.action, "github-token"),
];

pub(crate) fn pin_rules(action: &str, reference: &str, rules: &mut Vec<Rule>) {
    if let Some(pin) = PINNED_ACTIONS.iter().find(|pin| pin.action == action)
        && reference != pin.commit
    {
        rules.push(Rule::ActionPinMismatch {
            action: action.to_owned(),
            commit: pin.commit,
        });
    }
}

pub(crate) fn credential_rules(step: &RenderedStep, rules: &mut Vec<Rule>) {
    let Some(action) = step.uses.as_deref().and_then(|uses| uses.split('@').next()) else {
        return;
    };
    for (name, value) in &step.with {
        let credential = CREDENTIAL_INPUTS
            .iter()
            .any(|(guarded, input)| *guarded == action && input == name);
        if credential && !from_secret_or_env(value) {
            rules.push(Rule::LiteralCredential {
                action: action.to_owned(),
                input: name.clone(),
            });
        }
    }
}

fn from_secret_or_env(value: &Value) -> bool {
    match value {
        Value::Expr(text) | Value::BoundedExpr { text, .. } => {
            text.contains("secrets.") || text.contains("env.") || text.contains("github.token")
        }
        Value::Text(_) | Value::Int(_) | Value::Bool(_) => false,
    }
}
