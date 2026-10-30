// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_actionlint::actions::MISE_ACTION_SHA;
use velnor_actions_contract::cachekey::toolchain_id;
use velnor_actions_contract::canonical::{canonical_json_bytes, digest_b3};
use velnor_actions_contract::{Step, StepKind, expand_workflow};
use velnor_actions_mise::MISE_VERSION;
use velnor_actions_orchestrator::{
    GenerationPreparation, OrchestratorError, prepare, render_staged_tree,
};
use velnor_actions_workflow_renderer::render::{
    finalize_jobs, render_workflow_ir, render_workflow_ir_strict, render_workflow_ir_strict_shared,
};
use velnor_actions_workflow_renderer::setup::MISE_BINARY_SHA256_LINUX_X64;
use velnor_actions_workflow_renderer::{MiseSetup, RenderError};

use self::support::{
    LegacyToolchainInputs, SIGNER, VERSION, assert_selected_hk, config_with_test_tools,
    configured_repo, configured_repo_with_lock, execution_inputs, expect_config_problem, find_task,
    is_test_task, mise_config, prepare_with_pin, selected_hk, task_by_id, valid_lock,
};
use super::workflow_configuration_fixture;

#[path = "crate_test_tools_support.rs"]
mod support;

type DirectRenderResults = [(&'static str, Result<(), RenderError>); 4];

#[test]
fn prepare_scopes_hk_to_owner_tests_and_render_refuses_unprepared_tools()
-> Result<(), Box<dyn Error>> {
    let repo = configured_repo(&config_with_test_tools("demo = [\"hk\"]\n", None), "nested")?;
    let prepared = prepare(repo.path())?;
    let inputs = execution_inputs(&prepared);
    let mut selected_tasks = 0;

    for (task_id, toolchain) in &inputs {
        let task = task_by_id(&prepared, task_id)?;
        let should_select =
            task.stack_id == "rust" && task.display_name == "demo" && is_test_task(task);
        assert_eq!(
            !toolchain.selected_test_tools.is_empty(),
            should_select,
            "selected tool scope for {task_id}"
        );
        if should_select {
            selected_tasks += 1;
            assert_selected_hk(&toolchain.selected_test_tools, VERSION)?;
        }
    }
    assert!(selected_tasks > 0);

    match render_staged_tree(&prepared) {
        Err(OrchestratorError::Contract { problem }) => {
            assert_eq!(problem, "selected_test_tool_preparation_unavailable");
        }
        Err(error) => return Err(format!("unexpected render error: {error}").into()),
        Ok(_) => return Err("unprepared selected tools rendered a workflow".into()),
    }
    assert_unprepared_refusals(direct_render_results(&prepared)?)?;
    Ok(())
}

#[test]
fn absent_declaration_keeps_legacy_toolchain_identity_and_renders() -> Result<(), Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(
        &workflow_configuration_fixture::config_with_verification_branch(None),
    )?;
    let prepared = prepare(repo.path())?;
    let inputs = execution_inputs(&prepared);
    let (_, toolchain) = inputs.first_key_value().ok_or("missing task execution")?;
    assert_eq!(toolchain.selected_test_tools, Vec::new());
    assert_eq!(
        serde_json::to_value(toolchain)?,
        serde_json::to_value(LegacyToolchainInputs::from(toolchain))?
    );
    assert_eq!(
        toolchain_id(toolchain)?,
        digest_b3(&canonical_json_bytes(&LegacyToolchainInputs::from(
            toolchain
        ))?)
    );
    let task_step = prepared
        .workflow
        .ir
        .jobs
        .values()
        .flat_map(|job| &job.steps)
        .find(|step| matches!(&step.kind, StepKind::TaskExecution { .. }))
        .ok_or("missing task execution step")?;
    let serialized = serde_json::to_value(task_step)?;
    assert!(
        serialized
            .get("toolchain_inputs")
            .is_some_and(serde_json::Value::is_object)
    );
    let decoded: Step = serde_json::from_value(serialized)?;
    let StepKind::TaskExecution {
        toolchain_inputs: decoded_inputs,
        ..
    } = decoded.kind
    else {
        return Err("task execution step changed kind after Serde round-trip".into());
    };
    assert_eq!(decoded_inputs.as_ref(), toolchain);

    let rendered = render_staged_tree(&prepared)?;
    assert!(
        rendered
            .files
            .iter()
            .all(|file| { !file.bytes.contains("selected_test_tools") })
    );
    for (name, result) in direct_render_results(&prepared)? {
        result.map_err(|error| format!("{name} rejected an unselected workflow: {error}"))?;
    }
    Ok(())
}

fn direct_render_results(
    prepared: &GenerationPreparation,
) -> Result<DirectRenderResults, Box<dyn Error>> {
    let ir = expand_workflow(&prepared.workflow.ir, &prepared.config, None)?;
    let policy = prepared.config.workflow.policy;
    let support = prepared.workflow.support.as_ref();
    let context = &prepared.workflow.context;
    let mise = MiseSetup {
        uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    Ok([
        (
            "render_workflow_ir",
            render_workflow_ir(&ir, policy, support, context).map(|_| ()),
        ),
        (
            "render_workflow_ir_strict",
            render_workflow_ir_strict(&ir, policy, support, context, &mise).map(|_| ()),
        ),
        (
            "render_workflow_ir_strict_shared",
            render_workflow_ir_strict_shared(&ir, policy, support, context, &mise).map(|_| ()),
        ),
        (
            "finalize_jobs",
            finalize_jobs(&ir, policy, support, context, &mise).map(|_| ()),
        ),
    ])
}

fn assert_unprepared_refusals(
    results: [(&str, Result<(), RenderError>); 4],
) -> Result<(), Box<dyn Error>> {
    let mut rendered = Vec::new();
    for (name, result) in results {
        match result {
            Err(RenderError::InvalidWorkflow(problem))
                if problem == "selected_test_tool_preparation_unavailable" => {}
            Err(error) => {
                return Err(format!("{name} returned an unrelated error: {error}").into());
            }
            Ok(()) => rendered.push(name),
        }
    }
    if rendered.is_empty() {
        Ok(())
    } else {
        Err(
            format!("renderer paths emitted selected tools before preparation: {rendered:?}")
                .into(),
        )
    }
}

#[test]
fn source_custody_changes_do_not_change_semantic_identity_but_hk_pins_are_owner_local()
-> Result<(), Box<dyn Error>> {
    let repo = configured_repo(&config_with_test_tools("demo = [\"hk\"]\n", None), "nested")?;
    let root = repo.path();
    let first = prepare(root)?;
    let first_inputs = execution_inputs(&first);
    let (owner_id, _, owner_input) = find_task(&first, &first_inputs, "demo", is_test_task)?;
    let (unrelated_id, _, unrelated_input) =
        find_task(&first, &first_inputs, "nested", is_test_task)?;
    let (lint_id, _, lint_input) = find_task(&first, &first_inputs, "demo", |task| {
        task.task_kind == "clippy"
    })?;
    let source_before = selected_hk(owner_input)?.source_bindings.clone();
    let owner_id_before = toolchain_id(owner_input)?;
    let unrelated_id_before = toolchain_id(unrelated_input)?;
    let lint_id_before = toolchain_id(lint_input)?;
    fs::write(
        root.join("mise.toml"),
        format!("{}\n# source custody change\n", mise_config(VERSION)),
    )?;
    let commented = prepare(root)?;
    let commented_inputs = execution_inputs(&commented);
    let (_, _, commented_owner) = find_task(&commented, &commented_inputs, "demo", is_test_task)?;
    assert_ne!(source_before, selected_hk(commented_owner)?.source_bindings);
    assert_eq!(owner_id_before, toolchain_id(commented_owner)?);

    let next_digest = "b".repeat(64);
    let next = prepare_with_pin(root, "2.8.1", &next_digest)?;
    let next_inputs = execution_inputs(&next);
    let (next_owner_id, _, next_owner) = find_task(&next, &next_inputs, "demo", is_test_task)?;
    let (next_unrelated_id, _, next_unrelated) =
        find_task(&next, &next_inputs, "nested", is_test_task)?;
    let (next_lint_id, _, next_lint) = find_task(&next, &next_inputs, "demo", |task| {
        task.task_kind == "clippy"
    })?;
    assert_eq!(owner_id, next_owner_id);
    assert_eq!(unrelated_id, next_unrelated_id);
    assert_eq!(lint_id, next_lint_id);
    assert_ne!(owner_id_before, toolchain_id(next_owner)?);
    assert_eq!(unrelated_id_before, toolchain_id(next_unrelated)?);
    assert_eq!(lint_id_before, toolchain_id(next_lint)?);
    Ok(())
}

#[test]
fn prepare_rejects_bad_hk_pins_lock_rows_and_platforms() -> Result<(), Box<dyn Error>> {
    let config = config_with_test_tools("demo = [\"hk\"]\n", None);
    let invalid_lock_rows = [
        (valid_lock(VERSION, &"a".repeat(64)).replace(
            "backend = \"packslip:github.com/jdx/hk\"",
            "backend = \"core:hk\"",
        ), "hk_lock_backend_mismatch"),
        (valid_lock(VERSION, &"a".repeat(64)).replace(
            "specifiers = [\"2.8.0\"]",
            "specifiers = [\"latest\"]",
        ), "hk_lock_specifier_mismatch"),
        (valid_lock(VERSION, &"a".repeat(64)).replace(
            "specifiers = [\"2.8.0\"]",
            "specifiers = [\"2.8.0\"]\noptions = { archive = \"zip\" }",
        ), "hk_lock_options_unsupported"),
        (valid_lock(VERSION, &"a".repeat(64)).replace(
            "https://github.com/jdx/hk/releases/download/v2.8.0/hk-x86_64-unknown-linux-gnu.tar.gz",
            "https://example.invalid/hk.tar.gz",
        ), "hk_lock_url_mismatch"),
        (valid_lock(VERSION, &"a".repeat(64)).replace(SIGNER, "unreviewed-signer"), "hk_lock_signer_mismatch"),
        (valid_lock(VERSION, &"a".repeat(64)).replace(
            &format!("checksum = \"sha256:{}\"", "a".repeat(64)),
            "checksum = \"sha256:bad\"",
        ), "hk_lock_checksum_invalid"),
        (valid_lock(VERSION, &"a".repeat(64)).replace(
            "[tools.hk.\"platforms.linux-x64\"]",
            "[tools.hk.\"platforms.linux-x64\"]\nprovenance = \"unreviewed\"",
        ), "hk_lock_url_mismatch"),
        (
            format!(
                "{}\n[tools.hk.\"platforms.linux-x64\".repository_ids]\nrepository = \"other/hk\"\nowner = \"other\"\n",
                valid_lock(VERSION, &"a".repeat(64))
            ),
            "hk_lock_repository_mismatch",
        ),
        (valid_lock(VERSION, &"a".repeat(64)).replace(
            "platforms.linux-x64",
            "platforms.linux-x64-musl",
        ), "hk_lock_platform_missing"),
        (format!("root_extra = true\n{}", valid_lock(VERSION, &"a".repeat(64))), "hk_lock_root_shape"),
        (format!(
            "{}\n[[tools.hk]]\nversion = \"{VERSION}\"\nbackend = \"packslip:github.com/jdx/hk\"\nspecifiers = [\"{VERSION}\"]\n",
            valid_lock(VERSION, &"a".repeat(64))
        ), "hk_lock_tool_ambiguous"),
    ];
    for (lock, expected_problem) in invalid_lock_rows {
        let repo = configured_repo_with_lock(&config, &mise_config(VERSION), &lock, "nested")?;
        expect_config_problem(prepare(repo.path()), expected_problem)?;
    }

    let bad_pin = configured_repo_with_lock(
        &config,
        &mise_config("02.8.0"),
        &valid_lock("02.8.0", &"a".repeat(64)),
        "nested",
    )?;
    expect_config_problem(prepare(bad_pin.path()), "hk_version_not_exact")?;

    let bad_selector = configured_repo_with_lock(
        &config,
        "[tools]\nhk = { version = \"2.8.0\", matching_regex = \".*\" }\n",
        &valid_lock(VERSION, &"a".repeat(64)),
        "nested",
    )?;
    expect_config_problem(prepare(bad_selector.path()), "unsupported_hk_selector")?;

    let absent_pin = configured_repo_with_lock(
        &config,
        "[tools]\nrust = \"1.99.1\"\n",
        &valid_lock(VERSION, &"a".repeat(64)),
        "nested",
    )?;
    expect_config_problem(prepare(absent_pin.path()), "hk_project_pin_missing")?;

    Ok(())
}

#[test]
fn prepare_rejects_unknown_ambiguous_and_testless_package_declarations()
-> Result<(), Box<dyn Error>> {
    let unknown = configured_repo(
        &config_with_test_tools("missing = [\"hk\"]\n", None),
        "nested",
    )?;
    expect_config_problem(prepare(unknown.path()), "unknown_rust_package")?;

    let ambiguous = configured_repo(&config_with_test_tools("demo = [\"hk\"]\n", None), "demo")?;
    expect_config_problem(prepare(ambiguous.path()), "ambiguous_rust_package")?;

    let no_test = configured_repo(&config_with_test_tools("demo = [\"hk\"]\n", None), "nested")?;
    fs::write(
        no_test.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\nautotests = false\n\n[lib]\ntest = false\ndoctest = false\n",
    )?;
    expect_config_problem(prepare(no_test.path()), "no_runnable_test_obligation")?;

    for entries in [
        "demo = []\n",
        "demo = [\"hk\", \"hk\"]\n",
        "demo = [\"unknown\"]\n",
    ] {
        let repo =
            workflow_configuration_fixture::make_repo(&config_with_test_tools(entries, None))?;
        assert!(prepare(repo.path()).is_err(), "accepted {entries:?}");
    }
    Ok(())
}

#[test]
fn prepare_rejects_conflicting_source_pins_and_ambiguous_or_missing_locks()
-> Result<(), Box<dyn Error>> {
    let config = config_with_test_tools("demo = [\"hk\"]\n", None);
    let conflicting = configured_repo_with_lock(
        &config,
        &mise_config(VERSION),
        &valid_lock(VERSION, &"a".repeat(64)),
        "nested",
    )?;
    fs::write(conflicting.path().join(".mise.toml"), mise_config("2.8.1"))?;
    expect_config_problem(
        prepare(conflicting.path()),
        "conflicting_project_hk_versions",
    )?;

    let duplicate_locks = configured_repo(&config, "nested")?;
    fs::write(
        duplicate_locks.path().join(".mise.lock"),
        valid_lock(VERSION, &"a".repeat(64)),
    )?;
    expect_config_problem(
        prepare(duplicate_locks.path()),
        "ambiguous_project_mise_lock",
    )?;

    let missing_lock = workflow_configuration_fixture::make_repo(&config)?;
    fs::write(missing_lock.path().join("mise.toml"), mise_config(VERSION))?;
    expect_config_problem(prepare(missing_lock.path()), "hk_lock_missing")?;
    Ok(())
}
