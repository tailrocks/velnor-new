// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_render::{
    ACTIONS_CACHE, ALINT, ALLOWED_ACTIONS, ATTEST_BUILD_PROVENANCE, DOCKER_BUILD_PUSH,
    DOCKER_LOGIN, DOCKER_SETUP_BUILDX, GITHUB_SCRIPT, MR_BOXINGTON, PINNED_ACTIONS, PinnedAction,
    RenderedJob, RenderedStep, RenderedWorkflow, Rule, Value, check_rules, render_workflow,
};

use crate::release_fixture::app;
use crate::rules::plan;

type TestResult = Result<(), Box<dyn Error>>;

const OTHER_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn workflow(step: RenderedStep) -> RenderedWorkflow {
    RenderedWorkflow {
        jobs: vec![RenderedJob {
            id: "publish".to_owned(),
            runs_on: "ubuntu-latest".to_owned(),
            timeout_minutes: 10,
            steps: vec![step],
            ..RenderedJob::default()
        }],
    }
}

fn step(uses: &str, with: Vec<(&str, Value)>) -> RenderedStep {
    RenderedStep {
        name: "step".to_owned(),
        uses: Some(uses.to_owned()),
        with: with
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        ..RenderedStep::default()
    }
}

fn rules_of(step: RenderedStep) -> Vec<Rule> {
    check_rules(&workflow(step))
        .err()
        .into_iter()
        .flatten()
        .map(|violation| violation.rule)
        .collect()
}

fn at(pin: &PinnedAction, reference: &str) -> String {
    format!("{}@{reference}", pin.action)
}

const NEW_ACTIONS: [PinnedAction; 8] = [
    ATTEST_BUILD_PROVENANCE,
    ALINT,
    DOCKER_LOGIN,
    DOCKER_SETUP_BUILDX,
    DOCKER_BUILD_PUSH,
    GITHUB_SCRIPT,
    MR_BOXINGTON,
    ACTIONS_CACHE,
];

#[test]
fn every_new_action_is_allowed_and_pinned_by_a_full_commit_sha() {
    assert_eq!(PINNED_ACTIONS, NEW_ACTIONS);
    for pin in NEW_ACTIONS {
        assert!(ALLOWED_ACTIONS.contains(&pin.action), "{}", pin.action);
        assert_eq!(pin.commit.len(), 40, "{}", pin.action);
        assert!(
            pin.commit
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "{}",
            pin.action
        );
        assert!(pin.tag.starts_with('v'), "{}", pin.action);
    }
}

#[test]
fn a_new_action_is_accepted_only_at_its_pinned_commit() {
    for pin in NEW_ACTIONS {
        assert_eq!(
            rules_of(step(&at(&pin, pin.commit), Vec::new())),
            Vec::<Rule>::new(),
            "{}",
            pin.action
        );
        assert_eq!(
            rules_of(step(&at(&pin, OTHER_COMMIT), Vec::new())),
            [Rule::ActionPinMismatch {
                action: pin.action.to_owned(),
                commit: pin.commit,
            }],
            "{}",
            pin.action
        );
        assert_eq!(
            rules_of(step(&at(&pin, pin.tag), Vec::new())),
            [Rule::ActionNotPinned(at(&pin, pin.tag))],
            "{}",
            pin.action
        );
        assert_eq!(
            rules_of(step(pin.action, Vec::new())),
            [Rule::ActionNotPinned(pin.action.to_owned())],
            "{}",
            pin.action
        );
    }
}

#[test]
fn the_pins_match_the_commits_already_used_in_the_repository() {
    let expected = [
        (
            GITHUB_SCRIPT,
            "3a2844b7e9c422d3c10d287c895573f7108da1b3",
            "v9.0.0",
        ),
        (
            MR_BOXINGTON,
            "88acf2bb50552c7c508cbac13e5f8c576b95b1de",
            "v1.7.3",
        ),
        (
            ACTIONS_CACHE,
            "55cc8345863c7cc4c66a329aec7e433d2d1c52a9",
            "v6.1.0",
        ),
    ];
    for (pin, commit, tag) in expected {
        assert_eq!((pin.commit, pin.tag), (commit, tag), "{}", pin.action);
    }
}

