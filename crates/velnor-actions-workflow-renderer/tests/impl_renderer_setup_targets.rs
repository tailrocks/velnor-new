//! Strict render selects the per-target Mise setup digest for each job.
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_workflow_renderer::setup::MISE_BINARY_SHA256_MACOS_ARM64;
use velnor_actions_workflow_renderer::{RenderError, checkout_step, render_workflow_ir_strict};

use super::impl_renderer_fixtures::*;

#[test]
fn strict_selects_setup_digest_for_each_runner_target() -> Result<(), RenderError> {
    let lint_steps = || -> Result<_, RenderError> {
        Ok(vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run lint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ])
    };
    let linux = job("linux", "Linux", Vec::new(), lint_steps()?);
    let mut macos = job("macos", "macOS", Vec::new(), lint_steps()?);
    macos.1.runs_on = "macos-15".to_owned();
    let text = render_workflow_ir_strict(
        &fixture_ir(vec![linux, macos]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
        &mise_set()?,
    )?;
    assert!(text.contains(&format!("sha256: {MISE_SHA256}")), "{text}");
    assert!(
        text.contains(&format!("sha256: {MISE_BINARY_SHA256_MACOS_ARM64}")),
        "{text}"
    );
    Ok(())
}
