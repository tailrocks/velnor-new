//! Token hygiene: credential scoping, scrub coverage, unset-wrapper gates.

use std::collections::BTreeMap;

use velnor_actions_contract_config::WorkflowPolicy;
use velnor_actions_contract_workflow::Permissions;
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;
use velnor_actions_workflow_renderer::render_workflow_ir;
use velnor_actions_workflow_steps::{
    RenderError, ambient_shell_step, checkout_step, merge_step, plan_step, shell_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn token_hygiene_scopes_gh_token_to_plan() -> Result<(), RenderError> {
    use velnor_actions_contract_workflow::{Step, StepKind};
    use velnor_actions_workflow_steps::toolchain_env::with_credential_scrub;
    // The shell constructor rejects `github.token` env values outright:
    // only the render-time fetch binding may carry one.
    let err = shell_step(
        "Plan",
        vec!["true".to_owned()],
        BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
    )
    .expect_err("github.token in constructor env must fail");
    assert!(
        format!("{err:?}").contains("bad_env_expression"),
        "wrong rejection: {err:?}"
    );
    // Hand-built IR carrying the scoped shape still fails at render.
    let mut scrubbed = with_credential_scrub(&BTreeMap::new());
    scrubbed.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    let scoped = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            Step {
                name: "Plan".to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: scrubbed,
                },
            },
            plan_step(),
        ],
    );
    render_fails_with(vec![scoped], "bad_env_expression");
    // A nonempty denied value trips the constructor first ...
    let err = shell_step(
        "Leak",
        vec!["true".to_owned()],
        BTreeMap::from([("GITHUB_TOKEN".to_owned(), "x".to_owned())]),
    )
    .expect_err("nonempty denied key must trip constructor");
    assert!(
        format!("{err:?}").contains("credential_step_env"),
        "wrong rejection: {err:?}"
    );
    // ... and a hand-built literal bypassing the constructor still
    // trips the render-time gate.
    let bad_env = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            Step {
                name: "Leak".to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: BTreeMap::from([("GITHUB_TOKEN".to_owned(), "x".to_owned())]),
                },
            },
            plan_step(),
        ],
    );
    render_fails_with(vec![bad_env], "credential_env");
    Ok(())
}

#[test]
fn token_hygiene_allows_final_fetch_token() -> Result<(), RenderError> {
    use velnor_actions_workflow_steps::steps::{FETCH_OPERATION, internal_step};
    // The fetch binding is render-time only: the internal op renders
    // its fixed `GH_TOKEN`, never carried through IR env.
    let (id, mut final_job) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            checkout_step(&checkout_pin())?,
            internal_step("Download every expected matrix artifact", FETCH_OPERATION)?,
            merge_step(),
        ],
    );
    final_job.condition = Some("always()".to_owned());
    final_job.permissions = Some(Permissions {
        contents: PermissionLevel::Read,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::Read,
    });
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, (id, final_job)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains("GH_TOKEN: ${{ github.token }}"),
        "fetch must bind the token:\n{text}"
    );
    Ok(())
}

#[test]
fn token_hygiene_rejects_prints_and_task_tokens() -> Result<(), RenderError> {
    let printed = token_plan_job(
        "Task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "echo $GH_TOKEN".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    render_fails_with(vec![printed], "token_in_run");
    let task_token = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![velnor_actions_contract_workflow::Step {
            name: "Run task".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: velnor_actions_contract_workflow::StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
            },
        }],
    );
    render_fails_with(vec![minimal_plan_job()?, task_token], "token_misplaced");
    Ok(())
}

#[test]
fn token_hygiene_constructor_owns_overlay_and_rejects_all_nine_keys() -> Result<(), RenderError> {
    use velnor_actions_contract_workflow::{Step, StepKind};
    use velnor_actions_workflow_steps::toolchain_env::STEP_CREDENTIAL_DENYLIST;
    // Bare env: the constructor scrubs shut, so it renders clean.
    let clean = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![shell_step(
            "Run task",
            vec!["true".to_owned()],
            BTreeMap::new(),
        )?],
    );
    render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, clean]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    // Caller-supplied denied keys fail loud even when empty: the
    // constructor alone owns the overlay, never the caller.
    let scrub: BTreeMap<String, String> = STEP_CREDENTIAL_DENYLIST
        .iter()
        .map(|key| ((*key).to_owned(), String::new()))
        .collect();
    let err = shell_step("Run task", vec!["true".to_owned()], scrub)
        .expect_err("caller-supplied denied keys must trip constructor");
    assert!(
        format!("{err:?}").contains("credential_step_env"),
        "wrong rejection: {err:?}"
    );
    for key in STEP_CREDENTIAL_DENYLIST {
        let err = shell_step(
            "Run task",
            vec!["true".to_owned()],
            BTreeMap::from([(key.to_owned(), "x".to_owned())]),
        )
        .expect_err("nonempty sensitive key must trip constructor");
        assert!(
            format!("{err:?}").contains("credential_step_env"),
            "key {key} denied at constructor: {err:?}"
        );
        // Defense in depth: a Step literal bypassing the constructor
        // still trips the render-time gate.
        let leaked = job(
            "velnor-task",
            "Task",
            vec!["plan".to_owned()],
            vec![Step {
                name: "Run task".to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: BTreeMap::from([(key.to_owned(), "x".to_owned())]),
                },
            }],
        );
        let want = if key == "GH_TOKEN" {
            "token_misplaced"
        } else {
            "credential_env"
        };
        render_fails_with(vec![minimal_plan_job()?, leaked], want);
    }
    Ok(())
}

