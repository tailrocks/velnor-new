// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, refusal, rendered_text, workflow};

const PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]\n";
const READ: &str = "permissions = { contents = \"read\" }";

const LOGIN: &str = "docker/login-action@8aa5831fb8d48d8f76594c7ddde61ae58019f550";
const BUILD_PUSH: &str = "docker/build-push-action@c3c9e263c25d99ce0380d002d59b67737d91b0dc";
const APP_TOKEN: &str = "actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510";

/// A push workflow with one job that runs one step using `uses` and `with`.
fn steps_of(uses: &str, with: &str) -> String {
    workflow(
        "",
        PUSH,
        &format!(
            "\n[[workflow.job]]\nid = \"check\"\n{READ}\n[[workflow.job.step]]\nuses = \"{uses}\"\n{with}\n"
        ),
    )
}

#[test]
fn the_declared_docker_and_app_token_names_render_at_their_pinned_commits() -> TestResult {
    let cases = [
        (
            "docker-login",
            "with = { registry = \"ghcr.io\", password = \"${{ github.token }}\" }",
            LOGIN,
        ),
        (
            "docker-build-push",
            "with = { push = \"false\" }",
            BUILD_PUSH,
        ),
        (
            "create-github-app-token",
            "with = { private-key = \"not-a-key\" }",
            APP_TOKEN,
        ),
    ];
    for (name, with, pinned) in cases {
        let text = rendered_text(&steps_of(name, with), "ci")?;
        assert!(
            text.contains(&format!("uses: {pinned}")),
            "{name} must render at {pinned}: {text}"
        );
    }
    Ok(())
}

#[test]
fn a_declared_docker_or_app_token_step_takes_no_tag_and_no_other_commit() -> TestResult {
    for uses in [
        "docker/login-action@v4.8.0",
        "docker/login-action@8aa5831",
        "docker/login-action@0000000000000000000000000000000000000000",
        "docker/build-push-action@v7.4.0",
        "actions/create-github-app-token@v3.3.0",
        "actions/create-github-app-token@f418cff399f7aef4c2f0ad47e25ca7a6abbdd6fc",
    ] {
        let message = refusal(&steps_of(uses, ""))?;
        assert!(message.contains("pinned actions"), "{uses}: {message}");
    }
    Ok(())
}

#[test]
fn a_declared_docker_step_takes_only_its_own_inputs() -> TestResult {
    let message = refusal(&steps_of(
        "docker-build-push",
        "with = { push = \"false\", not-an-input = \"x\" }",
    ))?;
    assert!(message.contains("not-an-input"), "{message}");
    let message = refusal(&steps_of("create-github-app-token", ""))?;
    assert!(message.contains("private-key"), "{message}");
    Ok(())
}
