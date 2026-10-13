//! Per-crate MSRV: rust-version tool, --locked, PR exclusion.
use std::collections::BTreeMap;
use velnor_actions_workflow_renderer::msrv::{MsrvSpec, msrv_job, msrv_step};
use velnor_actions_workflow_renderer::{RenderError, checkout_step, plan_step, shell_step};

use super::impl_renderer_fixtures::*;

fn spec() -> MsrvSpec {
    MsrvSpec {
        package: "velnor-actions-contract".to_owned(),
        rust_version: "1.98".to_owned(),
    }
}

fn argv() -> Vec<String> {
    vec![
        "mise".to_owned(),
        "exec".to_owned(),
        "rust@1.98".to_owned(),
        "--".to_owned(),
        "cargo".to_owned(),
        "check".to_owned(),
        "--locked".to_owned(),
    ]
}

#[test]
fn msrv_step_pins_tool_to_rust_version_and_locked() -> Result<(), RenderError> {
    let step = msrv_step(&spec(), argv(), BTreeMap::new())?;
    assert_eq!(step.name, "msrv velnor-actions-contract");
    let mut wrong_tool = argv();
    wrong_tool[2] = "rust@1.99".to_owned();
    assert!(
        msrv_step(&spec(), wrong_tool, BTreeMap::new())
            .is_err_and(|err| format!("{err:?}").contains("msrv_tool_not_rust_version")),
        "wrong MSRV tool version must fail"
    );
    let unlocked: Vec<String> = argv().into_iter().filter(|arg| arg != "--locked").collect();
    assert!(!unlocked.is_empty());
    assert!(
        msrv_step(&spec(), unlocked, BTreeMap::new())
            .is_err_and(|err| format!("{err:?}").contains("msrv_without_locked")),
        "unlocked MSRV must fail"
    );
    for bad in ["", "has space", "has/slash"] {
        let spec = MsrvSpec {
            package: bad.to_owned(),
            rust_version: "1.98".to_owned(),
        };
        assert!(
            msrv_step(&spec, argv(), BTreeMap::new())
                .is_err_and(|err| format!("{err:?}").contains("msrv_bad_package")),
            "package {bad:?} must fail"
        );
    }
    for bad in ["", "1.98.1", "latest", "1.x"] {
        let spec = MsrvSpec {
            package: "pkg".to_owned(),
            rust_version: bad.to_owned(),
        };
        assert!(
            msrv_step(&spec, argv(), BTreeMap::new())
                .is_err_and(|err| format!("{err:?}").contains("msrv_bad_rust_version")),
            "rust-version {bad:?} must fail"
        );
    }
    Ok(())
}

#[test]
fn msrv_job_is_per_crate() -> Result<(), RenderError> {
    let job = msrv_job(LABEL, &checkout_pin(), &spec(), argv(), BTreeMap::new())?;
    assert_eq!(job.display_name, "MSRV velnor-actions-contract");
    assert_eq!(job.runs_on, LABEL);
    assert!(job.needs.is_empty());
    let names: Vec<&str> = job.steps.iter().map(|step| step.name.as_str()).collect();
    assert_eq!(names, ["Checkout", "msrv velnor-actions-contract"]);
    Ok(())
}

#[test]
fn pr_render_rejects_msrv_steps() -> Result<(), RenderError> {
    let leaking = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            shell_step("msrv velnor-actions-contract", argv(), BTreeMap::new())?,
            plan_step(),
        ],
    );
    assert!(
        strict(&fixture_ir(vec![leaking]), &fixture_ctx())
            .is_err_and(|err| format!("{err:?}").contains("msrv_in_pr_workflow")),
        "MSRV step in PR workflow must fail"
    );
    let named = job(
        "velnor-qual",
        "MSRV velnor-actions-contract",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    assert!(
        strict(&fixture_ir(vec![named]), &fixture_ctx())
            .is_err_and(|err| format!("{err:?}").contains("msrv_in_pr_workflow")),
        "MSRV job in PR workflow must fail"
    );
    Ok(())
}
