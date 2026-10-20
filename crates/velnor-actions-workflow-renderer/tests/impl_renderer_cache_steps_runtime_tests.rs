use super::*;

fn assert_save_gate_and_seed(
    rendered: &velnor_actions_workflow_renderer::render::RenderedWorkflow,
    text: &str,
) -> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::RenderError;
    let save_at = text
        .find("- name: Save Mise tools")
        .ok_or_else(|| RenderError::InvalidWorkflow("The save step is missing.".to_owned()))?;
    let save_step = text[save_at..]
        .split("      - name:")
        .next()
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("The end of the save step is missing.".to_owned())
        })?;
    for gate in [
        "github.event_name == 'push'",
        "github.ref_protected == true",
        "steps.v2.outputs.enabled == 'true'",
    ] {
        if !save_step.contains(gate) {
            return Err(RenderError::InvalidWorkflow(format!(
                "save requires protected-push gate {gate}:\n{save_step}",
            )));
        }
    }
    let seed = rendered
        .shared
        .iter()
        .find(|file| file.path == ".github/actions/velnor-tool-seed/action.yml")
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("The renderer must add the seed action.".to_owned())
        })?;
    if !seed.bytes.contains("velnor-host-seed-v1") {
        return Err(RenderError::InvalidWorkflow(format!(
            "The seed action must state its source:\n{}",
            seed.bytes
        )));
    }
    if !text.contains("${{steps.v2.outputs.identity}}") {
        return Err(RenderError::InvalidWorkflow(
            "The V2 identity output is missing.".to_owned(),
        ));
    }
    Ok(())
}

fn assert_runtime_cache_step_order(
    names: &[String],
    text: &str,
) -> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::RenderError;

    assert_eq!(names.iter().filter(|s| *s == TOOLS_RESTORE_NAME).count(), 1);
    assert_eq!(names.iter().filter(|s| *s == TOOLS_SAVE_NAME).count(), 1);
    assert_eq!(
        names.iter().position(|s| s == "Acquire Velnor"),
        Some(1),
        "verified helper precedes cache access: {names:?}"
    );
    assert_eq!(
        names.iter().position(|s| s == "Install cache tar adapters"),
        Some(2),
        "trusted tar adapters follow helper verification: {names:?}"
    );
    assert_eq!(
        names.iter().position(|s| s == "Setup Mise"),
        Some(3),
        "Mise setup follows the verified helper and tar adapter: {names:?}"
    );
    let identity_at = names
        .iter()
        .position(|s| s == "V2 identity")
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("The V2 identity step is missing.".to_owned())
        })?;
    let restore_at = names
        .iter()
        .position(|s| s == TOOLS_RESTORE_NAME)
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("The tools cache restore step is missing.".to_owned())
        })?;
    assert!(
        3 < identity_at && identity_at < restore_at,
        "setup, resolved identity, then restore: {names:?}"
    );
    assert!(!text.contains("name: Restore Velnor tool seed"), "{text}");
    Ok(())
}

fn assert_runtime_cache_step_fields(text: &str) {
    for need in [
        "name: V2 identity",
        "id: v2",
        "outputs.enabled == 'true'",
        "mise-tools-v2-",
        "cache: \"false\"",
        "cache_save: \"false\"",
        "path:",
        "${{ runner.temp }}/velnor/rustup",
        "${{ runner.temp }}/velnor/cargo/.crates.toml",
        "${{ runner.temp }}/velnor/cargo/.crates2.json",
        "${{ runner.temp }}/velnor/cargo/bin",
    ] {
        assert!(
            text.contains(need),
            "The V2 tools cache must include {need}:\n{text}"
        );
    }
    assert!(
        !text.contains("cache_key: mise-v1-"),
        "legacy Mise cache key:\n{text}"
    );
    let setup = text
        .split("      - name: Setup Mise")
        .nth(1)
        .unwrap_or_default();
    let setup = setup.split("      - name:").next().unwrap_or_default();
    assert!(
        !setup.contains("cache_key:"),
        "Mise does not own an archive key"
    );
}

#[test]
fn strict_wires_runtime_qualified_tools_cache_after_setup_and_saves_once()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::checkout_step;
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ],
    );
    let rendered = velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared(
        &fixture_ir(vec![lint]),
        velnor_actions_contract::WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )?;
    let text = &rendered.yaml;
    let names = step_names(text, "actionlint");
    assert_runtime_cache_step_order(&names, text)?;
    assert_runtime_cache_step_fields(text);
    assert_save_gate_and_seed(&rendered, text)?;
    Ok(())
}

#[test]
fn seed_and_tools_prelude_require_the_configured_unconditional_checkout()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::checkout_step;
    let shell_checkout = scrubbed_shell_step("Checkout", vec!["true".to_owned()])?;
    let run = || {
        let job = job(
            "actionlint",
            "Actionlint",
            Vec::new(),
            vec![
                shell_checkout.clone(),
                scrubbed_shell_step(
                    "Run actionlint",
                    mise_argv("actionlint@1.7.12", "actionlint", &[]),
                )?,
            ],
        );
        strict(&fixture_ir(vec![job]), &fixture_ctx())
    };
    let text = run()?;
    assert!(!text.contains("name: V2 identity"), "{text}");
    assert!(!text.contains(TOOLS_RESTORE_NAME), "{text}");
    assert!(!text.contains("Restore Velnor tool seed"), "{text}");

    let mut renamed = checkout_step(&checkout_pin())?;
    renamed.name = "Source checkout".to_owned();
    let real = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            renamed,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &[]),
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![real]), &fixture_ctx())?;
    assert!(
        text.contains("name: V2 identity"),
        "renamed typed checkout: {text}"
    );
    assert!(
        text.contains("uses: ./.github/actions/velnor-tools-prelude-u26"),
        "renamed typed checkout: {text}"
    );

    let mut conditional = checkout_step(&checkout_pin())?;
    conditional.condition = Some("false".to_owned());
    let conditional = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            conditional,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &[]),
            )?,
        ],
    );
    let text = strict(&fixture_ir(vec![conditional]), &fixture_ctx())?;
    assert!(!text.contains("name: V2 identity"), "{text}");
    assert!(!text.contains("Restore Velnor tool seed"), "{text}");
    Ok(())
}

#[test]
fn strict_leaves_setup_less_jobs_without_tools_cache()
-> Result<(), velnor_actions_workflow_renderer::RenderError> {
    use velnor_actions_workflow_renderer::{checkout_step, merge_step, plan_step};
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let mut final_job = job(
        "required",
        "Required verdict",
        vec!["plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    );
    final_job.1.condition =
        Some(velnor_actions_workflow_renderer::render::FINAL_CONDITION.to_owned());
    let text = strict(&fixture_ir(vec![plan, final_job]), &fixture_ctx())?;
    let names = step_names(&text, "required");
    assert!(
        !names
            .iter()
            .any(|s| s == TOOLS_RESTORE_NAME || s == TOOLS_SAVE_NAME),
        "A job without Mise setup must not use a tools cache: {names:?}"
    );
    Ok(())
}
