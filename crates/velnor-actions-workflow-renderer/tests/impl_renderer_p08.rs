//! P08 renderer cases: canonical tool cache, source paths, rust-cache gates.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, JobTimeout, StepKind};
use velnor_actions_workflow_renderer::cache_p08::{
    check_mbx_before_fetch, check_no_rust_cache_with_mbx, infer_job_tools,
    mise_cache_key_for_tools, tools_digest,
};
use velnor_actions_workflow_renderer::steps::cache_action_step;

use super::impl_renderer_fixtures::*;

#[test]
fn canonical_key_shares_same_tools_without_job_id() {
    let a = ["rust@1.98.1".to_owned(), "mr-boxington@1.19.0".to_owned()];
    let b = ["mr-boxington@1.19.0".to_owned(), "rust@1.98.1".to_owned()];
    let one = mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &a).expect("key");
    let two = mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &b).expect("key");
    assert_eq!(one, two, "tool order must not fork keys");
    assert!(!one.contains("plan") && !one.contains("rust-"), "{one}");
    assert!(
        one.starts_with("mise-v3-x86_64-unknown-linux-gnu-2026.9.16-"),
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
        cache_mode: None,
        display_name: "Demo".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
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
        cache_mode: None,
        display_name: "Cargo Deny".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
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
        cache_mode: None,
        display_name: "Demo".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
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
fn setup_p08_uses_exact_owned_bootstrap_without_action_cache() {
    let setup = mise();
    let domain = velnor_actions_contract::ToolCacheDomain::Full;
    let step = velnor_actions_workflow_renderer::mise_setup_step(&setup, domain, LABEL)
        .expect("qualified setup");
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        panic!("setup must be an owner-bound helper");
    };
    assert_eq!(
        invocation,
        setup.bootstraps[&(domain, LABEL.to_owned())]
            .helper
            .invocation()
    );
    assert_eq!(env["MISE_DATA_DIR"], domain.root());
    assert_eq!(env["VELNOR_MISE_VERSION"], MISE_VERSION);
    assert_eq!(env["VELNOR_MISE_SHA256"], MISE_SHA256);
    assert_eq!(env.len(), 5);
    assert_eq!(env["VELNOR_MISE_TARGET"], "x86_64-unknown-linux-gnu");
    assert!(!env.contains_key("cache"));
    assert!(
        velnor_actions_workflow_renderer::mise_setup_step(&setup, domain, "ubuntu-latest").is_err()
    );
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
    let mbx = velnor_actions_workflow_renderer::steps::mbx_objects_step(
        &format!("jdx/mr-boxington-action@{sha}"),
        false,
        "1.19.0",
    )
    .expect("mbx");
    let rust_cache = velnor_actions_contract::Step {
        id: None,
        name: "Restore Cargo registry".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: format!("Swatinem/rust-cache@{sha}"),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let both = Job {
        cache_mode: None,
        display_name: "Both".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: vec![mbx.clone(), rust_cache.clone()],
    };
    assert!(check_no_rust_cache_with_mbx("demo", &both).is_err());
    let cargo_only = Job {
        cache_mode: None,
        steps: vec![rust_cache],
        ..both.clone()
    };
    assert!(check_no_rust_cache_with_mbx("demo", &cargo_only).is_ok());
    let mbx_only = Job {
        cache_mode: None,
        steps: vec![mbx],
        ..both.clone()
    };
    assert!(check_no_rust_cache_with_mbx("demo", &mbx_only).is_ok());
}

#[test]
fn mbx_restore_precedes_fetch() {
    let fetch = velnor_actions_contract::Step {
        id: None,
        name: "Fetch Cargo sources".to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["sh".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let mbx = velnor_actions_contract::Step {
        id: None,
        name: "Restore MBX objects".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: format!("jdx/mr-boxington-action@{}", "d".repeat(40)),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let good = Job {
        cache_mode: None,
        display_name: "Good".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: vec![mbx.clone(), fetch.clone()],
    };
    assert!(check_mbx_before_fetch("demo", &good).is_ok());
    let bad = Job {
        cache_mode: None,
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

#[path = "impl_renderer_p08_writer.rs"]
mod writer;
