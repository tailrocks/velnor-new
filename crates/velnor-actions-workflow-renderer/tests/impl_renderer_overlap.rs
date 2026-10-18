//! Prep overlap-then-join: concurrent branches, needs join, no step syntax.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::overlap::{PrepOverlap, wire_prep_join};
use velnor_actions_workflow_renderer::{RenderError, checkout_step};

use super::impl_renderer_fixtures::*;

fn shell(name: &str) -> Result<velnor_actions_contract::Step, RenderError> {
    scrubbed_shell_step(
        name,
        vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    )
}

fn branch(
    id: &str,
    display: &str,
    step: &str,
) -> Result<(String, velnor_actions_contract::Job), RenderError> {
    Ok(job(
        id,
        display,
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, shell(step)?],
    ))
}

fn spec() -> PrepOverlap {
    PrepOverlap {
        download_job: "velnor-prep-download".to_owned(),
        image_job: "velnor-prep-image".to_owned(),
        join_job: "velnor-prep-join".to_owned(),
    }
}

#[test]
fn prep_overlap_joins_independent_branches() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        branch(
            "velnor-prep-download",
            "Download verified artifact",
            "Fetch asset",
        )?,
        branch("velnor-prep-image", "Prepare pinned image", "Pull image")?,
        branch("velnor-prep-join", "Consume prepared inputs", "Use inputs")?,
    ]);
    wire_prep_join(&mut jobs, &spec())?;
    assert_eq!(jobs["velnor-prep-download"].needs, [] as [String; 0]);
    assert_eq!(jobs["velnor-prep-image"].needs, [] as [String; 0]);
    assert_eq!(
        jobs["velnor-prep-join"].needs,
        ["velnor-prep-download", "velnor-prep-image"]
    );
    let text = strict(&fixture_ir(jobs.into_iter().collect()), &fixture_ctx())?;
    for token in ["parallel:", "background:"] {
        assert!(
            !text.contains(token),
            "v1 forbids step syntax {token}:\n{text}"
        );
    }
    let join_at = text.find("velnor-prep-join:").expect("join job");
    let window = snip(&text, join_at, 1600);
    assert!(
        window.contains("- velnor-prep-download"),
        "join needs:\n{window}"
    );
    assert!(
        window.contains("- velnor-prep-image"),
        "join needs:\n{window}"
    );
    Ok(())
}

#[test]
fn prep_overlap_rejects_dependent_or_missing() -> Result<(), RenderError> {
    let mut jobs = BTreeMap::from([
        branch(
            "velnor-prep-download",
            "Download verified artifact",
            "Fetch asset",
        )?,
        branch("velnor-prep-image", "Prepare pinned image", "Pull image")?,
        branch("velnor-prep-join", "Consume prepared inputs", "Use inputs")?,
    ]);
    let mut missing = spec();
    missing.image_job = "velnor-absent".to_owned();
    assert!(
        wire_prep_join(&mut jobs, &missing)
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_unknown_job")),
        "absent branch must fail"
    );
    let mut same = spec();
    same.image_job = same.download_job.clone();
    assert!(
        wire_prep_join(&mut jobs, &same)
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_not_distinct")),
        "shared branch must fail"
    );
    jobs.get_mut("velnor-prep-image")
        .expect("image job")
        .needs
        .push("velnor-prep-download".to_owned());
    assert!(
        wire_prep_join(&mut jobs, &spec())
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_dependent")),
        "ordered branches must fail"
    );
    jobs.get_mut("velnor-prep-image")
        .expect("image job")
        .needs
        .clear();
    jobs.get_mut("velnor-prep-download")
        .expect("download job")
        .needs
        .push("velnor-prep-join".to_owned());
    assert!(
        wire_prep_join(&mut jobs, &spec())
            .is_err_and(|err| format!("{err:?}").contains("prep_overlap_cycle")),
        "branch on join must fail"
    );
    Ok(())
}

const QUALIFIED_CAPS: bool = true;
const UNQUALIFIED_CAPS: bool = false;

