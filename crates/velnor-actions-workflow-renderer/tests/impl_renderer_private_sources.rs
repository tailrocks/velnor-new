//! HD-7: the private-source mint, fetch and revoke block and its credential
//! binding gate (`credential_binding_outside_private_fetch`).

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_workflow_renderer::private_source_steps::{
    PrivateSourcesSpec, private_source_steps,
};
use velnor_actions_workflow_renderer::{
    RenderError, checkout_step, plan_step, render_workflow_ir, shell_step,
};

use super::impl_renderer_fixtures::*;

fn repositories() -> Vec<String> {
    vec![
        "eth-processor-java".to_owned(),
        "processor-shared".to_owned(),
    ]
}

fn block() -> Result<[Step; 3], RenderError> {
    let repositories = repositories();
    private_source_steps(&PrivateSourcesSpec {
        owner: "ChainArgos",
        app_id_variable: "VELNOR_DEPS_APP_ID",
        app_private_key_secret: "VELNOR_DEPS_APP_PRIVATE_KEY",
        repositories: &repositories,
    })
}

fn plan_job_with(middle: Vec<Step>) -> Result<(String, velnor_actions_contract::Job), RenderError> {
    let mut steps = vec![checkout_step(&checkout_pin())?];
    steps.extend(middle);
    steps.push(plan_step());
    Ok(job("plan", "Plan", Vec::new(), steps))
}

fn render(jobs: Vec<(String, velnor_actions_contract::Job)>) -> Result<String, RenderError> {
    render_workflow_ir(
        &fixture_ir(jobs),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )
}

#[test]
fn the_block_renders_mint_fetch_revoke_with_literal_allowlist() -> Result<(), RenderError> {
    let text = render(vec![plan_job_with(block()?.to_vec())?])?;
    for needle in [
        "- name: Mint read-only token for private sources",
        "id: deps-token",
        "client-id: ${{ vars.VELNOR_DEPS_APP_ID }}",
        "private-key: ${{ secrets.VELNOR_DEPS_APP_PRIVATE_KEY }}",
        "owner: ChainArgos",
        "repositories: eth-processor-java,processor-shared",
        "permission-contents: read",
        "skip-token-revoke: 'false'",
        "- name: Fetch private sources",
        "VELNOR_INTERNAL_OP: private-fetch-v1",
        "VELNOR_INTERNAL_OP: private-revoke-v1",
        "VELNOR_PRIVATE_REPOSITORIES: eth-processor-java,processor-shared",
        "if: always() && steps.deps-token.outcome == 'success'",
    ] {
        assert!(
            text.contains(needle) || text.contains(&needle.replace("'false'", "\"false\"")),
            "missing {needle}:\n{text}"
        );
    }
    assert_eq!(
        text.matches("VELNOR_PRIVATE_FETCH_TOKEN: ${{ steps.deps-token.outputs.token }}")
            .count(),
        2,
        "token binds in the fetch and revoke steps only:\n{text}"
    );
    assert_eq!(text.matches("steps.deps-token.outputs.token").count(), 2);
    let mint_at = text.find("Mint read-only").unwrap_or(usize::MAX);
    let fetch_at = text.find("- name: Fetch private").unwrap_or(0);
    let revoke_at = text.find("- name: Revoke private").unwrap_or(0);
    let plan_at = text.find("VELNOR_INTERNAL_OP: plan-v1").unwrap_or(0);
    assert!(mint_at < fetch_at && fetch_at < revoke_at && revoke_at < plan_at);
    Ok(())
}

#[test]
fn constructors_reject_malformed_specs() {
    let empty: Vec<String> = Vec::new();
    let unsorted = vec!["b".to_owned(), "a".to_owned()];
    let dupes = vec!["a".to_owned(), "a".to_owned()];
    let slash = vec!["a/b".to_owned()];
    for repos in [&empty, &unsorted, &dupes, &slash] {
        let spec = PrivateSourcesSpec {
            owner: "ChainArgos",
            app_id_variable: "VELNOR_DEPS_APP_ID",
            app_private_key_secret: "VELNOR_DEPS_APP_PRIVATE_KEY",
            repositories: repos,
        };
        assert!(private_source_steps(&spec).is_err(), "{repos:?}");
    }
    let repos = repositories();
    for (owner, id, key) in [
        ("Chain Argos", "VELNOR_DEPS_APP_ID", "K"),
        ("ChainArgos", "lower", "K"),
        ("ChainArgos", "A", "x}} ${{ secrets.OTHER"),
    ] {
        let spec = PrivateSourcesSpec {
            owner,
            app_id_variable: id,
            app_private_key_secret: key,
            repositories: &repos,
        };
        assert!(private_source_steps(&spec).is_err(), "{owner} {id} {key}");
    }
}

