//! Canonical Mise cache generator and ordering regressions.

use std::collections::BTreeMap;

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;
use velnor_actions_workflow_renderer::steps::{
    TOOLS_CACHE_PATHS, TOOLS_RESTORE_NAME, TOOLS_SAVE_NAME, cache_action_step,
    tools_cache_key_for_tools, tools_cache_path_input, tools_restore_step, tools_save_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn tools_cache_key_binds_runner_image_and_tool_union() {
    let key = tools_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        MISE_SHA256,
        &["rust@1.98.1".to_owned()],
    )
    .expect("tools key");
    assert!(
        key.starts_with("mise-v3-${{ runner.os }}-${{ runner.arch }}-"),
        "{key}"
    );
    for part in [
        "env.VELNOR_CACHE_IMAGE_OS",
        "env.VELNOR_CACHE_IMAGE_VERSION",
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
    ] {
        assert!(key.contains(part), "key misses {part}: {key}");
    }
    let key_without_owned_expressions = key
        .replace("${{ runner.os }}", "")
        .replace("${{ runner.arch }}", "")
        .replace("${{ env.VELNOR_CACHE_IMAGE_OS }}", "")
        .replace("${{ env.VELNOR_CACHE_IMAGE_VERSION }}", "");
    assert!(
        key_without_owned_expressions
            .chars()
            .all(|character| !character.is_whitespace()),
        "unexpected whitespace outside owned image expressions: {key}"
    );
    for bad in [
        ("", "2026.9.16", vec!["rust@1.98.1".to_owned()]),
        (
            "x86_64-unknown-linux-gnu",
            "latest",
            vec!["rust@1.98.1".to_owned()],
        ),
        ("x86_64-unknown-linux-gnu", "2026.9.16", vec![]),
    ] {
        assert!(tools_cache_key_for_tools(bad.0, bad.1, MISE_SHA256, &bad.2).is_err());
    }
}

#[test]
fn tools_restore_and_save_use_one_ordered_payload_without_prefixes() {
    let key = tools_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        MISE_SHA256,
        &["rust@1.98.1".to_owned()],
    )
    .expect("tools key");
    let restore = tools_restore_step(&key).expect("restore");
    assert_eq!(restore.name, TOOLS_RESTORE_NAME);
    let StepKind::Action { uses, with, .. } = &restore.kind else {
        panic!("restore must be an action step");
    };
    assert!(uses.starts_with("actions/cache/restore@"), "{uses}");
    assert_eq!(with.get("key").map(String::as_str), Some(key.as_str()));
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(tools_cache_path_input().as_str())
    );
    assert!(
        !with.contains_key("restore-keys"),
        "restore has exact key only"
    );
    assert_eq!(TOOLS_CACHE_PATHS.len(), 6);
    let save = tools_save_step(&key).expect("save");
    assert_eq!(save.name, TOOLS_SAVE_NAME);
    let StepKind::Action { uses, with, .. } = &save.kind else {
        panic!("save must be an action step");
    };
    assert!(uses.starts_with("actions/cache/save@"), "{uses}");
    assert_eq!(with.get("key").map(String::as_str), Some(key.as_str()));
    assert_eq!(
        with.get("path").map(String::as_str),
        Some(tools_cache_path_input().as_str())
    );
    assert!(
        !with.contains_key("restore-keys"),
        "save has no restore keys"
    );
    assert!(
        cache_action_step(
            true,
            "actions/cache/restore@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "tools",
            &key,
            &[],
            &["$CARGO_HOME/registry".to_owned()]
        )
        .is_err(),
        "tools layer rejects non-mise paths"
    );
}

#[test]
fn strict_restores_canonical_payload_before_setup_and_saves_on_writer()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::checkout_step;
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![lint]), &fixture_ctx())?;
    let names = step_names(&text, "actionlint");
    assert_eq!(
        names.iter().filter(|s| *s == TOOLS_RESTORE_NAME).count(),
        1,
        "one explicit restore: {names:?}"
    );
    assert_eq!(
        names.iter().filter(|s| *s == TOOLS_SAVE_NAME).count(),
        1,
        "P08: sole owner saves once: {names:?}"
    );
    assert_eq!(
        names.iter().position(|s| s == "Setup Mise"),
        Some(4),
        "image, restore, and bootstrap guard precede setup: {names:?}"
    );
    for need in [
        "cache: \"false\"",
        "MISE_DATA_DIR: ${{ runner.temp }}/velnor/mise-bootstrap",
    ] {
        assert!(text.contains(need), "explicit setup {need}:\n{text}");
    }
    assert!(text.contains("actions/cache/restore@55cc834"));
    assert!(text.contains("actions/cache/save@55cc834"));
    Ok(())
}

#[test]
fn strict_render_elects_single_writer_per_shared_key()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    let text = render_shared_writer_fixture()?;
    assert_writer_gate_and_order(&text);
    assert_single_save_across_jobs(&text);
    Ok(())
}

fn render_shared_writer_fixture() -> Result<String, velnor_actions_workflow_renderer::RenderError> {
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
    let verify = shell_step(
        "Last pinned tool verification",
        vec![
            "mise".to_owned(),
            "exec".to_owned(),
            "rust@1.98.1".to_owned(),
            "--".to_owned(),
            "rustc".to_owned(),
            "--version".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    strict(
        &fixture_ir(vec![
            job(
                "plan",
                "Plan",
                Vec::new(),
                vec![prepare()?, verify, acquire_fixture()?, plan_step()],
            ),
            job(
                "rust-demo",
                "Rust / demo",
                vec!["plan".to_owned()],
                vec![prepare()?],
            ),
        ]),
        &fixture_ctx(),
    )
}

fn assert_writer_gate_and_order(text: &str) {
    let plan_at = text.find("\n  plan:\n").expect("plan block");
    let crate_at = text.find("\n  rust-demo:\n").expect("crate block");
    let (plan_block, crate_block) = text.split_at(crate_at);
    let plan_block = &plan_block[plan_at..];
    assert!(
        plan_block.contains("- name: Save Mise tools"),
        "plan wins the shared key:\n{text}"
    );
    let expected_save_gate =
        format!("if: {CACHE_SAVE_CONDITION} && env.VELNOR_CACHE_IMAGE_ELIGIBLE == 'true'");
    assert!(
        plan_block.contains(&expected_save_gate),
        "winner saves only on the trusted compatible runner:\n{text}"
    );
    let restore_at = plan_block
        .find("- name: Restore Mise tools")
        .expect("restore");
    let guard_at = plan_block
        .find("- name: Verify restored Mise bootstrap")
        .expect("bootstrap guard");
    let setup_at = plan_block.find("- name: Setup Mise").expect("setup");
    let install_at = plan_block
        .find("- name: Prepare pinned tools")
        .expect("tool installation");
    let verify_at = plan_block
        .find("- name: Last pinned tool verification")
        .expect("last tool producer");
    let save_at = plan_block.find("- name: Save Mise tools").expect("save");
    assert!(
        restore_at < guard_at
            && guard_at < setup_at
            && setup_at < install_at
            && install_at < verify_at
            && verify_at < save_at,
        "full tool production precedes save:\n{plan_block}"
    );
    assert!(
        !crate_block.contains("Save Mise tools"),
        "crate restores read-only:\n{text}"
    );
}

fn assert_single_save_across_jobs(text: &str) {
    assert_eq!(
        text.matches("- name: Save Mise tools").count(),
        1,
        "exactly one saver per key:\n{text}"
    );
    assert!(
        !text.contains("cache_save: ${{"),
        "no setup promises a built-in save:\n{text}"
    );
}
