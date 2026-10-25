//! Setup-prefix admission regressions for emitted workflows.

use super::{JobText, StepText, setup_is_early};

#[test]
fn required_standard_checkout_prefixes_remain_valid() {
    for prefix in [
        &["Checkout"][..],
        &[
            "Checkout",
            "V2 identity",
            velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_NAME,
        ][..],
    ] {
        let steps = prefix
            .iter()
            .chain(std::iter::once(&"Setup Mise"))
            .map(|name| StepText {
                name: (*name).to_owned(),
                body: String::new(),
            })
            .collect::<Vec<_>>();
        let job = JobText {
            id: "required".to_owned(),
            steps,
        };
        assert!(setup_is_early(&job, prefix.len()));
    }
}

#[test]
fn setup_prefix_accepts_only_one_reserved_cache_clock() {
    let mut job = JobText {
        id: "test".to_owned(),
        steps: vec![
            StepText {
                name: "Record optional cache budget".to_owned(),
                body: "id: \"velnor-cache-clock\"".to_owned(),
            },
            StepText {
                name: "Checkout".to_owned(),
                body: String::new(),
            },
            StepText {
                name: "Setup Mise".to_owned(),
                body: String::new(),
            },
        ],
    };
    assert!(setup_is_early(&job, 2));
    job.steps[0].body = "id: unknown-clock".to_owned();
    assert!(!setup_is_early(&job, 2));
    job.steps[0].body = "id: velnor-cache-clock".to_owned();
    job.steps.insert(
        1,
        StepText {
            name: "Record optional cache budget".to_owned(),
            body: "id: velnor-cache-clock".to_owned(),
        },
    );
    assert!(!setup_is_early(&job, 3));
}

#[test]
fn required_setup_accepts_only_exact_provisioning_prefixes() {
    let step = |name: &str| StepText {
        name: name.to_owned(),
        body: String::new(),
    };
    let mut locked = JobText {
        id: "required".to_owned(),
        steps: vec![step("Acquire Velnor"), step("Checkout"), step("Setup Mise")],
    };
    assert!(setup_is_early(&locked, 2));
    let wrong_job = JobText {
        id: "plan".to_owned(),
        steps: locked.steps.clone(),
    };
    assert!(
        !setup_is_early(&wrong_job, 2),
        "Required prefix is job-scoped"
    );

    let preseed_names = [
        "Download helper (pre-seed trust-on-review)",
        "Verify helper manifest (pre-seed trust-on-review)",
        "Stage helper (pre-seed trust-on-review)",
        "Checkout",
        "Setup Mise",
    ];
    let preseed = JobText {
        id: "required".to_owned(),
        steps: preseed_names.iter().map(|name| step(name)).collect(),
    };
    assert!(setup_is_early(&preseed, 4));
    let missing_verify = JobText {
        id: "required".to_owned(),
        steps: [
            preseed_names[0],
            preseed_names[2],
            preseed_names[3],
            preseed_names[4],
        ]
        .iter()
        .map(|name| step(name))
        .collect(),
    };
    assert!(
        !setup_is_early(&missing_verify, 3),
        "manifest verification is required"
    );

    locked.steps.swap(0, 1);
    assert!(
        !setup_is_early(&locked, 2),
        "reordered checkout is rejected"
    );
    locked.steps = vec![step("Unexpected bootstrap"), step("Setup Mise")];
    assert!(!setup_is_early(&locked, 1), "arbitrary prefix is rejected");
    locked.steps = vec![
        StepText {
            name: "Record optional cache budget".to_owned(),
            body: "id: velnor-cache-clock".to_owned(),
        },
        step("Acquire Velnor"),
        step("Checkout"),
        step("Setup Mise"),
    ];
    assert!(setup_is_early(&locked, 3));
    locked.steps.insert(
        2,
        StepText {
            name: "Record optional cache budget".to_owned(),
            body: "id: velnor-cache-clock".to_owned(),
        },
    );
    assert!(!setup_is_early(&locked, 4), "duplicate clock is rejected");
}