#[test]
fn an_action_outside_the_list_is_still_refused() {
    let found = rules_of(step(
        &format!("docker/metadata-action@{}", DOCKER_LOGIN.commit),
        Vec::new(),
    ));
    assert_eq!(
        found,
        [Rule::ActionNotAllowed("docker/metadata-action".to_owned())]
    );
}

#[test]
fn docker_login_refuses_a_literal_password() {
    let login = at(&DOCKER_LOGIN, DOCKER_LOGIN.commit);
    let literal = Rule::LiteralCredential {
        action: DOCKER_LOGIN.action.to_owned(),
        input: "password".to_owned(),
    };
    let text = |password: &str| {
        step(
            &login,
            vec![
                ("registry", Value::Text("ghcr.io".to_owned())),
                ("password", Value::Text(password.to_owned())),
            ],
        )
    };
    assert_eq!(rules_of(text("hunter2")), std::slice::from_ref(&literal));
    assert_eq!(
        rules_of(text("${{ secrets.TOKEN }}")),
        std::slice::from_ref(&literal)
    );
    let expr = |password: &str| step(&login, vec![("password", Value::Expr(password.to_owned()))]);
    assert_eq!(
        rules_of(expr("${{ secrets.GITHUB_TOKEN }}")),
        Vec::<Rule>::new()
    );
    assert_eq!(
        rules_of(expr("${{ env.REGISTRY_TOKEN }}")),
        Vec::<Rule>::new()
    );
    assert_eq!(
        rules_of(expr("${{ github.event.head_commit.message }}")),
        [literal]
    );
}

#[test]
fn the_other_credential_inputs_refuse_a_literal_too() {
    for pin in [DOCKER_BUILD_PUSH, ATTEST_BUILD_PROVENANCE] {
        let uses = at(&pin, pin.commit);
        assert_eq!(
            rules_of(step(
                &uses,
                vec![("github-token", Value::Text("ghp_literal".to_owned()))]
            )),
            [Rule::LiteralCredential {
                action: pin.action.to_owned(),
                input: "github-token".to_owned(),
            }]
        );
        assert_eq!(
            rules_of(step(
                &uses,
                vec![(
                    "github-token",
                    Value::Expr("${{ secrets.TOKEN }}".to_owned())
                )]
            )),
            Vec::<Rule>::new()
        );
    }
}

fn password_literal(action: &str, input: &str) -> Rule {
    Rule::LiteralCredential {
        action: action.to_owned(),
        input: input.to_owned(),
    }
}

#[test]
fn a_credential_expression_must_be_a_single_secret_reference() {
    let login = at(&DOCKER_LOGIN, DOCKER_LOGIN.commit);
    let refused = [
        "${{ 'literal-password' || secrets.TOKEN }}",
        "${{ github.event.head_commit.message || secrets.TOKEN }}",
        "${{ format('secrets.X') }}",
        "${{ secrets.TOKEN }}literal",
        "literal${{ secrets.TOKEN }}",
        "${{ secrets.TOKEN }}${{ github.event.issue.title }}",
        "${{ secrets['TOKEN'] }}",
        "${{ secrets.TOKEN == 'x' }}",
        "secrets.TOKEN",
    ];
    for text in refused {
        assert_eq!(
            rules_of(step(
                &login,
                vec![("password", Value::Expr(text.to_owned()))]
            )),
            [password_literal(DOCKER_LOGIN.action, "password")],
            "{text}"
        );
    }
    for text in [
        "${{ secrets.TOKEN }}",
        "${{secrets.TOKEN}}",
        "${{ env.REGISTRY_TOKEN }}",
        "${{ github.token }}",
    ] {
        assert_eq!(
            rules_of(step(
                &login,
                vec![("password", Value::Expr(text.to_owned()))]
            )),
            Vec::<Rule>::new(),
            "{text}"
        );
    }
}

