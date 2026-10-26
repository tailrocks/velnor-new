// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{RenderError, ReviewCompletionSpec, Yaml, map};
use std::collections::BTreeMap;

pub(super) fn render(spec: &ReviewCompletionSpec) -> Result<Vec<Yaml>, RenderError> {
    let policy = String::from_utf8(
        spec.policy
            .canonical_bytes()
            .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?,
    )
    .map_err(|e| RenderError::InvalidWorkflow(e.to_string()))?;
    let setup = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Install pinned Mise")),
        ("uses".to_owned(), Yaml::str(&spec.setup.uses)),
        (
            "with".to_owned(),
            map(&[
                ("version", &spec.setup.version),
                ("sha256", &spec.setup.sha256),
                ("install", "false"),
            ]),
        ),
    ]);
    let install = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Install pinned GitHub CLI")),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "run".to_owned(),
            Yaml::str(crate::commands::join_argv_for_run(&spec.install_gh_argv)?),
        ),
    ]);
    let acquire = crate::document_steps::non_internal_step_to_yaml(
        "review-completion",
        &spec.acquire,
        &BTreeMap::new(),
        "ubuntu-26.04",
    )?;
    Ok(vec![
        setup,
        install,
        acquire,
        operation(spec, &policy, false),
        mint(spec)?,
        operation(spec, &policy, true),
    ])
}

fn operation(spec: &ReviewCompletionSpec, policy: &str, emit: bool) -> Yaml {
    let mut env = vec![
        (
            "VELNOR_INTERNAL_OP".to_owned(),
            Yaml::str(if emit {
                "emit-review-completion-v1"
            } else {
                "capture-review-completion-v1"
            }),
        ),
        (
            "VELNOR_REQUEST_FILE".to_owned(),
            Yaml::str("${{ runner.temp }}/velnor/review-completion-policy.json"),
        ),
        (
            "VELNOR_COMPLETION_APP_ID".to_owned(),
            variable(&spec.config.app_id_variable),
        ),
        (
            "VELNOR_REVIEWER_APP_ID".to_owned(),
            variable(&spec.config.reviewer_app_id_variable),
        ),
        (
            "GH_TOKEN".to_owned(),
            Yaml::str(if emit {
                "${{ steps.completion-token.outputs.token }}"
            } else {
                "${{ github.token }}"
            }),
        ),
    ];
    if emit {
        env.push((
            "VELNOR_REVIEW_COMPLETION_PROPOSAL_DIGEST".to_owned(),
            Yaml::str("${{ steps.capture.outputs.proposal_digest }}"),
        ));
    } else {
        env.push((
            "VELNOR_REVIEW_COMPLETION_POLICY".to_owned(),
            Yaml::str(policy),
        ));
    }
    let materialize = if emit {
        ""
    } else {
        "mkdir -p \"$RUNNER_TEMP/velnor\"\n(set -C; printf '%s' \"$VELNOR_REVIEW_COMPLETION_POLICY\" > \"$VELNOR_REQUEST_FILE\")\n"
    };
    let run = format!(
        "set -euo pipefail\numask 077\n{materialize}\"$RUNNER_TEMP/velnor/bin/velnor-actions-{}\"\n",
        spec.generator_version
    );
    let mut fields = vec![
        (
            "name".to_owned(),
            Yaml::str(if emit {
                "Emit freshly authenticated review state"
            } else {
                "Capture authenticated provider and feedback"
            }),
        ),
        (
            "id".to_owned(),
            Yaml::str(if emit { "emit" } else { "capture" }),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
        ("env".to_owned(), Yaml::Map(env)),
    ];
    if emit {
        fields.push((
            "if".to_owned(),
            Yaml::str("${{ steps.capture.outputs.eligible == 'true' }}"),
        ));
    }
    Yaml::Map(fields)
}

fn mint(spec: &ReviewCompletionSpec) -> Result<Yaml, RenderError> {
    let (owner, repo) = spec
        .policy
        .repository
        .split_once('/')
        .ok_or_else(super::invalid)?;
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Mint independent completion check capability"),
        ),
        ("id".to_owned(), Yaml::str("completion-token")),
        (
            "if".to_owned(),
            Yaml::str("${{ steps.capture.outputs.eligible == 'true' }}"),
        ),
        (
            "uses".to_owned(),
            Yaml::str(crate::steps::CREATE_GITHUB_APP_TOKEN_USES),
        ),
        (
            "with".to_owned(),
            Yaml::Map(vec![
                (
                    "client-id".to_owned(),
                    variable(&spec.config.app_id_variable),
                ),
                (
                    "private-key".to_owned(),
                    Yaml::str(format!(
                        "${{{{ secrets.{} }}}}",
                        spec.config.app_private_key_secret
                    )),
                ),
                ("owner".to_owned(), Yaml::str(owner)),
                ("repositories".to_owned(), Yaml::str(repo)),
                ("permission-checks".to_owned(), Yaml::str("write")),
                ("permission-pull-requests".to_owned(), Yaml::str("read")),
                ("permission-contents".to_owned(), Yaml::str("read")),
                ("skip-token-revoke".to_owned(), Yaml::Bool(false)),
            ]),
        ),
    ]))
}
fn variable(name: &str) -> Yaml {
    Yaml::str(format!("${{{{ vars.{name} }}}}"))
}
