use std::collections::BTreeMap;

use super::*;
use crate::config::{RepositoryConnection, SourcePolicy, SourcePolicyMode};
use crate::config_test_support::{basic_connection, host_config};

fn connection() -> Result<RepositoryConnection, HostError> {
    let config = super::super::super::HostConfig::parse(&host_config(
        &[basic_connection(
            "test",
            7,
            "example/repo",
            "test",
            "test-scale-set",
        )],
        1,
    ))?;
    Ok(config.connections()[0].clone())
}

fn entry(path: &str, key: &str) -> ApprovedJobAuthorityEntry {
    ApprovedJobAuthorityEntry {
        root_path: path.to_owned(),
        workflow_path: path.to_owned(),
        job_key: key.to_owned(),
        role: JobAuthorityRole::Verification,
        contexts: vec![ApprovedEventContext {
            event: WorkflowEvent::Push,
            refs: vec!["refs/heads/sea/integration".to_owned()],
            pull_request_rules: Vec::new(),
        }],
        permissions_declared: true,
        permissions: BTreeMap::from([("contents".to_owned(), PermissionLevel::Read)]),
        secrets: Vec::new(),
        environments: Vec::new(),
        condition_sha256: None,
        concurrency: None,
        needs: Vec::new(),
    }
}

fn policy(connection: &RepositoryConnection) -> ApprovedJobAuthorityPolicy {
    let path = ".github/workflows/ci.yml";
    let mut policy = ApprovedJobAuthorityPolicy {
        schema_version: POLICY_SCHEMA,
        connection_id: connection.connection_id.clone(),
        binding_sha256: sha256(
            &connection
                .canonical_identity_bytes()
                .expect("connection identity"),
        ),
        generation: 1,
        policy_sha256: String::new(),
        generator: GeneratorIdentity {
            source_sha: "a".repeat(40),
            artifact_sha256: "b".repeat(64),
        },
        workflows: vec![ApprovedWorkflow {
            repository_id: connection.repository_id,
            path: path.to_owned(),
            content_sha256: "c".repeat(64),
            calls: Vec::new(),
            jobs: vec![entry(path, "required")],
        }],
    };
    policy.canonicalize();
    policy.policy_sha256 = policy_digest(&policy);
    policy
}

fn policy_digest(policy: &ApprovedJobAuthorityPolicy) -> String {
    super::sha256(&serde_json::to_vec(&policy.content()).expect("policy content"))
}

fn validate(policy: &ApprovedJobAuthorityPolicy) -> Result<(), HostError> {
    super::validate::validate_policy(policy, &connection()?)
}

fn lookup<'a>(
    connection_id: &'a ConnectionId,
    generation: u64,
    event_ref: &'a str,
) -> JobAuthorityLookup<'a> {
    JobAuthorityLookup {
        connection_id,
        generation,
        root_path: ".github/workflows/ci.yml",
        workflow_path: ".github/workflows/ci.yml",
        job_key: "required",
        event: WorkflowEvent::Push,
        event_ref,
        pull_request: None,
    }
}

#[test]
fn valid_policy_matches_exact_job_context_and_round_trips() -> Result<(), HostError> {
    let connection = connection()?;
    let policy = policy(&connection);
    validate(&policy)?;
    let toml = toml::to_string(&policy).map_err(|_| HostError::Config)?;
    let reopened: ApprovedJobAuthorityPolicy =
        toml::from_str(&toml).map_err(|_| HostError::Config)?;
    assert_eq!(reopened, policy);
    let connection_id = policy.connection_id();
    assert!(
        policy
            .entry(&lookup(connection_id, 1, "refs/heads/sea/integration"))
            .is_some()
    );
    assert!(
        policy
            .entry(&lookup(connection_id, 1, "refs/heads/main"))
            .is_none()
    );
    Ok(())
}

