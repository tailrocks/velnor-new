//! P08 renderer cases: built-in Mise cache, sources paths, rust-cache gates.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, JobTimeout, StepKind};
use velnor_actions_workflow_renderer::cache_p08::{
    check_no_rust_cache_with_mbx, infer_job_tools, mise_cache_key_for_tools, mise_setup_step_p08,
    tools_digest,
};
use velnor_actions_workflow_renderer::steps::cache_action_step;

use super::impl_renderer_fixtures::*;

#[test]
fn builtin_key_shares_same_tools_without_job_id() {
    let a = ["rust@1.98.1".to_owned(), "mr-boxington@1.19.0".to_owned()];
    let b = ["mr-boxington@1.19.0".to_owned(), "rust@1.98.1".to_owned()];
    let one = mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &a).expect("key");
    let two = mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &b).expect("key");
    assert_eq!(one, two, "tool order must not fork keys");
    assert!(!one.contains("plan") && !one.contains("rust-"), "{one}");
    assert!(
        one.starts_with("mise-v1-x86_64-unknown-linux-gnu-2026.9.16-"),
        "{one}"
    );
    let other = mise_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        &["actionlint@1.7.12".to_owned()],
    )
    .expect("other");
    assert_ne!(one, other, "distinct tools need distinct keys");
    assert!(mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "latest", &a).is_err());
    assert!(mise_cache_key_for_tools("riscv-none", "2026.9.16", &a).is_err());
    assert!(mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &[]).is_err());
}

#[test]
fn tools_digest_is_order_stable_short_hex() {
    let digest = tools_digest(&["b@2".to_owned(), "a@1".to_owned()]);
    assert_eq!(digest.len(), 16);
    assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        digest,
        tools_digest(&["a@1".to_owned(), "b@2".to_owned(), "a@1".to_owned()])
    );
}

#[test]
fn job_tools_inferred_from_install_and_exec() {
    let job = Job {
        display_name: "Demo".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
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
        check_runner: None,
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
        check_runner: None,
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
fn setup_p08_enables_builtin_cache_with_key() {
    let key = mise_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        &["rust@1.98.1".to_owned()],
    )
    .expect("key");
    let step = mise_setup_step_p08(&mise(), &key).expect("setup");
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("setup must be an action step");
    };
    assert_eq!(with.get("cache").map(String::as_str), Some("true"));
    assert_eq!(
        with.get("cache_save").map(String::as_str),
        Some("false"),
        "setups restore-only: the action saves only inside its disabled install leg"
    );
    assert_eq!(
        with.get("cache_key").map(String::as_str),
        Some(key.as_str())
    );
    assert_eq!(with.len(), 7);
    assert!(mise_setup_step_p08(&mise(), "bad key").is_err());
    assert!(mise_setup_step_p08(&mise(), "mise-tools-v1-plan").is_err());
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
        format!("{home}/.crates.toml"),
    ];
    assert!(
        cache_action_step(true, &uses, "sources", "k", &[], &good).is_ok(),
        "subset must parse"
    );
    for bad in [
        format!("{home}/registry/src/x"),
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
fn rust_cache_never_stacks_over_mbx() {
    let sha = "c".repeat(40);
    let [preflight, mbx, version_check] = mbx_tool_steps(
        &format!("jdx/mr-boxington-action@{sha}"),
        "1.19.0",
        "1.98.1",
    )
    .expect("mbx steps");
    let rust_cache = velnor_actions_contract::Step {
        name: "Restore Cargo registry".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Action {
            uses: format!("Swatinem/rust-cache@{sha}"),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let both = Job {
        display_name: "Both".to_owned(),
        runs_on: LABEL.to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            preflight.clone(),
            mbx.clone(),
            version_check.clone(),
            rust_cache.clone(),
        ],
    };
    assert!(check_no_rust_cache_with_mbx("demo", &both).is_err());
    let cargo_only = Job {
        steps: vec![rust_cache],
        ..both.clone()
    };
    assert!(check_no_rust_cache_with_mbx("demo", &cargo_only).is_ok());
    let mbx_only = Job {
        steps: vec![mbx, version_check],
        ..both.clone()
    };
    assert!(check_no_rust_cache_with_mbx("demo", &mbx_only).is_ok());
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