#[test]
fn native_step_parallelism_fails_closed_for_unqualified_actionlint_or_composite()
-> Result<(), RenderError> {
    use velnor_actions_workflow_renderer::overlap::{
        BackgroundStep, NativeStepItem, ParallelStepGroup, WaitStep, WaitTarget,
        render_parallel_group_yaml, validate_native_step_sequence,
    };

    let group = ParallelStepGroup {
        name: "Independent probes".to_owned(),
        condition: None,
        steps: vec![shell("Probe A")?, shell("Probe B")?],
    };
    let seq = [
        NativeStepItem::Parallel(group.clone()),
        NativeStepItem::Background(BackgroundStep {
            handle_id: "bg_fetch".to_owned(),
            step: shell("Background fetch")?,
        }),
        NativeStepItem::Wait(WaitStep {
            name: "Wait for background fetch".to_owned(),
            condition: None,
            target: WaitTarget::Steps(vec!["bg_fetch".to_owned()]),
        }),
    ];

    assert!(
        validate_native_step_sequence(&seq, false, UNQUALIFIED_CAPS).is_err_and(|err| {
            format!("{err:?}").contains("native_step_parallelism_unqualified_actionlint")
        }),
        "pinned actionlint 1.7.12 must fail closed"
    );
    assert!(
        validate_native_step_sequence(&seq, true, QUALIFIED_CAPS).is_err_and(|err| {
            format!("{err:?}").contains("native_step_parallelism_composite_forbidden")
        }),
        "composite action scope must reject native step parallelism"
    );
    assert!(
        render_parallel_group_yaml("plan", &group, &fixture_ctx(), false, UNQUALIFIED_CAPS)
            .is_err_and(|err| {
                format!("{err:?}").contains("native_step_parallelism_unqualified_actionlint")
            })
    );
    assert!(
        render_parallel_group_yaml("plan", &group, &fixture_ctx(), true, QUALIFIED_CAPS)
            .is_err_and(|err| {
                format!("{err:?}").contains("native_step_parallelism_composite_forbidden")
            })
    );
    validate_native_step_sequence(&seq, false, QUALIFIED_CAPS)?;
    Ok(())
}

#[test]
fn native_step_parallelism_renders_parallel_background_and_wait_yaml() -> Result<(), RenderError> {
    use velnor_actions_workflow_renderer::overlap::{
        BackgroundStep, ParallelStepGroup, WaitStep, WaitTarget, render_background_step_yaml,
        render_parallel_group_yaml, render_wait_step_yaml,
    };
    use velnor_actions_workflow_renderer::render_yaml;

    let ctx = fixture_ctx();
    let group = ParallelStepGroup {
        name: "Parallel prep".to_owned(),
        condition: Some("success()".to_owned()),
        steps: vec![shell("Branch A")?, shell("Branch B")?],
    };
    let group_yaml = render_yaml(&render_parallel_group_yaml(
        "plan",
        &group,
        &ctx,
        false,
        QUALIFIED_CAPS,
    )?);
    assert!(group_yaml.contains("name: Parallel prep\n"));
    assert!(group_yaml.contains("parallel:\n"));
    assert!(group_yaml.contains("- name: Branch A\n"));
    assert!(group_yaml.contains("- name: Branch B\n"));

    let bg = BackgroundStep {
        handle_id: "warm_image".to_owned(),
        step: shell("Warm image")?,
    };
    let bg_yaml = render_yaml(&render_background_step_yaml(
        "plan",
        &bg,
        &ctx,
        false,
        QUALIFIED_CAPS,
    )?);
    assert!(bg_yaml.contains("id: warm_image\n"));
    assert!(bg_yaml.contains("background: true\n"));

    let wait_one = WaitStep {
        name: "Join warm image".to_owned(),
        condition: None,
        target: WaitTarget::Steps(vec!["warm_image".to_owned()]),
    };
    let wait_one_yaml = render_yaml(&render_wait_step_yaml(&wait_one, false, QUALIFIED_CAPS)?);
    assert!(wait_one_yaml.contains("wait: warm_image\n"));

    let wait_all = WaitStep {
        name: "Join all background steps".to_owned(),
        condition: None,
        target: WaitTarget::All,
    };
    let wait_all_yaml = render_yaml(&render_wait_step_yaml(&wait_all, false, QUALIFIED_CAPS)?);
    assert!(wait_all_yaml.contains("wait-all: true\n"));
    Ok(())
}

