//! P08 renderer cases: built-in Mise cache, sources paths, rust-cache gates.

use std::collections::BTreeMap;
use velnor_actions_contract::{Job, StepKind};
use velnor_actions_workflow_renderer::cache_p08::{
    check_mbx_before_fetch, check_no_rust_cache_with_mbx, infer_job_tools,
    mise_cache_key_for_tools, mise_setup_step_p08, tools_digest,
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
    assert_eq!(with.get("cache_save").map(String::as_str), Some("true"));
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
    let mbx = velnor_actions_workflow_renderer::steps::mbx_objects_step(
        &format!("jdx/mr-boxington-action@{sha}"),
        false,
    )
    .expect("mbx");
    let rust_cache = velnor_actions_contract::Step {
        name: "Restore Cargo registry".to_owned(),
        kind: StepKind::Action {
            uses: format!("Swatinem/rust-cache@{sha}"),
            with: BTreeMap::new(),
        },
    };
    let both = Job {
        display_name: "Both".to_owned(),
        runs_on: LABEL.to_owned(),
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![mbx.clone(), rust_cache.clone()],
    };
    assert!(check_no_rust_cache_with_mbx("demo", &both).is_err());
    let cargo_only = Job {
        steps: vec![rust_cache],
        ..both.clone()
    };
    assert!(check_no_rust_cache_with_mbx("demo", &cargo_only).is_ok());
    let mbx_only = Job {
        steps: vec![mbx],
        ..both.clone()
    };
    assert!(check_no_rust_cache_with_mbx("demo", &mbx_only).is_ok());
}

#[test]
fn mbx_restore_precedes_fetch() {
    let fetch = velnor_actions_contract::Step {
        name: "Fetch Cargo sources".to_owned(),
        kind: StepKind::Shell {
            run: vec!["sh".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let mbx = velnor_actions_contract::Step {
        name: "Restore MBX objects".to_owned(),
        kind: StepKind::Action {
            uses: format!("jdx/mr-boxington-action@{}", "d".repeat(40)),
            with: BTreeMap::new(),
        },
    };
    let good = Job {
        display_name: "Good".to_owned(),
        runs_on: LABEL.to_owned(),
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