type MintEdit = (&'static str, Box<dyn Fn(&mut BTreeMap<String, String>)>);

fn token_step(name: &str, role: Option<StepRole>, key: &str, value: &str) -> Step {
    Step {
        parallel: false,
        name: name.to_owned(),
        id: None,
        role,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::from([(key.to_owned(), value.to_owned())]),
        },
    }
}

#[test]
fn any_other_step_binding_the_app_token_fails_closed() -> Result<(), RenderError> {
    const APP: &str = "${{ steps.deps-token.outputs.token }}";
    for key in [
        "VELNOR_PRIVATE_FETCH_TOKEN",
        "VELNOR_NPM_READ_TOKEN",
        "GH_TOKEN",
        "CARGO_TOKEN_FOR_THING",
        "PAYLOAD",
    ] {
        let step = token_step("Build", None, key, APP);
        render_fails_with(
            vec![plan_job_with(vec![block()?[0].clone(), step])?],
            "credential_binding_outside_private_fetch",
        );
    }
    let npm = token_step(
        "Install",
        None,
        "VELNOR_NPM_READ_TOKEN",
        "${{ github.token }}",
    );
    render_fails_with(
        vec![plan_job_with(vec![npm])?],
        "credential_binding_outside_private_fetch",
    );
    Ok(())
}

#[test]
fn the_token_expression_cannot_hide_in_argv_or_a_task_env() -> Result<(), RenderError> {
    let argv = shell_step(
        "Echo",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "echo ${{ steps.deps-token.outputs.token }}".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    render_fails_with(
        vec![plan_job_with(vec![block()?[0].clone(), argv])?],
        "credential_binding_outside_private_fetch",
    );
    Ok(())
}

#[test]
fn private_steps_must_carry_the_exact_binding() -> Result<(), RenderError> {
    let [mint, mut fetch, mut revoke] = block()?;
    if let StepKind::Internal { env, .. } = &mut fetch.kind {
        env.insert(
            "VELNOR_PRIVATE_FETCH_TOKEN".to_owned(),
            "literal".to_owned(),
        );
    }
    render_fails_with(
        vec![plan_job_with(vec![mint.clone(), fetch])?],
        "private_binding_mismatch",
    );
    if let StepKind::Internal { env, .. } = &mut revoke.kind {
        env.insert(
            "VELNOR_NPM_READ_TOKEN".to_owned(),
            "${{ github.token }}".to_owned(),
        );
    }
    render_fails_with(
        vec![plan_job_with(vec![mint, revoke])?],
        "credential_binding_outside_private_fetch",
    );
    Ok(())
}

#[test]
fn the_mint_step_shape_is_closed() -> Result<(), RenderError> {
    let edits: Vec<MintEdit> = vec![
        (
            "permission",
            Box::new(|with| {
                with.insert("permission-contents".to_owned(), "write".to_owned());
            }),
        ),
        (
            "revoke",
            Box::new(|with| {
                with.insert("skip-token-revoke".to_owned(), "true".to_owned());
            }),
        ),
        (
            "extra",
            Box::new(|with| {
                with.insert("permission-packages".to_owned(), "read".to_owned());
            }),
        ),
        (
            "repositories",
            Box::new(|with| {
                with.remove("repositories");
            }),
        ),
        (
            "key",
            Box::new(|with| {
                with.insert(
                    "private-key".to_owned(),
                    "${{ secrets.OTHER }} x".to_owned(),
                );
            }),
        ),
    ];
    for (label, edit) in edits {
        let [mut mint, ..] = block()?;
        if let StepKind::Action { with, .. } = &mut mint.kind {
            edit(with);
        }
        assert!(
            render(vec![plan_job_with(vec![mint])?])
                .is_err_and(|err| format!("{err:?}").contains("private_mint_shape")),
            "{label}"
        );
    }
    Ok(())
}

#[test]
fn a_secret_in_a_non_mint_action_input_still_fails() -> Result<(), RenderError> {
    let [mut mint, ..] = block()?;
    mint.role = None;
    mint.id = None;
    render_fails_with(vec![plan_job_with(vec![mint])?], "token_in_action_input");
    Ok(())
}