#[test]
fn native_step_parallelism_enforces_concurrency_and_output_barriers() -> Result<(), RenderError> {
    use velnor_actions_workflow_renderer::overlap::{
        BackgroundStep, MAX_CONCURRENT_BACKGROUND_STEPS, NativeStepItem, WaitStep, WaitTarget,
        validate_native_step_sequence,
    };

    let mut over_limit = Vec::new();
    for idx in 0..=MAX_CONCURRENT_BACKGROUND_STEPS {
        over_limit.push(NativeStepItem::Background(BackgroundStep {
            handle_id: format!("bg_{idx}"),
            step: shell("Background task")?,
        }));
    }
    over_limit.push(NativeStepItem::Wait(WaitStep {
        name: "Join all".to_owned(),
        condition: None,
        target: WaitTarget::All,
    }));
    assert!(
        validate_native_step_sequence(&over_limit, false, QUALIFIED_CAPS).is_err_and(|err| {
            format!("{err:?}").contains("native_step_parallelism_concurrency_limit_exceeded")
        }),
        "more than 10 concurrent background steps must fail closed"
    );

    let mut pre_wait_consumer = shell("Read output early")?;
    pre_wait_consumer.condition = Some("steps.bg_1.outputs.ready == 'true'".to_owned());
    let early_output_seq = [
        NativeStepItem::Background(BackgroundStep {
            handle_id: "bg_1".to_owned(),
            step: shell("Producer")?,
        }),
        NativeStepItem::Sequential(pre_wait_consumer.clone()),
        NativeStepItem::Wait(WaitStep {
            name: "Join producer".to_owned(),
            condition: None,
            target: WaitTarget::Steps(vec!["bg_1".to_owned()]),
        }),
    ];
    assert!(
        validate_native_step_sequence(&early_output_seq, false, QUALIFIED_CAPS).is_err_and(|err| {
            format!("{err:?}").contains("native_step_parallelism_output_before_wait:bg_1")
        }),
        "referencing background step outputs before wait must fail closed"
    );

    let post_wait_seq = [
        NativeStepItem::Background(BackgroundStep {
            handle_id: "bg_1".to_owned(),
            step: shell("Producer")?,
        }),
        NativeStepItem::Wait(WaitStep {
            name: "Join producer".to_owned(),
            condition: None,
            target: WaitTarget::Steps(vec!["bg_1".to_owned()]),
        }),
        NativeStepItem::Sequential(pre_wait_consumer),
    ];
    validate_native_step_sequence(&post_wait_seq, false, QUALIFIED_CAPS)?;
    Ok(())
}

#[test]
fn native_step_parallelism_enforces_cache_save_after_wait() -> Result<(), RenderError> {
    use velnor_actions_contract::{Step, StepKind, StepRole};
    use velnor_actions_workflow_renderer::overlap::{
        BackgroundStep, NativeStepItem, WaitStep, WaitTarget, validate_native_step_sequence,
    };

    let cache_save = Step {
        name: "Save Cargo sources".to_owned(),
        id: None,
        role: Some(StepRole::CargoSourcesSave),
        condition: None,
        kind: StepKind::Action {
            uses: "actions/cache/save@0000000000000000000000000000000000000000".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    };
    let save_before_wait_seq = [
        NativeStepItem::Background(BackgroundStep {
            handle_id: "bg_fetch".to_owned(),
            step: shell("Fetch")?,
        }),
        NativeStepItem::Sequential(cache_save),
        NativeStepItem::Wait(WaitStep {
            name: "Join fetch".to_owned(),
            condition: None,
            target: WaitTarget::Steps(vec!["bg_fetch".to_owned()]),
        }),
    ];
    assert!(
        validate_native_step_sequence(&save_before_wait_seq, false, QUALIFIED_CAPS).is_err_and(
            |err| format!("{err:?}").contains("native_step_parallelism_cache_save_before_wait")
        ),
        "cache save before background wait must fail closed"
    );
    Ok(())
}
