#[path = "dispatch_cache_boundary_qualification_tests.rs"]
mod qualification_tests;

use super::*;

#[test]
fn generated_dispatch_plan_gates_native_mbx_and_tools_cache_steps() -> Result<(), crate::RenderError>
{
    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    let ir = dispatch_ir(dispatch_plan_job(&staged)?);
    let context = dispatch_render_context(staged);
    let jobs = crate::render::finalize_jobs(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
        &dispatch_mise(),
        &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only(),
    )?;
    let plan = jobs.get(PLAN_JOB_ID).expect("finalized plan job");
    assert_native_mbx_step(plan);
    assert_seed_and_mise_steps(plan)?;
    assert_rendered_dispatch_guard(&ir, &context)?;
    Ok(())
}

#[test]
fn finalization_requires_and_uses_the_selected_routing_policy() -> Result<(), crate::RenderError> {
    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    let mut ir = dispatch_ir(dispatch_plan_job(&staged)?);
    let selector = velnor_actions_contract::ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("valid test selector");
    ir.jobs
        .get_mut(PLAN_JOB_ID)
        .expect("dispatch plan job")
        .runs_on = selector.token();
    let context = dispatch_render_context(staged);
    let mise = dispatch_mise();
    let hosted = crate::workflow_routing::WorkflowRoutingPolicy::hosted_only();
    let error = crate::render::finalize_jobs(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
        &mise,
        &hosted,
    )
    .expect_err("hosted policy rejects selector leaves");
    assert!(
        error
            .to_string()
            .contains("scale_set_selector_unconfigured"),
        "{error}"
    );

    let image = velnor_actions_contract::config::ValidatedImageReference::parse(
        "ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect("valid test image");
    let scale_set = crate::workflow_routing::WorkflowRoutingPolicy::configured(
        velnor_actions_contract::ExecutionMode::Both,
        selector,
        Some(image),
    );
    assert!(
        crate::render::finalize_jobs(
            &ir,
            WorkflowPolicy::ConsumerV1,
            None,
            &context,
            &mise,
            &scale_set,
        )
        .is_ok()
    );
    Ok(())
}

#[test]
fn required_gate_skips_cache_setup_but_keeps_scale_set_image() -> Result<(), crate::RenderError> {
    let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
    let selector = scale_set_selector();
    let mut ir = dispatch_ir(dispatch_plan_job(&staged)?);
    ir.permissions.actions = velnor_actions_contract::PermissionLevel::Read;
    ir.jobs.get_mut(PLAN_JOB_ID).expect("plan job").runs_on = selector.token();
    ir.jobs
        .insert(crate::render::FINAL_JOB_ID.to_owned(), required_verdict());
    let mut context = dispatch_render_context(staged);
    context.scale_set_selector = Some(selector.clone());
    context.scale_set_image_source_sha = Some("b".repeat(40));
    context.require_verified_cache_helper = true;
    context.verified_cache_helpers.insert(
        selector.token(),
        vec![acquire_step(&context.staged_binary)?],
    );
    let image = velnor_actions_contract::config::ValidatedImageReference::parse(
        "ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    )
    .expect("valid immutable image");
    let routing = crate::workflow_routing::WorkflowRoutingPolicy::configured(
        velnor_actions_contract::ExecutionMode::ScaleSet,
        selector.clone(),
        Some(image),
    );

    let jobs = crate::render::finalize_jobs(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
        &dispatch_mise(),
        &routing,
    )?;
    let gate = &jobs[velnor_actions_contract::REQUIRED_GATE_JOB_ID];
    assert_eq!(gate.runs_on, selector.token());
    assert_eq!(gate.steps.len(), 1);
    let StepKind::Shell { run, .. } = &gate.steps[0].kind else {
        panic!("gate step stays a shell step");
    };
    assert_eq!(run.last().map(String::as_str), Some("false"));
    assert!(
        jobs[PLAN_JOB_ID]
            .steps
            .iter()
            .any(|step| { step.role == Some(velnor_actions_contract::StepRole::AcquireVelnor) })
    );
    crate::required_gate::check_required_gate(&jobs)?;
    let rendered = crate::render::render_workflow_ir_strict_shared_with_routing(
        &ir,
        WorkflowPolicy::ConsumerV1,
        None,
        &context,
        &dispatch_mise(),
        &routing,
    )?;
    let yaml = rendered.yaml;
    assert!(yaml.contains("required-gate:"));
    let gate_yaml = yaml
        .split("  required-gate:")
        .nth(1)
        .expect("required gate YAML");
    let gate_end = gate_yaml
        .match_indices("\n  ")
        .find(|(index, _)| gate_yaml.as_bytes().get(index + 3) != Some(&b' '))
        .map_or(gate_yaml.len(), |(index, _)| index);
    let gate_yaml = &gate_yaml[..gate_end];
    assert!(gate_yaml.contains(
        "ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ));
    assert!(!gate_yaml.contains("actions/checkout"));
    assert!(!gate_yaml.contains("mise"));
    Ok(())
}

fn scale_set_selector() -> velnor_actions_contract::ScaleSetSelector {
    velnor_actions_contract::ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("valid Scale Set selector")
}

fn required_verdict() -> Job {
    Job {
        services: Vec::new(),
        display_name: velnor_actions_contract::REQUIRED_VERDICT_DISPLAY_NAME.to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::REQUIRED,
        needs: vec![PLAN_JOB_ID.to_owned()],
        condition: Some(crate::render::FINAL_CONDITION.to_owned()),
        permissions: None,
        cache_mode: None,
        environment: None,
        steps: vec![
            crate::steps::checkout_step(&format!("actions/checkout@{}", "c".repeat(40)))
                .expect("pinned checkout"),
        ],
    }
}

fn assert_native_mbx_step(plan: &Job) {
    let mbx = plan
        .steps
        .iter()
        .find(|step| step.role == Some(velnor_actions_contract::StepRole::MbxCache))
        .expect("MBX action");
    let install_only = match &mbx.kind {
        StepKind::Action { with, .. } => with.get("backend").map(String::as_str),
        _ => None,
    };
    assert_eq!(install_only, Some("local"));
    if let StepKind::Action { with, .. } = &mbx.kind {
        assert!(!with.contains_key("github-cache-mode"));
    }
}

fn assert_seed_and_mise_steps(plan: &Job) -> Result<(), crate::RenderError> {
    // V2 carries the tool seed inside the runtime-identity prelude; the
    // boundary still gates the MBX object step below. The prelude digest is
    // static and content-bound, so dispatch cannot smuggle a foreign seed.
    let prelude = plan
        .steps
        .iter()
        .find(|step| step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity))
        .expect("V2 tools-cache identity prelude");
    let acquired = plan
        .steps
        .iter()
        .position(|step| step.role == Some(velnor_actions_contract::StepRole::AcquireVelnor))
        .expect("verified helper acquisition");
    let prelude_at = plan
        .steps
        .iter()
        .position(|step| step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity))
        .expect("identity prelude index");
    let restore_at = plan
        .steps
        .iter()
        .position(|step| step.role == Some(velnor_actions_contract::StepRole::ToolsCacheRestore))
        .expect("tools-cache restore index");
    assert!(acquired < prelude_at && prelude_at < restore_at);
    assert_eq!(
        prelude.condition.as_deref(),
        Some(crate::cache_p08::TOOLS_CACHE_PLAN_RUN_CONDITION)
    );
    assert!(
        !prelude
            .condition
            .as_deref()
            .unwrap_or_default()
            .contains("needs.plan"),
        "the plan job does not depend on its own output"
    );
    let StepKind::Action { with, .. } = &prelude.kind else {
        return Err(crate::RenderError::InvalidWorkflow(
            "tools_cache_prelude_not_action".to_owned(),
        ));
    };
    let digest = with
        .get(crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit()
                && (byte.is_ascii_digit() || byte.is_ascii_lowercase())),
        "prelude binds the exact static runtime digest"
    );
    let restore = plan
        .steps
        .iter()
        .find(|step| step.role == Some(velnor_actions_contract::StepRole::ToolsCacheRestore))
        .expect("V2 tools-cache restore");
    assert_eq!(
        restore.condition.as_deref(),
        Some(crate::cache_p08::TOOLS_CACHE_PLAN_RESTORE_CONDITION)
    );
    assert!(
        plan.steps
            .iter()
            .all(|step| !step.name.contains("MBX bundle")),
        "the retired manual transport is absent"
    );
    let mise = plan
        .steps
        .iter()
        .find(|step| step.name == crate::setup::SETUP_MISE_NAME)
        .expect("Mise setup");
    let StepKind::Action { with, .. } = &mise.kind else {
        return Err(crate::RenderError::InvalidWorkflow(
            "setup_mise_not_action".to_owned(),
        ));
    };
    assert_eq!(with.get("cache").map(String::as_str), Some("false"));
    Ok(())
}

fn assert_rendered_dispatch_guard(
    ir: &WorkflowIr,
    context: &RenderContext,
) -> Result<(), crate::RenderError> {
    let rendered = crate::render::render_workflow_ir_strict_shared(
        ir,
        WorkflowPolicy::ConsumerV1,
        None,
        context,
        &dispatch_mise(),
    )?;
    assert!(
        rendered
            .yaml
            .contains(crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME)
    );
    assert!(
        rendered
            .yaml
            .contains("github.event_name != 'workflow_dispatch'")
    );
    Ok(())
}

#[test]
fn empty_seed_key_exits_before_the_trusted_image_tree_is_inspected() {
    let script = crate::tool_seed::tool_seed_action_script(crate::tool_seed::SEED_ROOT)
        .expect("fixed seed script");
    let disabled = script.find("if [ -z \"$key\" ]").expect("empty key gate");
    let trust = script
        .find("trusted_seed_is_trusted")
        .expect("trusted tree check");
    assert!(disabled < trust, "empty key must exit before seed access");
}

#[path = "dispatch_cache_boundary_seed_tests.rs"]
mod seed_tests;
