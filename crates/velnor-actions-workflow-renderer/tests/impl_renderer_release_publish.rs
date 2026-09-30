//! Release publish gates: OIDC purity, config binding, bootstrap token.
use crate::impl_renderer_release_gates::{
    ENV, REPO, SHA, binding, bootstrap, checkout_step, gated_spec, invalid, job, job_steps,
    publish_argv, shell, spec, with_steps,
};
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind};
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_jobs::{ReleaseJobSpec, ReleaseRole};
use velnor_actions_workflow_renderer::release_spec::publish_gate_condition;
use velnor_actions_workflow_renderer::release_tree::{
    RELEASE_BOOTSTRAP_CONFIG_PATH, RELEASE_CONFIG_PATH,
};

#[test]
fn oidc_publish_carries_zero_token_material() {
    assert!(check_release_jobs(&gated_spec(), &binding()).is_ok());
    let leaked = with_steps(
        gated_spec(),
        "release-publish",
        vec![
            checkout_step(SHA, Some("false")),
            shell(
                "Publish",
                &publish_argv(RELEASE_CONFIG_PATH),
                &[("TOKEN", "${{ secrets.X }}")],
            ),
        ],
    );
    assert!(
        invalid(check_release_jobs(&leaked, &binding())).starts_with("secret_outside_bootstrap:")
    );
    let tokened = with_steps(
        gated_spec(),
        "release-publish",
        vec![
            checkout_step(SHA, Some("false")),
            shell(
                "Publish",
                &publish_argv(RELEASE_CONFIG_PATH),
                &[("CARGO_REGISTRY_TOKEN", "x")],
            ),
        ],
    );
    assert!(
        invalid(check_release_jobs(&tokened, &binding()))
            .starts_with("registry_token_outside_bootstrap:")
    );
}

#[test]
fn publish_binds_the_exact_generated_config() {
    let unbound = with_steps(
        gated_spec(),
        "release-publish",
        vec![
            checkout_step(SHA, Some("false")),
            shell("Publish", &["release-plz", "release"], &[]),
        ],
    );
    assert_eq!(
        invalid(check_release_jobs(&unbound, &binding())),
        "missing_config_binding:release-publish"
    );
    let swapped = with_steps(
        gated_spec(),
        "release-publish",
        vec![
            checkout_step(SHA, Some("false")),
            shell("Publish", &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH), &[]),
        ],
    );
    assert_eq!(
        invalid(check_release_jobs(&swapped, &binding())),
        "missing_config_binding:release-publish"
    );
}

#[test]
fn bootstrap_binds_exactly_one_env_token() {
    let gate = publish_gate_condition(REPO, &bootstrap());
    let token_job = job(
        ReleaseRole::PublishBootstrap,
        vec![
            checkout_step(SHA, Some("false")),
            shell(
                "Publish",
                &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH),
                &[("CARGO_REGISTRY_TOKEN", "${{ secrets.BOOTSTRAP_TOKEN }}")],
            ),
        ],
        Some(&gate),
        Some(ENV),
    );
    let mut jobs: BTreeMap<String, ReleaseJobSpec> = gated_spec().jobs.clone();
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
    );
    let mut leaked = jobs.clone();
    leaked.get_mut("release-bootstrap").expect("job").steps =
        vec![checkout_step(SHA, Some("false")), argv_secret];
    assert!(invalid(check_release_jobs(&spec(leaked), &binding())).starts_with("secret_in_argv:"));
    let mut unbound = jobs.clone();
    unbound.get_mut("release-bootstrap").expect("job").steps = vec![
        checkout_step(SHA, Some("false")),
        shell("Publish", &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH), &[]),
    ];
    assert!(
        invalid(check_release_jobs(&spec(unbound), &binding()))
            .starts_with("bootstrap_token_binding:")
    );
    let mut wrong_key = jobs;
    wrong_key.get_mut("release-bootstrap").expect("job").steps = vec![
        checkout_step(SHA, Some("false")),
        shell(
            "Publish",
            &publish_argv(RELEASE_BOOTSTRAP_CONFIG_PATH),
            &[("TOKEN", "${{ secrets.BOOTSTRAP_TOKEN }}")],
        ),
    ];
    assert!(
        invalid(check_release_jobs(&spec(wrong_key), &binding()))
            .starts_with("bootstrap_token_binding:")
    );
}

#[test]
fn verification_bypasses_are_rejected_on_publish_argv() {
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
            gated_spec(),
            "release-publish",
            vec![
                checkout_step(SHA, Some("false")),
                shell("Publish", &argv, &[]),
            ],
        );
        assert_eq!(
            invalid(check_release_jobs(&dirty, &binding())),
            "verify_bypass:release-publish",
            "for {flag}"
        );
    }
}

#[test]
fn dispatch_inputs_stay_out_of_publish_steps() {
    let interpolated = with_steps(
        gated_spec(),
        "release-publish",
        vec![
            checkout_step(SHA, Some("false")),
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
            ),
        ],
    );
    assert!(
        invalid(check_release_jobs(&interpolated, &binding()))
            .starts_with("dispatch_input_in_steps:")
    );
}

#[test]
fn reconcile_is_credential_free_and_internal_ops_rejected() {
    let leaked = with_steps(
        gated_spec(),
        "release-reconcile",
        vec![shell("Run", &["echo", "${{ secrets.TOKEN }}"], &[])],
    );
    assert!(
        invalid(check_release_jobs(&leaked, &binding())).starts_with("secret_outside_bootstrap:")
    );
    let internal = Step {
        name: "Plan".to_owned(),
        kind: StepKind::Internal {
            operation: "plan-v1".to_owned(),
        },
    };
    let mut steps = job_steps(&gated_spec(), "release-preflight");
    steps.push(internal);
    let injected = with_steps(gated_spec(), "release-preflight", steps);
    assert!(invalid(check_release_jobs(&injected, &binding())).starts_with("release_internal_op:"));
}
