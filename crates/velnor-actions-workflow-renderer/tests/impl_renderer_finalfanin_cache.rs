//! The checkout-less final gate keeps pinned tool setup on a cold path.

use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::{
    RenderError, checkout_step, merge_step, plan_step, shell_step, write_request_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn checkoutless_required_job_does_not_call_local_cache_action() -> Result<(), RenderError> {
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
    let (required_id, mut required) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            shell_step(
                "Prepare pinned tools",
                vec![
                    "mise".to_owned(),
                    "--no-config".to_owned(),
                    "--no-env".to_owned(),
                    "--no-hooks".to_owned(),
                    "install".to_owned(),
                    "gh@2.102.0".to_owned(),
                ],
                BTreeMap::new(),
            )?,
            acquire_fixture()?,
            write_request_step("merge-v1")?,
            merge_step(),
        ],
    );
    required.condition = Some("always()".to_owned());

    let text = strict(
        &fixture_ir(vec![plan, (required_id, required)]),
        &fixture_ctx(),
    )?;
    let names = step_names(&text, "required");
    assert!(names.contains(&"Setup Mise".to_owned()), "{names:?}");
    assert!(
        names.contains(&"Prepare pinned tools".to_owned()),
        "{names:?}"
    );
    assert!(!names.contains(&"V2 identity".to_owned()), "{names:?}");
    assert!(
        !names.contains(&"Restore Mise tools".to_owned()),
        "{names:?}"
    );
    assert!(!names.contains(&"Save Mise tools".to_owned()), "{names:?}");
    assert!(!text.contains("uses: ./.github/actions/u26"), "{text}");
    assert!(
        !text.contains(velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES),
        "checkoutless Required must not call the local restore composite:\n{text}"
    );
    Ok(())
}
