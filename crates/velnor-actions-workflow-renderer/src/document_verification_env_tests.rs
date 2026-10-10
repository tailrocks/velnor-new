//! Verification-job environment keeps repo Mise config visible while scrubbing credentials.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::permissions::PermissionLevel;
use velnor_actions_contract::{MiseTaskSource, VerificationRunner, VerificationTask};

use crate::{
    MiseSetup, RenderContext, VerificationTaskPolicy, build_verification_task_job,
    verification_jobs::WorkflowTaskPolicy,
};

use super::job_to_yaml;

#[test]
fn report_fetch_requires_the_exact_read_permission() {
    assert!(super::grants_exact_actions_read(PermissionLevel::Read));
    assert!(!super::grants_exact_actions_read(PermissionLevel::Write));
    assert!(!super::grants_exact_actions_read(PermissionLevel::None));
}

#[test]
fn emitted_verification_job_scrubs_credentials_without_disabling_mise_config() {
    let policy = VerificationTaskPolicy {
        task: VerificationTask {
            id: "native-format".to_owned(),
            mise_task: "desktop-format-check".to_owned(),
            source: MiseTaskSource {
                mise_config: "mise.toml".to_owned(),
                working_directory: ".".to_owned(),
            },
            runner: VerificationRunner::Macos26Arm64,
            timeout_minutes: 10,
        },
        runner_label: "macos-26".to_owned(),
        scale_set_token: None,
        mise_setup: MiseSetup {
            uses: "jdx/mise-action@0123456789abcdef0123456789abcdef01234567".to_owned(),
            version: "2026.10.4".to_owned(),
            sha256: "a".repeat(64),
        },
        selected_tools: Vec::new(),
        mise_config_sha256: Some("b".repeat(64)),
        mise_lock_sha256: None,
        rust_toolchain_sha256: None,
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
        workflow_tasks: vec![WorkflowTaskPolicy::Verification(policy)],
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    };
    let checkouts = BTreeMap::new();
    let steps = BTreeMap::new();
    let lanes = crate::document_lanes::SharedLaneSteps {
        checkouts: &checkouts,
        env_steps: &steps,
        runtime_preludes: &steps,
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
        super::JobRenderPolicy {
            native_mbx: false,
            actions_read: false,
            workflow_env: &BTreeMap::new(),
        },
    )
    .expect("render verification job");
    let rendered = crate::yaml::render_yaml(&yaml);

    assert!(rendered.contains("GITHUB_TOKEN: \"\""));
    assert!(rendered.contains("GH_TOKEN: \"\""));
    assert!(rendered.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    for key in ["MISE_NO_CONFIG", "MISE_LOCKFILE"] {
        assert!(
            !rendered.contains(key),
            "task job must inherit no {key} override"
        );
    }
    assert!(rendered.contains("export MISE_NO_ENV=1"));
    assert!(rendered.contains("export MISE_NO_HOOKS=1"));
    assert!(!rendered.contains("mise --no-env --locked --no-hooks install"));
    assert!(rendered.contains("mise --no-env --no-hooks run --skip-tools desktop-format-check"));
}
