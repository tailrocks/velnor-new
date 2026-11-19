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
pub const GITHUB_SCRIPT: PinnedAction = PinnedAction {
    action: "actions/github-script",
    commit: "3a2844b7e9c422d3c10d287c895573f7108da1b3",
    tag: "v9.0.0",
};
pub const MR_BOXINGTON: PinnedAction = PinnedAction {
    action: "jdx/mr-boxington-action",
    commit: "88acf2bb50552c7c508cbac13e5f8c576b95b1de",
    tag: "v1.7.3",
};
/// The combined cache action, at the same commit as `actions/cache/restore`
/// and `actions/cache/save`.
pub const ACTIONS_CACHE: PinnedAction = PinnedAction {
    action: "actions/cache",
    commit: "55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
    tag: "v6.1.0",
};

pub const PINNED_ACTIONS: [PinnedAction; 7] = [
    ATTEST_BUILD_PROVENANCE,
    DOCKER_LOGIN,
    DOCKER_SETUP_BUILDX,
    DOCKER_BUILD_PUSH,
    GITHUB_SCRIPT,
    MR_BOXINGTON,
    ACTIONS_CACHE,
];

#[derive(Clone, Copy)]
enum Shape {
    Reference,
    KeyedReferences,
}

const CREDENTIAL_INPUTS: [(&str, &str, Shape); 5] = [
    (DOCKER_LOGIN.action, "password", Shape::Reference),
    (DOCKER_BUILD_PUSH.action, "github-token", Shape::Reference),
    (DOCKER_BUILD_PUSH.action, "secrets", Shape::KeyedReferences),
    (
        ATTEST_BUILD_PROVENANCE.action,
        "github-token",
        Shape::Reference,
    ),
    (GITHUB_SCRIPT.action, "github-token", Shape::Reference),
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
            .find(|(guarded, input, _)| *guarded == action && input.eq_ignore_ascii_case(name));
        if let Some((_, _, shape)) = credential
            && !is_secret_reference(value, *shape)
        {
            rules.push(Rule::LiteralCredential {
                action: action.to_owned(),
                input: name.clone(),
            });
        }
    }
}

fn is_secret_reference(value: &Value, shape: Shape) -> bool {
    let (Value::Expr(text) | Value::BoundedExpr { text, .. }) = value else {
        return false;
    };
    match shape {
        Shape::Reference => single_reference(text),
        Shape::KeyedReferences => {
            text.lines().any(|line| !line.is_empty())
                && text.lines().filter(|line| !line.is_empty()).all(|line| {
                    line.split_once('=').is_some_and(|(id, reference)| {
                        name_chars(id) && single_reference(reference)
                    })
                })
        }
    }
}

fn single_reference(text: &str) -> bool {
    let Some(inner) = text
        .strip_prefix("${{")
        .and_then(|rest| rest.strip_suffix("}}"))
    else {
        return false;
    };
    let path = inner.trim();
    path == "github.token"
        || ["secrets.", "env."]
            .iter()
            .any(|root| path.strip_prefix(root).is_some_and(name_chars))
}

fn name_chars(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.'
        })
}
