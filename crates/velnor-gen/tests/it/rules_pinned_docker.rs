// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_render::{RenderedJob, RenderedStep, RenderedWorkflow, Rule, check_rules};

fn uses(text: &str) -> RenderedWorkflow {
    RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "publish".to_owned(),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            steps: vec![RenderedStep {
                name: "step".to_owned(),
                uses: Some(text.to_owned()),
                ..RenderedStep::default()
            }],
            ..RenderedJob::default()
        }],
    }
}

fn rules_of(text: &str) -> Vec<Rule> {
    check_rules(&uses(text))
        .err()
        .into_iter()
        .flatten()
        .map(|violation| violation.rule)
        .collect()
}

#[test]
fn a_rendered_docker_or_app_token_step_is_accepted_only_at_its_pinned_commit() {
    for (pinned, other) in [
        (
            "docker/login-action@8aa5831fb8d48d8f76594c7ddde61ae58019f550",
            "docker/login-action@f87e5991a6d7451dcb8d9637bfbc97413f497069",
        ),
        (
            "docker/build-push-action@c3c9e263c25d99ce0380d002d59b67737d91b0dc",
            "docker/build-push-action@8aa5831fb8d48d8f76594c7ddde61ae58019f550",
        ),
        (
            "actions/create-github-app-token@64ab729a6294000f5aa4a4bc0b73d1f32bc45510",
            "actions/create-github-app-token@f418cff399f7aef4c2f0ad47e25ca7a6abbdd6fc",
        ),
    ] {
        assert_eq!(rules_of(pinned), Vec::<Rule>::new(), "{pinned}");
        assert!(
            matches!(rules_of(other).as_slice(), [Rule::ActionPinMismatch { .. }]),
            "{other} must be refused: {:?}",
            rules_of(other)
        );
        let action = pinned.split_once('@').map_or(pinned, |(action, _)| action);
        assert!(
            matches!(
                rules_of(&format!("{action}@v1")).as_slice(),
                [Rule::ActionNotPinned(_)]
            ),
            "{action}@v1 must be refused as a tag"
        );
    }
}