#[test]
fn source_inventory_exposes_the_validated_call_graph_without_mutable_policy_fields()
-> Result<(), HostError> {
    let connection = connection()?;
    let root = ".github/workflows/ci.yml";
    let leaf = ".github/workflows/reusable.yml";
    let mut policy = policy(&connection);
    policy.workflows[0].calls.push(WorkflowCall {
        repository_id: connection.repository_id,
        path: leaf.to_owned(),
        content_sha256: "d".repeat(64),
    });
    policy.workflows.push(ApprovedWorkflow {
        repository_id: connection.repository_id,
        path: leaf.to_owned(),
        content_sha256: "d".repeat(64),
        calls: Vec::new(),
        jobs: vec![{
            let mut job = entry(root, "reusable");
            job.workflow_path = leaf.to_owned();
            job
        }],
    });
    policy.canonicalize();
    policy.policy_sha256 = policy_digest(&policy);
    validate(&policy)?;

    let inventory = policy
        .workflow_sources()
        .map(|workflow| {
            (
                workflow.repository_id(),
                workflow.path().to_owned(),
                workflow.content_sha256().to_owned(),
                workflow
                    .calls()
                    .map(|call| {
                        (
                            call.repository_id(),
                            call.path().to_owned(),
                            call.content_sha256().to_owned(),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        inventory,
        vec![
            (
                connection.repository_id,
                root.to_owned(),
                "c".repeat(64),
                vec![(connection.repository_id, leaf.to_owned(), "d".repeat(64),)],
            ),
            (
                connection.repository_id,
                leaf.to_owned(),
                "d".repeat(64),
                Vec::new(),
            ),
        ]
    );
    Ok(())
}

#[test]
fn host_config_missing_and_stale_policy_lookup_deny() -> Result<(), HostError> {
    let connection_text = basic_connection("test", 7, "example/repo", "test", "test-scale-set");
    let missing =
        crate::HostConfig::parse(&host_config(std::slice::from_ref(&connection_text), 1))?;
    let id = ConnectionId::parse("test")?;
    assert!(
        missing
            .approved_job_authority(&lookup(&id, 1, "refs/heads/sea/integration"))
            .is_none()
    );

    let connection = missing.connections()[0].clone();
    let mut configured = missing.clone();
    configured.job_authority_policies.push(policy(&connection));
    let configured = crate::HostConfig::parse(&configured.to_toml()?)?;
    assert!(
        configured
            .approved_job_authority(&lookup(&id, 0, "refs/heads/sea/integration"))
            .is_none()
    );
    assert!(
        configured
            .approved_job_authority(&lookup(&id, 2, "refs/heads/sea/integration"))
            .is_none()
    );
    Ok(())
}

#[test]
fn policy_digest_binds_workflow_content_and_job_authority() -> Result<(), HostError> {
    let connection = connection()?;
    let mut changed = policy(&connection);
    changed.workflows[0].content_sha256 = "d".repeat(64);
    assert!(validate(&changed).is_err());
    changed.policy_sha256 = policy_digest(&changed);
    validate(&changed)?;

    changed.workflows[0].jobs[0]
        .permissions
        .insert("contents".to_owned(), PermissionLevel::Write);
    assert!(validate(&changed).is_err());
    Ok(())
}

#[test]
fn binding_change_and_connection_digest_change_reject_policy() -> Result<(), HostError> {
    let connection = connection()?;
    let policy = policy(&connection);
    let mut changed = policy.clone();
    changed.binding_sha256 = "d".repeat(64);
    changed.policy_sha256 = policy_digest(&changed);
    assert!(super::validate::validate_policy(&changed, &connection).is_err());

    let mut changed_connection = connection.clone();
    changed_connection.repository = "example/other".to_owned();
    assert!(super::validate::validate_policy(&policy, &changed_connection).is_err());
    Ok(())
}

#[test]
fn duplicate_workflows_jobs_contexts_and_matrix_authority_reject() -> Result<(), HostError> {
    let connection = connection()?;
    let mut duplicate_workflow = policy(&connection);
    duplicate_workflow
        .workflows
        .push(duplicate_workflow.workflows[0].clone());
    duplicate_workflow.canonicalize();
    duplicate_workflow.policy_sha256 = policy_digest(&duplicate_workflow);
    assert!(validate(&duplicate_workflow).is_err());

    let mut duplicate_job = policy(&connection);
    duplicate_job.workflows[0]
        .jobs
        .push(entry(".github/workflows/ci.yml", "required"));
    duplicate_job.canonicalize();
    duplicate_job.policy_sha256 = policy_digest(&duplicate_job);
    assert!(validate(&duplicate_job).is_err());

    let mut mixed_matrix_authority = policy(&connection);
    let mut writer = entry(".github/workflows/ci.yml", "required");
    writer.role = JobAuthorityRole::TrustedPublication;
    writer.condition_sha256 = Some("e".repeat(64));
    writer.concurrency = Some(ConcurrencyControl {
        scope: ConcurrencyScope::Workflow,
        group: "${{ github.workflow }}-${{ github.ref }}".to_owned(),
        cancel_in_progress: "false".to_owned(),
    });
    mixed_matrix_authority.workflows[0].jobs.push(writer);
    mixed_matrix_authority.canonicalize();
    mixed_matrix_authority.policy_sha256 = policy_digest(&mixed_matrix_authority);
    assert!(validate(&mixed_matrix_authority).is_err());

    let mut duplicate_context = policy(&connection);
    let context = duplicate_context.workflows[0].jobs[0].contexts[0].clone();
    duplicate_context.workflows[0].jobs[0]
        .contexts
        .push(context);
    duplicate_context.canonicalize();
    duplicate_context.policy_sha256 = policy_digest(&duplicate_context);
    assert!(validate(&duplicate_context).is_err());
    Ok(())
}

#[test]
fn policy_canonicalization_sorts_nested_contexts_before_contexts() -> Result<(), HostError> {
    let connection = connection()?;
    let mut candidate = policy(&connection);
    candidate.workflows[0].jobs[0].contexts = vec![
        ApprovedEventContext {
            event: WorkflowEvent::Push,
            refs: vec!["refs/heads/z".to_owned(), "refs/heads/a".to_owned()],
            pull_request_rules: Vec::new(),
        },
        ApprovedEventContext {
            event: WorkflowEvent::Push,
            refs: vec!["refs/heads/b".to_owned()],
            pull_request_rules: Vec::new(),
        },
    ];
    candidate.canonicalize();
    let canonical = candidate.clone();
    candidate.canonicalize();
    assert_eq!(candidate, canonical);
    Ok(())
}

#[test]
fn repository_policy_allows_narrow_pr_and_dispatch_tag_contexts() -> Result<(), HostError> {
    let mut connection = connection()?;
    connection.source_policy = SourcePolicy::try_new(
        SourcePolicyMode::Repository,
        Vec::new(),
        Vec::new(),
        vec![WorkflowEvent::PullRequest, WorkflowEvent::WorkflowDispatch],
        Vec::new(),
    )?;
    let mut policy = policy(&connection);
    policy.workflows[0].jobs[0].contexts = vec![
        ApprovedEventContext {
            event: WorkflowEvent::PullRequest,
            refs: Vec::new(),
            pull_request_rules: vec![PullRequestRule {
                number: Some(9),
                base_repository_id: connection.repository_id,
                base_branch: "sea/integration".to_owned(),
                head_repository_id: connection.repository_id,
                head_branch: "feature".to_owned(),
            }],
        },
        ApprovedEventContext {
            event: WorkflowEvent::WorkflowDispatch,
            refs: vec!["refs/tags/v1".to_owned(), "refs/heads/main".to_owned()],
            pull_request_rules: Vec::new(),
        },
    ];
    policy.canonicalize();
    policy.policy_sha256 = policy_digest(&policy);
    super::validate::validate_policy(&policy, &connection)?;
    Ok(())
}

#[test]
fn unknown_and_malformed_policy_fields_reject() -> Result<(), HostError> {
    let connection = connection()?;
    let policy = policy(&connection);
    let mut value = serde_json::to_value(&policy).map_err(|_| HostError::Config)?;
    value["unexpected"] = serde_json::Value::Bool(true);
    assert!(serde_json::from_value::<ApprovedJobAuthorityPolicy>(value).is_err());

    let mut malformed = policy.clone();
    malformed.workflows[0].content_sha256 = "C".repeat(64);
    malformed.policy_sha256 = policy_digest(&malformed);
    assert!(validate(&malformed).is_err());

    let mut zero_generation = policy;
    zero_generation.generation = 0;
    zero_generation.policy_sha256 = policy_digest(&zero_generation);
    assert!(validate(&zero_generation).is_err());
    Ok(())
}

#[test]
fn omitted_permissions_and_unknown_permission_values_reject() -> Result<(), HostError> {
    let connection = connection()?;
    let mut explicit_empty = policy(&connection);
    explicit_empty.workflows[0].jobs[0].permissions.clear();
    explicit_empty.policy_sha256 = policy_digest(&explicit_empty);
    validate(&explicit_empty)?;

    let mut omitted = policy(&connection);
    omitted.workflows[0].jobs[0].permissions_declared = false;
    omitted.policy_sha256 = policy_digest(&omitted);
    assert!(validate(&omitted).is_err());

    let mut unresolved = policy(&connection);
    unresolved.workflows[0].jobs[0]
        .permissions
        .insert("contents".to_owned(), PermissionLevel::Write);
    unresolved.workflows[0].jobs[0].role = JobAuthorityRole::TrustedPublication;
    unresolved.workflows[0].jobs[0].condition_sha256 = Some("d".repeat(64));
    unresolved.workflows[0].jobs[0].concurrency = Some(ConcurrencyControl {
        scope: ConcurrencyScope::Workflow,
        group: "${{ github.workflow }}-${{ github.ref }}".to_owned(),
        cancel_in_progress: "false".to_owned(),
    });
    unresolved.workflows[0].jobs[0]
        .permissions
        .insert("unknown-scope".to_owned(), PermissionLevel::Write);
    unresolved.policy_sha256 = policy_digest(&unresolved);
    assert!(validate(&unresolved).is_err());
    Ok(())
}

#[test]
fn environment_names_keep_literal_names_and_reject_case_duplicates() -> Result<(), HostError> {
    let connection = connection()?;
    let mut candidate = policy(&connection);
    let job = &mut candidate.workflows[0].jobs[0];
    job.role = JobAuthorityRole::TrustedPublication;
    job.condition_sha256 = Some("d".repeat(64));
    job.concurrency = Some(ConcurrencyControl {
        scope: ConcurrencyScope::Workflow,
        group: "${{ github.workflow }}-${{ github.ref }}".to_owned(),
        cancel_in_progress: "false".to_owned(),
    });
    job.environments = vec!["Production deployment".to_owned()];
    candidate.canonicalize();
    candidate.policy_sha256 = policy_digest(&candidate);
    validate(&candidate)?;

    candidate.workflows[0].jobs[0].environments = vec![
        "Production deployment".to_owned(),
        "production deployment".to_owned(),
    ];
    candidate.canonicalize();
    candidate.policy_sha256 = policy_digest(&candidate);
    assert!(validate(&candidate).is_err());
    Ok(())
}

#[test]
fn connection_identity_bytes_do_not_include_job_authority() -> Result<(), HostError> {
    let connection_text = basic_connection("test", 7, "example/repo", "test", "test-scale-set");
    let before = crate::HostConfig::parse(&host_config(std::slice::from_ref(&connection_text), 1))?;
    let before_bytes = before.connections()[0].canonical_identity_bytes()?;
    let mut config = before.clone();
    config
        .job_authority_policies
        .push(policy(&before.connections()[0]));
    let with_policy = crate::HostConfig::parse(&config.to_toml()?)?;
    assert_eq!(
        with_policy.connections()[0].canonical_identity_bytes()?,
        before_bytes
    );
    let encoded = with_policy.to_toml()?;
    assert!(encoded.contains("[[job_authority_policies]]"));
    Ok(())
}
