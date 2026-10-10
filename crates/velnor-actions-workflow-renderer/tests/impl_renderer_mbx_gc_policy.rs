//! Native MBX collection stays off until the guarded post-task cleanup.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};
use velnor_actions_workflow_renderer::steps::{MBX_CACHE_MODE_ENV, checkout_step};
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

/// Rendered MBX policy reaches the action and build payload.
#[test]
fn action_step_env_renders_only_when_present() -> Result<(), RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let [preflight, mbx, version_check] = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let plain = checkout_step(&checkout_pin())?;
    let text = render_workflow_ir(
        &fixture_ir(vec![job(
            "demo",
            "Demo",
            Vec::new(),
            vec![plain, preflight, mbx, version_check],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains("github.ref_protected == true && 'write' || 'read'"),
        "only protected default-branch pushes write:\n{text}"
    );
    assert!(
        text.contains("MBX_GC_AUTO: \"0\""),
        "MBX jobs defer GC while action results may be in flight:\n{text}"
    );
    assert!(text.contains("MBX_SHARE_OUT_DIR: \"0\""), "{text}");
    assert!(text.contains(&format!("{MBX_CACHE_MODE_ENV}:")), "{text}");

    let plain = checkout_step(&checkout_pin())?;
    let cargo_text = render_workflow_ir(
        &fixture_ir(vec![job(
            "cargo-only",
            "Cargo only",
            Vec::new(),
            vec![plain],
        )]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        !cargo_text.contains("MBX_GC_AUTO"),
        "Cargo-only jobs do not receive MBX policy:\n{cargo_text}"
    );
    assert!(
        !cargo_text.contains("jdx/mr-boxington-action")
            && !cargo_text.contains("MBX_SHARE_OUT_DIR"),
        "Cargo-only jobs do not carry native MBX cache policy:\n{cargo_text}"
    );
    Ok(())
}

/// Lane extraction preserves GC policy and final cleanup on both runners.
#[test]
fn mbx_job_policy_applies_to_hosted_and_scale_set_lanes() -> Result<(), RenderError> {
    let text = render_mbx_lane_pair()?;
    assert_eq!(
        text.matches("MBX_GC_AUTO: \"0\"").count(),
        2,
        "both native MBX consumers use the same GC policy: {text}"
    );
    assert_eq!(
        text.matches("MBX_SHARE_OUT_DIR: \"0\"").count(),
        2,
        "{text}"
    );
    assert_eq!(
        text.matches("MBX_CACHE_DIR: ${{ runner.temp }}/velnor/mbx")
            .count(),
        8,
        "preflight, action main/post, version guard, and cleanup use one path per job: {text}"
    );
    assert_eq!(
        text.matches("name: Clean MBX workspace outputs").count(),
        2,
        "each hosted and Scale Set native job cleans after its shared task body: {text}"
    );
    assert_eq!(
        text.matches("if: always() && steps.mbx-ready.outcome == 'success'")
            .count(),
        2,
        "task failures still clean after a successful native setup guard: {text}"
    );
    assert_eq!(
        text.matches("mise --no-config --no-env --no-hooks exec rust@1.98.1 -- mbx clean")
            .count(),
        2,
        "cleanup uses the exact task toolchain in both lanes: {text}"
    );
    assert!(
        !text.contains("isolate-objects-cache"),
        "v1.6 has no isolation input"
    );
    Ok(())
}

#[test]
fn mbx_ready_guard_stays_outside_each_shared_lane_body() -> Result<(), RenderError> {
    let text = render_mbx_lane_pair()?;
    let ready_checks = positions(&text, "id: mbx-ready");
    let lane_calls = positions(&text, "uses: ./.github/actions/rust-demo");
    let cleanups = positions(&text, "name: Clean MBX workspace outputs");
    assert_eq!(ready_checks.len(), 2, "one ready guard per lane: {text}");
    assert_eq!(lane_calls.len(), 2, "one shared task call per lane: {text}");
    assert_eq!(cleanups.len(), 2, "one cleanup per lane: {text}");
    for ((ready, call), cleanup) in ready_checks.iter().zip(&lane_calls).zip(&cleanups) {
        assert!(
            ready < call && call < cleanup,
            "ready → shared tasks → clean: {text}"
        );
    }
    Ok(())
}

#[test]
fn native_mbx_disk_samples_cover_the_active_lifecycle_boundaries() -> Result<(), RenderError> {
    let text = render_mbx_lane_pair()?;
    let pre_restore = positions(&text, "mbx_disk_sample before-action-restore");
    let actions = positions(&text, "uses: jdx/mr-boxington-action@");
    let post_restore = positions(&text, "mbx_disk_sample after-cache-restore");
    let consumers = positions(&text, "uses: ./.github/actions/rust-demo");
    let before_clean = positions(&text, "mbx_disk_sample before-final-clean");
    let clean_calls = positions(&text, "mbx clean");
    let after_clean = positions(&text, "mbx_disk_sample after-final-clean");
    for positions in [
        &pre_restore,
        &actions,
        &post_restore,
        &consumers,
        &before_clean,
        &clean_calls,
        &after_clean,
    ] {
        assert_eq!(
            positions.len(),
            2,
            "one sample boundary per native lane: {text}"
        );
    }
    for ((((((pre_restore, action), post_restore), consumer), before_clean), clean), after_clean) in
        pre_restore
            .iter()
            .zip(&actions)
            .zip(&post_restore)
            .zip(&consumers)
            .zip(&before_clean)
            .zip(&clean_calls)
            .zip(&after_clean)
    {
        assert!(
            pre_restore < action
                && action < post_restore
                && post_restore < consumer
                && consumer < before_clean
                && before_clean < clean
                && clean < after_clean,
            "samples bracket import, consumers, and final cleanup: {text}"
        );
    }
    assert_eq!(text.matches("df -Pk").count(), 6);
    assert_eq!(text.matches("df -Pi").count(), 6);
    assert_eq!(text.matches("du -sk").count(), 12);
    Ok(())
}

fn render_mbx_lane_pair() -> Result<String, RenderError> {
    let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
    let mbx = mbx_tool_steps(&uses, "1.21.1", "1.98.1")?;
    let checkout = checkout_step(&checkout_pin())?;
    let hosted = job(
        &format!("rust-demo{HOSTED_SUFFIX}"),
        "Rust demo hosted",
        Vec::new(),
        vec![
            checkout.clone(),
            mbx[0].clone(),
            mbx[1].clone(),
            mbx[2].clone(),
        ],
    );
    let mut local = job(
        &format!("rust-demo{SCALE_SUFFIX}"),
        "Rust demo scale set",
        Vec::new(),
        vec![checkout, mbx[0].clone(), mbx[1].clone(), mbx[2].clone()],
    );
    local.1.runs_on = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?
    .token();
    let text = render_workflow_ir(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    Ok(text)
}

fn positions(text: &str, needle: &str) -> Vec<usize> {
    text.match_indices(needle).map(|(at, _)| at).collect()
}

#[test]
fn public_renderer_rejects_missing_duplicate_and_misordered_mbx_ready_checks() {
    let uses = format!("jdx/mr-boxington-action@{}", "b".repeat(40));
    let valid = mbx_tool_steps(&uses, "1.21.1", "1.98.1")
        .expect("native MBX steps render")
        .to_vec();

    let mut missing = valid.clone();
    missing.remove(2);
    assert!(
        render_native_steps(missing)
            .expect_err("missing same-job ready check is rejected")
            .to_string()
            .contains("mbx_ready_check_missing"),
    );

    let mut duplicate = valid.clone();
    duplicate.push(valid[2].clone());
    assert!(
        render_native_steps(duplicate)
            .expect_err("duplicate same-job ready IDs are rejected")
            .to_string()
            .contains("duplicate_step_id"),
    );

    let mut misordered = valid.clone();
    misordered.swap(1, 2);
    assert!(
        render_native_steps(misordered)
            .expect_err("ready check before action setup is rejected")
            .to_string()
            .contains("mbx_ready_check_order"),
    );

    let mut noncanonical = valid.clone();
    let velnor_actions_contract::StepKind::Shell { run, .. } = &mut noncanonical[2].kind else {
        panic!("ready check is a shell step");
    };
    run[0] = "true".to_owned();
    assert!(
        render_native_steps(noncanonical)
            .expect_err("noncanonical ready script is rejected")
            .to_string()
            .contains("mbx_ready_check_mismatch"),
    );

    let mut consumer_before_ready = valid;
    consumer_before_ready.insert(
        2,
        velnor_actions_workflow_renderer::shell_step(
            "MBX consumer before ready",
            vec!["mbx".to_owned(), "test".to_owned()],
            std::collections::BTreeMap::new(),
        )
        .expect("MBX consumer step builds"),
    );
    assert!(
        render_native_steps(consumer_before_ready)
            .expect_err("MBX consumer before ready check is rejected")
            .to_string()
            .contains("mbx_consumer_before_ready"),
    );
}

fn render_native_steps(steps: Vec<velnor_actions_contract::Step>) -> Result<String, RenderError> {
    render_workflow_ir(
        &fixture_ir(vec![job("mbx-job", "Native MBX", Vec::new(), steps)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )
}
