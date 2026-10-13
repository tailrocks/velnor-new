//! P08 renderer cases: V2 tools selectors and exact sources paths.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, JobTimeout, StepKind};
use velnor_actions_workflow_renderer::cache_p08::{
    check_mbx_before_fetch, check_no_legacy_rust_cache, infer_job_tools,
};
use velnor_actions_workflow_renderer::steps::cache_action_step;
use velnor_actions_workflow_renderer::{MiseSetup, setup::mise_setup_step};

use super::impl_renderer_fixtures::*;

#[test]
fn job_tools_inferred_from_install_and_exec() {
    let job = Job {
        display_name: "Demo".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            velnor_actions_workflow_renderer::shell_step(
                "Prepare pinned tools",
                vec![
                    "mise".to_owned(),
                    "--no-config".to_owned(),
                    "install".to_owned(),
                    "rust@1.98.1".to_owned(),
                ],
                BTreeMap::new(),
            )
            .expect("prepare"),
            velnor_actions_workflow_renderer::shell_step(
                "Run lint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
                BTreeMap::new(),
            )
            .expect("lint"),
        ],
    };
    let tools = infer_job_tools(&job);
    assert!(tools.contains(&"rust@1.98.1".to_owned()), "{tools:?}");
    assert!(tools.contains(&"actionlint@1.7.12".to_owned()), "{tools:?}");
}

#[test]
fn job_tools_inferred_from_inline_shell_script() {
    // Isolation steps join fixed `mise install`/`exec` argv into `sh -c`
    // scripts (deny/fetch): the detectors must see inside, or the job
    // loses its Setup Mise bootstrap and fails at runtime.
    let script = "{ mise --no-config install cargo-deny@0.20.2 && unset GH_TOKEN; } \
        && mkdir -p \"$RUNNER_TEMP/velnor/cargo-clean\" \
        && cd \"$RUNNER_TEMP/velnor/cargo-clean\" \
        && mise --no-config exec cargo-deny@0.20.2 -- cargo deny --locked check";
    let job = Job {
        display_name: "Cargo Deny".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            velnor_actions_workflow_renderer::ambient_shell_step(
                "Run cargo-deny",
                vec!["sh".to_owned(), "-c".to_owned(), script.to_owned()],
                BTreeMap::new(),
            )
            .expect("deny"),
        ],
    };
    assert_eq!(infer_job_tools(&job), vec!["cargo-deny@0.20.2".to_owned()]);
}

#[test]
fn job_tools_inferred_from_quoted_spec() {
    // A drift into quoted specs must still bootstrap instead of
    // silently dropping the setup step.
    let job = Job {
        display_name: "Demo".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            velnor_actions_workflow_renderer::shell_step(
                "Run task",
                vec![
                    "sh".to_owned(),
                    "-c".to_owned(),
                    "mise install 'quoted-tool@1.0'".to_owned(),
                ],
                BTreeMap::new(),
            )
            .expect("task"),
        ],
    };
    assert_eq!(infer_job_tools(&job), vec!["quoted-tool@1.0".to_owned()]);
}

#[test]
fn setup_disables_action_owned_cache_for_v2_tools_layer() {
    let step = mise_setup_step(&MiseSetup {
        uses: MISE_USES.to_owned(),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_SHA256.to_owned(),
    })
    .expect("setup");
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("setup must be an action step");
    };
    assert_eq!(with.get("cache").map(String::as_str), Some("false"));
    assert_eq!(
        with.get("cache_save").map(String::as_str),
        Some("false"),
        "the action owns no cache restore or save"
    );
    assert!(!with.contains_key("cache_key"));
    assert_eq!(with.len(), 6);
}

#[test]
fn sources_subset_accepted_under_owned_home_only() {
    let sha = "b".repeat(40);
    let uses = format!("actions/cache/restore@{sha}");
    let home = "${{ runner.temp }}/velnor/cargo";
    let good = [
        format!("{home}/registry/cache"),
        format!("{home}/registry/index"),
        format!("{home}/git/db"),
    ];
    assert!(
        cache_action_step(true, &uses, "sources", "k", &[], &good).is_ok(),
        "subset must parse"
    );
    for bad in [
        format!("{home}/registry/src/x"),
        format!("{home}/.crates.toml"),
        format!("{home}/.crates2.json"),
        format!("{home}/bin"),
        format!("{home}/credentials.toml"),
        format!("{home}/../escape"),
        "~/.cargo/registry/cache".to_owned(),
    ] {
        assert!(
            cache_action_step(true, &uses, "sources", "k", &[], std::slice::from_ref(&bad))
                .is_err(),
            "must reject {bad}"
        );
    }
}

