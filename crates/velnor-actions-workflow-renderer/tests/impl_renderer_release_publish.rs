//! Release publish gates: OIDC purity, config binding, bootstrap token.
use crate::impl_renderer_release_gates::{
    ENV, REPO, SHA, binding, bootstrap, checkout_step, gated_spec, invalid, job, job_steps,
    publish_argv, shell, spec, with_steps,
};
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};
use velnor_actions_workflow_renderer::release_spec::publish_gate_condition;
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH,
};

#[test]
fn oidc_publish_carries_zero_token_material() -> Result<(), RenderError> {
    assert!(check_release_jobs(&gated_spec()?, &binding()).is_ok());
    let leaked = with_steps(
        gated_spec()?,
        "release-publish",
        vec![
            checkout_step(SHA, Some("false"))?,
            shell(
                "Publish",
                &publish_argv(RELEASE_CONFIG_PATH),
                &[("TOKEN", "${{ secrets.X }}")],
            )?,
        ],
    )
    .expect("job");
    assert!(
        invalid(check_release_jobs(&leaked, &binding()))
            .expect("reject")
            .starts_with("secret_outside_bootstrap:")
    );
    let tokened = with_steps(
        gated_spec()?,
        "release-publish",
        vec![
            checkout_step(SHA, Some("false"))?,
            shell(
                "Publish",
                &publish_argv(RELEASE_CONFIG_PATH),
                &[("CARGO_REGISTRY_TOKEN", "x")],
            )?,
        ],
    )
    .expect("job");
    assert!(
        invalid(check_release_jobs(&tokened, &binding()))
            .expect("reject")
            .starts_with("registry_token_outside_bootstrap:")
    );
    Ok(())
}

#[test]
fn publish_binds_the_exact_generated_config() -> Result<(), RenderError> {
    let unbound = with_steps(
        gated_spec()?,
        "release-publish",
        vec![
            checkout_step(SHA, Some("false"))?,
            shell("Publish", &["release-plz", "release"], &[])?,
        ],
    )
    .expect("job");
    assert_eq!(
        invalid(check_release_jobs(&unbound, &binding())).expect("reject"),
        "missing_config_binding:release-publish"
    );
    let swapped = with_steps(
        gated_spec()?,
        "release-publish",
        vec![
            checkout_step(SHA, Some("false"))?,
            shell("Publish", &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH), &[])?,
        ],
    )
    .expect("job");
    assert_eq!(
        invalid(check_release_jobs(&swapped, &binding())).expect("reject"),
        "missing_config_binding:release-publish"
    );
    Ok(())
}

#[test]
fn bootstrap_binds_exactly_one_env_token() -> Result<(), RenderError> {
    let gate = publish_gate_condition(REPO, &bootstrap());
    let token_job = job(
        ReleaseRole::PublishBootstrap,
        vec![
            checkout_step(SHA, Some("false"))?,
            shell(
                "Publish",
                &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH),
                &[("CARGO_REGISTRY_TOKEN", "${{ secrets.BOOTSTRAP_TOKEN }}")],
            )?,
        ],
        Some(&gate),
        Some(ENV),
    );
    let mut jobs: BTreeMap<String, ReleaseJobSpec> = gated_spec()?.jobs.clone();
    jobs.insert("release-bootstrap".to_owned(), token_job);
    assert!(check_release_jobs(&spec(jobs.clone()), &binding()).is_ok());
    let argv_secret = shell(
        "Publish",
        &[
            "release-plz",
            "release",
            "--config",
            RELEASE_BOOTSTRAP_CONFIG_PATH,
            "${{ secrets.X }}",
        ],
        &[],
    )?;
    let mut leaked = jobs.clone();
    leaked.get_mut("release-bootstrap").expect("job").steps =
        vec![checkout_step(SHA, Some("false"))?, argv_secret];
    assert!(
        invalid(check_release_jobs(&spec(leaked), &binding()))
            .expect("reject")
            .starts_with("secret_in_argv:")
    );
    let mut unbound = jobs.clone();
    unbound.get_mut("release-bootstrap").expect("job").steps = vec![
        checkout_step(SHA, Some("false"))?,
        shell("Publish", &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH), &[])?,
    ];
    assert!(
        invalid(check_release_jobs(&spec(unbound), &binding()))
            .expect("reject")
            .starts_with("bootstrap_token_binding:")
    );
    let mut wrong_key = jobs;
    wrong_key.get_mut("release-bootstrap").expect("job").steps = vec![
        checkout_step(SHA, Some("false"))?,
        shell(
            "Publish",
            &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH),
            &[("TOKEN", "${{ secrets.BOOTSTRAP_TOKEN }}")],
        )?,
    ];
    assert!(
        invalid(check_release_jobs(&spec(wrong_key), &binding()))
            .expect("reject")
            .starts_with("bootstrap_token_binding:")
    );
    Ok(())
}

#[test]
fn verification_bypasses_are_rejected_on_publish_argv() -> Result<(), RenderError> {
    for flag in [
        "--no-verify",
        "--allow-dirty",
        "--no_verify",
        "--allow_dirty",
    ] {
        let argv = [
            "release-plz",
            "release",
            "--config",
            RELEASE_CONFIG_PATH,
            flag,
        ];
        let dirty = with_steps(
            gated_spec()?,
            "release-publish",
            vec![
                checkout_step(SHA, Some("false"))?,
                shell("Publish", &argv, &[])?,
            ],
        )
        .expect("job");
        assert_eq!(
            invalid(check_release_jobs(&dirty, &binding())).expect("reject"),
            "verify_bypass:release-publish",
            "for {flag}"
        );
    }
    Ok(())
}

#[test]
fn dispatch_inputs_stay_out_of_publish_steps() -> Result<(), RenderError> {
    let interpolated = with_steps(
        gated_spec()?,
        "release-publish",
        vec![
            checkout_step(SHA, Some("false"))?,
            shell(
                "Publish",
                &[
                    "release-plz",
                    "release",
                    "--config",
                    RELEASE_CONFIG_PATH,
                    "inputs.plan",
                ],
                &[],
            )?,
        ],
    )
    .expect("job");
    assert!(
        invalid(check_release_jobs(&interpolated, &binding()))
            .expect("reject")
            .starts_with("dispatch_input_in_steps:")
    );
    Ok(())
}

#[test]
fn reconcile_is_credential_free_and_internal_ops_rejected() -> Result<(), RenderError> {
    let leaked = with_steps(
        gated_spec()?,
        "release-reconcile",
        vec![shell("Run", &["echo", "${{ secrets.TOKEN }}"], &[])?],
    )
    .expect("job");
    assert!(
        invalid(check_release_jobs(&leaked, &binding()))
            .expect("reject")
            .starts_with("secret_outside_bootstrap:")
    );
    let internal = Step {
        name: "Plan".to_owned(),
        kind: StepKind::Internal {
            operation: "plan-v1".to_owned(),
        },
    };
    let mut steps = job_steps(&gated_spec()?, "release-preflight").expect("job");
    steps.push(internal);
    let injected = with_steps(gated_spec()?, "release-preflight", steps).expect("job");
    assert!(
        invalid(check_release_jobs(&injected, &binding()))
            .expect("reject")
            .starts_with("release_internal_op:")
    );
    Ok(())
}