#[test]
fn credential_input_names_match_case_insensitively() {
    let login = at(&DOCKER_LOGIN, DOCKER_LOGIN.commit);
    for name in ["PASSWORD", "Password"] {
        assert_eq!(
            rules_of(step(
                &login,
                vec![(name, Value::Text("hunter2".to_owned()))]
            )),
            [password_literal(DOCKER_LOGIN.action, name)],
            "{name}"
        );
    }
    let build = at(&DOCKER_BUILD_PUSH, DOCKER_BUILD_PUSH.commit);
    assert_eq!(
        rules_of(step(
            &build,
            vec![("Github-Token", Value::Text("ghp_literal".to_owned()))]
        )),
        [password_literal(DOCKER_BUILD_PUSH.action, "Github-Token")]
    );
}

#[test]
fn the_build_secrets_input_accepts_only_secret_references() {
    let build = at(&DOCKER_BUILD_PUSH, DOCKER_BUILD_PUSH.commit);
    let literal = password_literal(DOCKER_BUILD_PUSH.action, "secrets");
    for value in [
        Value::Text("GIT_AUTH_TOKEN=ghp_literal".to_owned()),
        Value::Text("GIT_AUTH_TOKEN=${{ secrets.TOKEN }}".to_owned()),
        Value::Expr("GIT_AUTH_TOKEN=ghp_literal".to_owned()),
        Value::Expr("A=${{ secrets.A }}\nB=literal".to_owned()),
        Value::Expr("A=${{ 'literal' || secrets.A }}".to_owned()),
        Value::Expr("${{ secrets.A }}".to_owned()),
    ] {
        assert_eq!(
            rules_of(step(&build, vec![("secrets", value.clone())])),
            std::slice::from_ref(&literal),
            "{value:?}"
        );
    }
    for value in [
        "GIT_AUTH_TOKEN=${{ secrets.TOKEN }}",
        "A=${{ secrets.A }}\nB=${{ env.B }}",
    ] {
        assert_eq!(
            rules_of(step(
                &build,
                vec![("secrets", Value::Expr(value.to_owned()))]
            )),
            Vec::<Rule>::new(),
            "{value}"
        );
    }
}

#[test]
fn a_rendered_new_action_carries_its_tag_in_a_trailing_comment() -> TestResult {
    let mut rendered = step(
        &at(&DOCKER_SETUP_BUILDX, DOCKER_SETUP_BUILDX.commit),
        Vec::new(),
    );
    rendered.uses_comment = Some(DOCKER_SETUP_BUILDX.tag.to_owned());
    let files = render_workflow(&plan(Vec::new())?, &workflow(rendered))
        .map_err(|found| format!("{found:?}"))?;
    let text = files.values().next().ok_or("no workflow")?;
    assert!(
        text.contains(&format!(
            "uses: docker/setup-buildx-action@{}  # {}",
            DOCKER_SETUP_BUILDX.commit, DOCKER_SETUP_BUILDX.tag
        )),
        "{text}"
    );
    Ok(())
}

fn yaml_files(dir: &Path, found: &mut Vec<std::path::PathBuf>) -> TestResult {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            yaml_files(&path, found)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "yml" || extension == "yaml")
        {
            found.push(path);
        }
    }
    Ok(())
}

fn without_github_mode(text: &str) -> String {
    text.replace("github-cache-mode", "")
}

#[test]
fn no_rendered_workflow_contains_the_non_github_cache_mode_key() -> TestResult {
    let (release, _) = app()?;
    assert!(!without_github_mode(&release).contains("cache-mode"));
    let rust_ci = render_workflow(
        &plan(Vec::new())?,
        &workflow(step(&at(&ALINT, ALINT.commit), Vec::new())),
    )
    .map_err(|found| format!("{found:?}"))?;
    for text in rust_ci.values() {
        assert!(!without_github_mode(text).contains("cache-mode"));
    }
    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples");
    let mut files = Vec::new();
    yaml_files(&samples, &mut files)?;
    assert_ne!(files.len(), 0);
    for file in files {
        assert!(
            !without_github_mode(&fs::read_to_string(&file)?).contains("cache-mode"),
            "{}",
            file.display()
        );
    }
    Ok(())
}
