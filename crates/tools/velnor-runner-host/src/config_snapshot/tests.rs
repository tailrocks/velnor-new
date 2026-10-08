use super::{ValidatedHostConfigSnapshot, snapshot_from_bytes, snapshot_from_optional_bytes};
use crate::{HostPlatform, RegistrationScope};
use velnor_runner_github::policy::{
    JobTrustEvidence, PolicyMismatch, PollWithTrust, get_actions_workflow_trust_run,
    parse_poll_with_trust, verify_job_offer,
};
use velnor_runner_github::{Exchange, Method, SessionRequest, Transport, TransportFail};
use velnor_runner_host_config::HostConfig;
use velnor_runner_journal::HostError;

const LINUX_CONFIG: &str = concat!(
    "schema = 1\n",
    "[github]\n",
    "repository = \"ChainArgos/java-monorepo\"\n",
    "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
    "credential_ref = \"systemd-credential:github-token\"\n",
    "registration_scope = \"repository\"\n",
    "runner_group_id = 1\n",
    "runner_group_name = \"Default\"\n",
    "[host]\n",
    "platform = \"linux\"\n",
    "max_jobs = 1\n",
    "drain_timeout_secs = 1800\n",
    "[trust]\n",
    "allowed_repositories = [\"ChainArgos/java-monorepo\"]\n",
    "allowed_events = [\"push\", \"pull_request\"]\n",
    "allowed_workflow_paths = [\".github/workflows/ci.yml\"]\n",
    "allow_forks = false\n",
    "[runner]\n",
    "image_profile = \"ubuntu-26.04-amd64\"\n",
    "[docker]\n",
    "context = \"system\"\n",
    "platform = \"linux/amd64\"\n",
    "endpoint = \"unix:///var/run/docker.sock\"\n",
);

fn snapshot(bytes: &[u8]) -> Result<ValidatedHostConfigSnapshot, HostError> {
    snapshot_from_bytes(bytes, HostPlatform::Linux)
}

#[test]
fn linux_snapshot_accepts_cleanup_config_without_runnable_image_profile() -> Result<(), HostError> {
    assert!(matches!(
        snapshot(
            &LINUX_CONFIG
                .replace("ubuntu-26.04-scale-set", "ubuntu-24.04-scale-set")
                .replace("ubuntu-26.04-amd64", "ubuntu-24.04-amd64")
                .into_bytes()
        ),
        Err(HostError::Config)
    ));
    let configured_26 = snapshot(LINUX_CONFIG.as_bytes())?;
    assert_eq!(
        configured_26
            .scale_set_binding()
            .runner_image_profile
            .as_deref(),
        Some("ubuntu-26.04-amd64")
    );
    assert!(configured_26.runner_image_profile().is_none());
    Ok(())
}

#[test]
fn snapshot_missing_file_is_an_error_only_for_linux() {
    assert!(matches!(
        snapshot_from_optional_bytes(None, HostPlatform::Linux),
        Err(HostError::Config)
    ));
    assert!(matches!(
        snapshot_from_optional_bytes(None, HostPlatform::Macos),
        Ok(None)
    ));
}

#[test]
fn snapshot_rejects_wrong_platform_invalid_utf8_and_repository_downgrade() -> Result<(), HostError>
{
    assert!(matches!(
        snapshot_from_bytes(LINUX_CONFIG.as_bytes(), HostPlatform::Macos),
        Err(HostError::Config)
    ));
    assert!(matches!(
        snapshot_from_bytes(&[0xff], HostPlatform::Linux),
        Err(HostError::Config)
    ));
    let organization = LINUX_CONFIG.replace(
        "registration_scope = \"repository\"\n",
        "registration_scope = \"organization\"\nregistration_scope_name = \"ChainArgos\"\n",
    );
    let config = HostConfig::parse(&organization)?;
    config.validate_for_host(HostPlatform::Linux)?;
    let binding = config.scale_set_binding()?;
    assert_eq!(
        binding.scope,
        RegistrationScope::Organization {
            organization: "ChainArgos".to_owned(),
        }
    );
    Ok(())
}

const WORKFLOW_RULES: &str = concat!(
    "allowed_head_branches = [\"main\", \"release\"]\n",
    "allowed_group_workflows = [\"ChainArgos/java-monorepo/.github/workflows/ci.yml@main\"]\n",
    "allow_forks = false\n",
    "[[trust.workflow_rules]]\n",
    "workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/ci.yml@main\"\n",
    "job_workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/reuse.yml@refs/tags/v1\"\n",
    "workflow_path = \".github/workflows/ci.yml@main\"\n",
    "event = \"push\"\n",
    "head_branch = \"main\"\n",
    "[[trust.workflow_rules.referenced_workflows]]\n",
    "path = \"ChainArgos/java-monorepo/.github/workflows/reuse.yml@v1\"\n",
    "git_ref = \"refs/tags/v1\"\n",
    "sha = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    "[[trust.workflow_rules]]\n",
    "workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/ci.yml@main\"\n",
    "job_workflow_ref = \"ChainArgos/java-monorepo/.github/workflows/ci.yml@refs/heads/main\"\n",
    "workflow_path = \".github/workflows/ci.yml@main\"\n",
    "event = \"pull_request\"\n",
    "head_branch = \"release\"\n",
);