#[test]
fn scrub_coverage_rejects_bare_and_partial_shell_env() -> Result<(), RenderError> {
    use velnor_actions_contract_workflow::{Step, StepKind};
    // Hand-built literals: the constructor would scrub these shut, so
    // only a literal bypassing it reaches the gate uncovered.
    for (label, env) in [
        ("bare", BTreeMap::new()),
        (
            "partial",
            BTreeMap::from([("GITHUB_TOKEN".to_owned(), String::new())]),
        ),
    ] {
        let uncovered = job(
            "plan",
            "Plan",
            Vec::new(),
            vec![
                checkout_step(&checkout_pin())?,
                Step {
                    name: "Run task".to_owned(),
                    id: None,
                    role: None,
                    condition: None,
                    kind: StepKind::Shell {
                        run: vec!["true".to_owned()],
                        env,
                    },
                },
                plan_step(),
            ],
        );
        let err = render_workflow_ir(
            &fixture_ir(vec![uncovered]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )
        .expect_err("bare or partial env must fail coverage");
        assert!(
            format!("{err:?}").contains("missing_scrub"),
            "{label}: {err:?}"
        );
    }
    Ok(())
}

#[test]
fn scrub_coverage_allows_ambient_auth_steps_and_release() -> Result<(), RenderError> {
    use velnor_actions_contract_workflow::StepRole;
    use velnor_actions_workflow_steps::steps::{DENY_STEP_NAME, MACHETE_STEP_NAME};
    // Ambient constructor (no scrub overlay): the typed role is the
    // authority, independent of the presentation name.
    for (name, role) in [
        ("Pinned tools", StepRole::PreparePinnedTools),
        ("Rust components", StepRole::PrepareRustComponents),
        ("Cargo source fetch", StepRole::CargoSourcesFetch),
        ("Nested source fetch", StepRole::CargoSourcesFetch),
        (DENY_STEP_NAME, StepRole::CargoDeny),
        (MACHETE_STEP_NAME, StepRole::CargoMachete),
        ("Run zizmor", StepRole::Zizmor),
        ("Run actionlint", StepRole::Actionlint),
    ] {
        let mut ambient = ambient_shell_step(name, vec!["true".to_owned()], BTreeMap::new())?;
        ambient.role = Some(role);
        let allowed = job(
            "plan",
            "Plan",
            Vec::new(),
            vec![checkout_step(&checkout_pin())?, ambient, plan_step()],
        );
        render_workflow_ir(
            &fixture_ir(vec![allowed]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )?;
    }
    let mislabeled = ambient_shell_step(
        "Prepare pinned tools",
        vec!["true".to_owned()],
        BTreeMap::new(),
    )?;
    let denied = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, mislabeled, plan_step()],
    );
    assert!(
        format!(
            "{:?}",
            render_workflow_ir(
                &fixture_ir(vec![denied]),
                WorkflowPolicy::ConsumerV1,
                None,
                &fixture_ctx(),
            )
            .expect_err("display names carry no ambient-auth authority")
        )
        .contains("missing_scrub"),
        "renamed or forged presentation names do not grant auth"
    );
    let release = job(
        "release",
        "Release",
        vec!["plan".to_owned()],
        vec![ambient_shell_step(
            "Publish release assets",
            vec!["true".to_owned()],
            BTreeMap::new(),
        )?],
    );
    render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, release]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &fixture_ctx(),
    )?;
    Ok(())
}

#[test]
fn unset_wrapper_passes_but_exfil_behind_it_fails() -> Result<(), RenderError> {
    // The step constructor applies the argv wrapper itself; bare payloads
    // must scan clean while exfil behind the wrapper still fails.
    let wrapped = token_plan_job(
        "Run task",
        ["mise".to_owned(), "run".to_owned(), "x".to_owned()].to_vec(),
        BTreeMap::new(),
    )?;
    render_workflow_ir(
        &fixture_ir(vec![wrapped]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    // Bare script: the constructor preludes once, and the single
    // prelude strips clean. (A caller-preluded script would double the
    // prelude and trip the scanner — fail-closed by design.)
    let scripted = token_plan_job(
        "Run task",
        vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
        BTreeMap::new(),
    )?;
    render_workflow_ir(
        &fixture_ir(vec![scripted]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let smuggled = token_plan_job(
        "Run task",
        ["echo".to_owned(), "$GH_TOKEN".to_owned()].to_vec(),
        BTreeMap::new(),
    )?;
    render_fails_with(vec![smuggled], "token_in_run");
    let fake_prelude = token_plan_job(
        "Run task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "unset FOO; echo $GH_TOKEN".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    render_fails_with(vec![fake_prelude], "token_in_run");
    Ok(())
}
