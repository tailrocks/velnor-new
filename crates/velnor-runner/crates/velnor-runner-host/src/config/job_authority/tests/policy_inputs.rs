use super::*;

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

#[test]
fn policy_json_adds_one_bound_policy_without_replacing_existing_policy() -> Result<(), HostError> {
    let connections = [
        basic_connection("first", 7, "example/first", "first", "first-set"),
        basic_connection("second", 8, "example/second", "second", "second-set"),
        basic_connection("third", 9, "example/third", "third", "third-set"),
    ];
    let mut base = crate::HostConfig::parse(&host_config(&connections, 1))?;
    let first_policy = policy(&base.connections()[0]);
    base.job_authority_policies.push(first_policy.clone());
    let target_connection = &base.connections()[1];
    let target_policy = policy(target_connection);
    let policy_json = serde_json::to_string(&target_policy).map_err(|_| HostError::Config)?;
    let added =
        base.with_job_authority_policy_json(&target_connection.connection_id, &policy_json)?;
    assert_eq!(added.connections(), base.connections());
    assert_eq!(added.host, base.host);
    assert_eq!(added.docker, base.docker);
    assert!(added.job_authority_policies.contains(&first_policy));
    assert!(added.job_authority_policies.contains(&target_policy));

    assert!(
        added
            .with_job_authority_policy_json(&target_connection.connection_id, &policy_json)
            .is_err()
    );
    let unrelated_policy = policy(&base.connections()[2]);
    let unrelated_json = serde_json::to_string(&unrelated_policy).map_err(|_| HostError::Config)?;
    assert!(
        base.with_job_authority_policy_json(&target_connection.connection_id, &unrelated_json)
            .is_err()
    );
    assert!(
        base.with_job_authority_policy_json(&target_connection.connection_id, "{}")
            .is_err()
    );
    Ok(())
}