#[test]
fn retired_rust_cache_is_rejected_for_every_lane() {
    let sha = "c".repeat(40);
    let [preflight, mbx] = mbx_tool_steps(
        &format!("jdx/mr-boxington-action@{sha}"),
        "1.19.0",
        "1.98.1",
    )
    .expect("mbx steps");
    let rust_cache = velnor_actions_contract::Step {
        name: "Restore Cargo registry".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: format!("Swatinem/rust-cache@{sha}"),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let cargo_only = Job {
        display_name: "Both".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![rust_cache.clone()],
    };
    let both = Job {
        steps: vec![preflight, mbx, rust_cache],
        ..cargo_only.clone()
    };
    assert!(check_no_legacy_rust_cache("cargo-only", &cargo_only).is_err());
    assert!(check_no_legacy_rust_cache("both", &both).is_err());
}

#[test]
fn mbx_restore_precedes_fetch() {
    let fetch = velnor_actions_contract::Step {
        name: "Fetch Cargo sources".to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let mbx = velnor_actions_contract::Step {
        name: "Restore MBX objects".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: format!("jdx/mr-boxington-action@{}", "d".repeat(40)),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let good = Job {
        display_name: "Good".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![mbx.clone(), fetch.clone()],
    };
    assert!(check_mbx_before_fetch("demo", &good).is_ok());
    let bad = Job {
        steps: vec![fetch, mbx],
        ..good.clone()
    };
    assert!(check_mbx_before_fetch("demo", &bad).is_err());
}

#[test]
fn step_conditions_serialize_as_if_with_upload_default()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
    use velnor_actions_workflow_renderer::{action_step, matrix_report_upload_step};
    let mut save = action_step(
        "Save Cargo sources",
        &format!("actions/cache/save@{}", "c".repeat(40)),
        BTreeMap::from([("key".to_owned(), "k".to_owned())]),
    )?;
    save.condition = Some(CACHE_SAVE_CONDITION.to_owned());
    let mut check = scrubbed_shell_step(
        "Check",
        vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "rust@1.98.1".to_owned(),
        ],
    )?;
    check.condition = Some(CACHE_SAVE_CONDITION.to_owned());
    let plain = scrubbed_shell_step("Plain", vec!["true".to_owned()])?;
    let upload = matrix_report_upload_step()?;
    let text = strict(
        &fixture_ir(vec![job(
            "demo",
            "Demo",
            Vec::new(),
            vec![save, check, plain, upload],
        )]),
        &fixture_ctx(),
    )?;
    let save_at = text.find("Save Cargo sources").expect("save step");
    assert!(
        text[save_at..].starts_with(
            "Save Cargo sources\n        if: success() && github.event_name == 'push'",
        ),
        "save carries push-only if:\n{text}"
    );
    let check_at = text.find("- name: Check\n").expect("check step");
    assert!(
        text[check_at..]
            .starts_with("- name: Check\n        if: success() && github.event_name == 'push'"),
        "shell condition serializes:\n{text}"
    );
    let plain_at = text.find("- name: Plain\n").expect("plain step");
    assert!(
        !text[plain_at..].starts_with("- name: Plain\n        if:"),
        "unconditioned steps stay if-free:\n{text}"
    );
    let upload_at = text.find("Upload matrix report").expect("upload step");
    assert!(
        text[upload_at..].starts_with("Upload matrix report\n        if: always()"),
        "upload keeps always() default:\n{text}"
    );
    Ok(())
}

#[test]
fn strict_render_elects_single_writer_per_shared_key()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::{plan_step, shell_step};
    let prepare = || {
        shell_step(
            "Prepare pinned tools",
            vec![
                "mise".to_owned(),
                "install".to_owned(),
                "rust@1.98.1".to_owned(),
            ],
            BTreeMap::new(),
        )
    };
    let text = strict(
        &fixture_ir(vec![
            job(
                "plan",
                "Plan",
                Vec::new(),
                vec![prepare()?, acquire_fixture()?, plan_step()],
            ),
            job(
                "rust-demo",
                "Rust / demo",
                vec!["plan".to_owned()],
                vec![prepare()?],
            ),
        ]),
        &fixture_ctx(),
    )?;
    let plan_at = text.find("\n  plan:\n").expect("plan block");
    let crate_at = text.find("\n  rust-demo:\n").expect("crate block");
    let (plan_block, crate_block) = text.split_at(crate_at);
    let plan_block = &plan_block[plan_at..];
    assert!(
        plan_block.contains("- name: Save Mise tools"),
        "plan wins the shared key:\n{text}"
    );
    assert!(
        plan_block.contains("if: success() && github.event_name == 'push'"),
        "winner saves push-only:\n{text}"
    );
    assert!(
        !crate_block.contains("Save Mise tools"),
        "crate restores read-only:\n{text}"
    );
    assert_eq!(
        text.matches("- name: Save Mise tools").count(),
        1,
        "exactly one saver per key:\n{text}"
    );
    assert!(
        !text.contains("cache_save: ${{"),
        "no setup promises a built-in save:\n{text}"
    );
    Ok(())
}
