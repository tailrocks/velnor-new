//! Release checkout-gate cases (policy/source shape, credentials, history).
use crate::impl_renderer_release_gates::{
    FORGE_ENV, SHA, binding, checkout, gated_spec, invalid, policy_checkout, publish_argv, shell,
    source_checkout, with_steps,
};
use velnor_actions_contract_workflow::Step;
use velnor_actions_workflow_renderer::RenderError;
use velnor_actions_workflow_renderer::release_gates::check_release_jobs;
use velnor_actions_workflow_renderer::release_tree::RELEASE_CONFIG_PATH;

#[test]
fn checkout_credentials_follow_role_and_kind() -> Result<(), RenderError> {
    assert!(check_release_jobs(&gated_spec()?, &binding()).is_ok());
    // (job, steps, expected rejection or None when the matrix allows it)
    let matrix: &[(&str, Vec<Step>, Option<&str>)] = &[
        (
            "release-preflight",
            vec![
                policy_checkout(Some("true"))?,
                source_checkout(SHA, Some("false"))?,
            ],
            Some("checkout_with_credentials:release-preflight"),
        ),
        (
            "release-preflight",
            vec![policy_checkout(None)?, source_checkout(SHA, Some("false"))?],
            Some("checkout_with_credentials:release-preflight"),
        ),
        (
            "release-preflight",
            vec![
                policy_checkout(Some("false"))?,
                source_checkout(SHA, Some("true"))?,
            ],
            Some("checkout_with_credentials:release-preflight"),
        ),
        (
            "release-preparation",
            vec![policy_checkout(Some("true"))?],
            None,
        ),
        (
            "release-preparation",
            vec![policy_checkout(Some("false"))?],
            Some("checkout_with_credentials:release-preparation"),
        ),
        (
            "release-publish",
            vec![
                policy_checkout(Some("false"))?,
                source_checkout(SHA, Some("false"))?,
                shell("Publish", &publish_argv(RELEASE_CONFIG_PATH), &FORGE_ENV),
            ],
            Some("checkout_with_credentials:release-publish"),
        ),
        (
            "release-publish",
            vec![
                policy_checkout(Some("true"))?,
                source_checkout(SHA, Some("true"))?,
                shell("Publish", &publish_argv(RELEASE_CONFIG_PATH), &FORGE_ENV),
            ],
            Some("checkout_with_credentials:release-publish"),
        ),
    ];
    for (job, steps, expected) in matrix {
        let spec = with_steps(gated_spec()?, job, steps.clone()).expect("job");
        match expected {
            Some(code) => assert_eq!(
                invalid(check_release_jobs(&spec, &binding())).expect("reject"),
                *code,
                "for {job}"
            ),
            None => assert!(check_release_jobs(&spec, &binding()).is_ok(), "for {job}"),
        }
    }
    Ok(())
}

#[test]
fn policy_checkouts_track_the_event_commit() -> Result<(), RenderError> {
    let pinned = with_steps(
        gated_spec()?,
        "release-preflight",
        vec![
            checkout(&[
                ("persist-credentials", "false"),
                ("fetch-depth", "0"),
                ("ref", SHA),
            ])?,
            source_checkout(SHA, Some("false"))?,
        ],
    )
    .expect("job");
    assert_eq!(
        invalid(check_release_jobs(&pinned, &binding())).expect("reject"),
        "pinned_policy_checkout:release-preflight"
    );
    Ok(())
}

#[test]
fn checkouts_fetch_full_history() -> Result<(), RenderError> {
    for with in [
        vec![("persist-credentials", "false")],
        vec![("persist-credentials", "false"), ("fetch-depth", "1")],
    ] {
        let shallow =
            with_steps(gated_spec()?, "release-preflight", vec![checkout(&with)?]).expect("job");
        assert_eq!(
            invalid(check_release_jobs(&shallow, &binding())).expect("reject"),
            "shallow_checkout:release-preflight",
            "for {with:?}"
        );
    }
    Ok(())
}

#[test]
fn source_checkouts_bind_the_fixed_path() -> Result<(), RenderError> {
    let stray = with_steps(
        gated_spec()?,
        "release-preflight",
        vec![
            policy_checkout(Some("false"))?,
            checkout(&[
                ("persist-credentials", "false"),
                ("fetch-depth", "0"),
                ("path", "elsewhere"),
                ("ref", SHA),
            ])?,
        ],
    )
    .expect("job");
    assert_eq!(
        invalid(check_release_jobs(&stray, &binding())).expect("reject"),
        "unknown_checkout_path:release-preflight"
    );
    Ok(())
}
