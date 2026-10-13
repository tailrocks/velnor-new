//! MBX cache keys follow one immutable job/component identity.

use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
use velnor_actions_contract::{Job, StepKind};
use velnor_actions_workflow_renderer::render::render_workflow_ir_strict_shared;
use velnor_actions_workflow_renderer::{RenderError, render_workflow_ir};

use super::impl_renderer_fixtures::*;

#[test]
fn independent_jobs_keep_distinct_mbx_cache_identities() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![
            super::impl_renderer_mbx_bundle::mbx_job("first", "1.21.1")?,
            super::impl_renderer_mbx_bundle::mbx_job("second", "1.21.1")?,
        ]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(text.matches("name: Export MBX single bundle").count(), 2);
    assert_eq!(text.matches("name: Save MBX single bundle").count(), 2);
    assert_eq!(text.matches("name: Import MBX single bundle").count(), 2);
    assert!(text.contains("MBX_CACHE_SCOPE: first"), "{text}");
    assert!(text.contains("MBX_CACHE_SCOPE: second"), "{text}");
    Ok(())
}

#[test]
fn explicit_qualification_scope_has_one_designated_writer() -> Result<(), RenderError> {
    let mut writer = super::impl_renderer_mbx_bundle::mbx_job("mbx-probe-write", "1.21.1")?;
    let mut reader = super::impl_renderer_mbx_bundle::mbx_job("mbx-probe-read", "1.21.1")?;
    set_scope(&mut writer.1, "qualification-mbx-v1/probe-v1", Some(true));
    set_scope(&mut reader.1, "qualification-mbx-v1/probe-v1", Some(false));
    let text = render_workflow_ir(
        &fixture_ir(vec![writer, reader]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(text.matches("name: Export MBX single bundle").count(), 1);
    assert_eq!(text.matches("name: Import MBX single bundle").count(), 2);
    assert_eq!(
        text.matches("MBX_CACHE_SCOPE: qualification-mbx-v1/probe-v1")
            .count(),
        2
    );
    assert!(!text.contains("velnor-cache-scope"));
    assert!(!text.contains("velnor-cache-writer"));
    Ok(())
}

#[test]
fn qualification_scope_without_explicit_roles_fails_closed() -> Result<(), RenderError> {
    let mut first = super::impl_renderer_mbx_bundle::mbx_job("probe-first", "1.21.1")?;
    let mut second = super::impl_renderer_mbx_bundle::mbx_job("probe-second", "1.21.1")?;
    set_scope(&mut first.1, "qualification-mbx-v1/probe-v1", None);
    set_scope(&mut second.1, "qualification-mbx-v1/probe-v1", None);
    let result = render_workflow_ir(
        &fixture_ir(vec![first, second]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    );
    assert!(result.is_err_and(|error| {
        format!("{error:?}").contains("qualification_scope_requires_explicit_role")
    }));
    Ok(())
}

#[test]
fn shared_scope_requires_one_static_cache_identity() -> Result<(), RenderError> {
    let mut writer = super::impl_renderer_mbx_bundle::mbx_job("probe-write", "1.21.1")?;
    let mut reader = super::impl_renderer_mbx_bundle::mbx_job("probe-read", "1.21.1")?;
    set_scope(&mut writer.1, "qualification-mbx-v1/probe-v1", Some(true));
    set_scope(&mut reader.1, "qualification-mbx-v1/probe-v1", Some(false));
    let Some(step) = reader.1.steps.iter_mut().find(|step| match &step.kind {
        StepKind::Shell { env, .. } => env.contains_key("RUSTUP_TOOLCHAIN"),
        StepKind::Action { .. } | StepKind::Internal { .. } => false,
    }) else {
        return Err(RenderError::InvalidWorkflow(
            "missing_pinned_rust_step".to_owned(),
        ));
    };
    let StepKind::Shell { env, .. } = &mut step.kind else {
        return Err(RenderError::InvalidWorkflow(
            "bad_pinned_rust_step".to_owned(),
        ));
    };
    env.insert("RUSTUP_TOOLCHAIN".to_owned(), "1.99.0".to_owned());
    let result = render_workflow_ir(
        &fixture_ir(vec![writer, reader]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    );
    assert!(
        result
            .is_err_and(|error| { format!("{error:?}").contains("incompatible_shared_identity") })
    );
    Ok(())
}

#[test]
fn explicit_shared_scope_is_limited_to_qualification_namespace() -> Result<(), RenderError> {
    let mut built = super::impl_renderer_mbx_bundle::mbx_job("demo", "1.21.1")?;
    set_scope(&mut built.1, "production-shared-cache", Some(true));
    let result = render_workflow_ir(
        &fixture_ir(vec![built]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    );
    assert!(result.is_err_and(|error| {
        format!("{error:?}").contains("shared_scope_outside_qualification_namespace")
    }));
    Ok(())
}

#[test]
fn unsupported_action_cache_inputs_fail_closed() -> Result<(), RenderError> {
    for input in ["isolate-objects-cache", "cache-key-suffix"] {
        let mut built = super::impl_renderer_mbx_bundle::mbx_job("demo", "1.21.1")?;
        let Some(step) = built.1.steps.iter_mut().find(|step| match &step.kind {
            StepKind::Action { uses, .. } => uses.contains("mr-boxington-action@"),
            StepKind::Shell { .. } | StepKind::Internal { .. } => false,
        }) else {
            return Err(RenderError::InvalidWorkflow("missing_mbx_step".to_owned()));
        };
        let StepKind::Action { with, .. } = &mut step.kind else {
            return Err(RenderError::InvalidWorkflow("bad_mbx_step".to_owned()));
        };
        with.insert(input.to_owned(), "true".to_owned());
        let result = render_workflow_ir(
            &fixture_ir(vec![built]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        );
        assert!(
            result.is_err_and(|error| { format!("{error:?}").contains("unsupported_mbx_input") })
        );
    }
    Ok(())
}

#[test]
fn hosted_and_scale_set_copies_elect_one_cache_writer() -> Result<(), RenderError> {
    let hosted = super::impl_renderer_mbx_bundle::mbx_job("rust-demo__hosted", "1.21.1")?;
    let mut local = super::impl_renderer_mbx_bundle::mbx_job("rust-demo__local", "1.21.1")?;
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .map_err(|_| RenderError::InvalidWorkflow("bad_scale_set".to_owned()))?;
    local.1.runs_on = selector.token();
    let rendered = render_workflow_ir_strict_shared(
        &fixture_ir(vec![hosted, local]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise(),
    )?;
    let text = rendered.yaml;
    assert_eq!(text.matches("name: Export MBX single bundle").count(), 1);
    assert_eq!(text.matches("name: Save MBX single bundle").count(), 1);
    assert_eq!(text.matches("uses: $/.github/actions/rust-demo").count(), 2);
    assert_eq!(text.matches("id: mbx-lane-cache").count(), 2);
    assert!(
        text.contains("steps.mbx-lane-cache.outputs.mbx-cache-hit != 'true'"),
        "{text}"
    );
    assert!(
        text.contains("${{ steps.mbx-lane-cache.outputs.mbx-cache-key }}"),
        "{text}"
    );
    assert!(
        !text.contains("steps.mbx-bundle.outputs.cache-hit"),
        "{text}"
    );
    assert!(
        !text.contains("steps.mbx-bundle-key.outputs.primary"),
        "{text}"
    );
    let composite = &rendered.shared[0].bytes;
    assert!(composite.contains("outputs:"), "{composite}");
    assert!(
        composite.contains("value: ${{ steps.mbx-bundle.outputs.cache-hit }}"),
        "{composite}"
    );
    assert!(
        composite.contains("value: ${{ steps.mbx-bundle-key.outputs.primary }}"),
        "{composite}"
    );
    assert!(!composite.contains("mbx-pr-cache-allowed:"), "{composite}");
    Ok(())
}

#[test]
fn explicit_shared_scope_rejects_zero_designated_writers() -> Result<(), RenderError> {
    let mut lone_reader = super::impl_renderer_mbx_bundle::mbx_job("probe-reader", "1.21.1")?;
    set_scope(
        &mut lone_reader.1,
        "qualification-mbx-v1/probe-v1",
        Some(false),
    );
    let lone_result = render_workflow_ir(
        &fixture_ir(vec![lone_reader]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    );
    assert!(
        lone_result.is_err_and(|error| { format!("{error:?}").contains("scope_writer_count") })
    );

    let mut first = super::impl_renderer_mbx_bundle::mbx_job("probe-first", "1.21.1")?;
    let mut second = super::impl_renderer_mbx_bundle::mbx_job("probe-second", "1.21.1")?;
    set_scope(&mut first.1, "qualification-mbx-v1/probe-v1", Some(false));
    set_scope(&mut second.1, "qualification-mbx-v1/probe-v1", Some(false));
    let result = render_workflow_ir(
        &fixture_ir(vec![first, second]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    );
    assert!(result.is_err_and(|error| { format!("{error:?}").contains("scope_writer_count") }));
    Ok(())
}

fn set_scope(job: &mut Job, scope: &str, writer: Option<bool>) {
    let Some(step) = job.steps.iter_mut().find(|step| match &step.kind {
        StepKind::Action { uses, .. } => uses.contains("mr-boxington-action@"),
        StepKind::Shell { .. } | StepKind::Internal { .. } => false,
    }) else {
        return;
    };
    let StepKind::Action { with, .. } = &mut step.kind else {
        return;
    };
    with.insert("velnor-cache-scope".to_owned(), scope.to_owned());
    if let Some(writer) = writer {
        with.insert("velnor-cache-writer".to_owned(), writer.to_string());
    }
}