fn trusted_snapshot() -> Result<ValidatedHostConfigSnapshot, HostError> {
    let config = LINUX_CONFIG.replace("allow_forks = false\n", WORKFLOW_RULES);
    snapshot_from_bytes(config.as_bytes(), HostPlatform::Linux)
}

struct WorkflowRunTransport {
    body: Vec<u8>,
    path_seen: bool,
}

impl Transport for WorkflowRunTransport {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        assert_eq!(request.method, Method::Get);
        assert_eq!(
            request.path,
            "repos/ChainArgos/java-monorepo/actions/runs/88"
        );
        assert_eq!(request.query, None);
        self.path_seen = true;
        Ok(Exchange {
            status: 200,
            body: self.body.clone(),
        })
    }
}

fn workflow_run(branch: &str) -> WorkflowRunTransport {
    WorkflowRunTransport {
        body: serde_json::json!({
            "id": 88,
            "run_attempt": 2,
            "event": "push",
            "path": ".github/workflows/ci.yml@main",
            "head_sha": "0123456789abcdef0123456789abcdef01234567",
            "head_branch": branch,
            "head_repository": {"full_name": "ChainArgos/java-monorepo"},
            "referenced_workflows": [{
                "path": "ChainArgos/java-monorepo/.github/workflows/reuse.yml@v1",
                "ref": "refs/tags/v1",
                "sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            }]
        })
        .to_string()
        .into_bytes(),
        path_seen: false,
    }
}

fn trust_batch() -> velnor_runner_github::policy::ParsedTrustBatch {
    let inner = serde_json::json!([{
        "messageType": "JobAvailable",
        "runnerRequestId": 42,
        "workflowRunId": 88,
        "ownerName": "ChainArgos",
        "repositoryName": "java-monorepo",
        "eventName": "push",
        "jobWorkflowRef": "ChainArgos/java-monorepo/.github/workflows/reuse.yml@refs/tags/v1",
        "requestLabels": ["ubuntu-26.04-scale-set"]
    }])
    .to_string();
    let envelope = serde_json::json!({
        "messageId": 7,
        "messageType": "RunnerScaleSetJobMessages",
        "body": inner
    })
    .to_string();
    let PollWithTrust::Batch(batch) = parse_poll_with_trust(200, &envelope).expect("poll parses")
    else {
        panic!("expected a job batch");
    };
    batch
}

#[test]
fn snapshot_maps_exact_rules_into_composed_message_and_rest_verification() {
    let snapshot = trusted_snapshot().expect("structured policy snapshot validates");
    let batch = trust_batch();
    let mut transport = workflow_run("main");
    let run = get_actions_workflow_trust_run(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        88,
        "fixture-actions-read",
    )
    .expect("workflow run is read through the Actions REST parser");
    assert!(transport.path_seen);

    let evidence = snapshot
        .with_job_trust_policy_view(|view| {
            assert_eq!(view.repository_full_name, "ChainArgos/java-monorepo");
            assert_eq!(view.allowed_head_branches, ["main", "release"]);
            assert_eq!(view.workflow_rules.len(), 2);
            assert_eq!(
                view.workflow_rules[0].workflow_path,
                ".github/workflows/ci.yml@main"
            );
            assert_eq!(
                view.workflow_rules[0].referenced_workflows[0].git_ref,
                "refs/tags/v1"
            );
            assert_eq!(view.policy_digest, snapshot.policy_digest());
            verify_job_offer(&batch, 0, &run, &view)
        })
        .expect("complete exact tuple policy produces a view");
    assert!(matches!(evidence, JobTrustEvidence::Verified(_)));

    let mut transport = workflow_run("release");
    let wrong_pair = get_actions_workflow_trust_run(
        &mut transport,
        "ChainArgos",
        "java-monorepo",
        88,
        "fixture-actions-read",
    )
    .expect("workflow run is read through the Actions REST parser");
    let rejected = snapshot
        .with_job_trust_policy_view(|view| verify_job_offer(&batch, 0, &wrong_pair, &view))
        .expect("complete exact tuple policy produces a view");
    assert_eq!(
        rejected,
        JobTrustEvidence::Rejected(PolicyMismatch::WorkflowReferenceMismatch)
    );
}

#[test]
fn missing_structured_policy_returns_no_per_offer_view() -> Result<(), HostError> {
    let snapshot = snapshot(LINUX_CONFIG.as_bytes())?;
    assert!(snapshot.with_job_trust_policy_view(|_| ()).is_none());
    Ok(())
}
