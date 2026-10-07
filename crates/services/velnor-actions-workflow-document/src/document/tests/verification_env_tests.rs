//! Verification-job environment keeps repo Mise config visible while scrubbing credentials.

use std::collections::BTreeMap;

use velnor_actions_contract_config::{VerificationRunner, VerificationTask, VerificationTaskKind};
use velnor_actions_contract_workflow::workflow::permissions::PermissionLevel;

use velnor_actions_workflow_jobs::{
    RenderContext, VerificationTaskPolicy, build_verification_task_job,
};
use velnor_actions_workflow_steps::MiseSetup;

use super::super::job_to_yaml;

#[test]
fn report_fetch_requires_the_exact_read_permission() {
    assert!(super::super::grants_exact_actions_read(
        PermissionLevel::Read
    ));
    assert!(!super::super::grants_exact_actions_read(
        PermissionLevel::Write
    ));
    assert!(!super::super::grants_exact_actions_read(
        PermissionLevel::None
    ));
}

#[test]
fn emitted_verification_job_scrubs_credentials_without_disabling_mise_config() {
    let policy = VerificationTaskPolicy {
        task: VerificationTask {
            id: "native-format".to_owned(),
            kind: VerificationTaskKind::Verification,
            mise_task: "desktop-format-check".to_owned(),
            runner: VerificationRunner::MacosArm64,
            timeout_minutes: 10,
        },
        runner_label: "macos-15".to_owned(),
        scale_set_token: None,
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            version: "2026.9.18".to_owned(),
            sha256: "a".repeat(64),
        },
    };
    let id = policy.job_id();
    let job = build_verification_task_job(
        &policy,
        "actions/checkout@0123456789abcdef0123456789abcdef01234567",
    )
    .expect("typed task job");
    let context = RenderContext {
        generator_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
        checkout_uses: "actions/checkout@0123456789abcdef0123456789abcdef01234567".to_owned(),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        verification_tasks: vec![policy],
        plan_consumer_env: BTreeMap::new(),
    };
    let checkouts = BTreeMap::new();
    let steps = BTreeMap::new();
    let lanes = crate::document_lanes::SharedLaneSteps {
        checkouts: &checkouts,
        env_steps: &steps,
        prefixes: &steps,
        preludes: &steps,
        postludes: &steps,
    };
    let yaml = job_to_yaml(
        &id,
        &job,
        &context,
        &[],
        None,
        &lanes,
        super::super::MbxJobPolicy {
            native_mbx: false,
            actions_read: false,
        },
    )
    .expect("render verification job");
    let rendered = velnor_actions_workflow_tree::yaml::render_yaml(&yaml);

    assert!(rendered.contains("GITHUB_TOKEN: \"\""));
    assert!(rendered.contains("GH_TOKEN: \"\""));
    assert!(rendered.contains("env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    for key in [
        "MISE_NO_CONFIG",
        "MISE_NO_ENV",
        "MISE_NO_HOOKS",
        "MISE_LOCKFILE",
    ] {
        assert!(
            !rendered.contains(key),
            "task job must inherit no {key} override"
        );
    }
    assert!(rendered.contains("mise install --locked"));
    assert!(rendered.contains("mise run desktop-format-check"));
}
