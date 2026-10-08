use super::{host_toml, stored};
use crate::{
    ConnectPlan, DockerConfig, GithubSection, HostConfig, HostError, HostLimits, connect_plan,
};

#[test]
fn same_binding_is_idempotent_and_other_repository_or_context_is_rejected() -> Result<(), HostError>
{
    let current = stored()?;
    assert!(current.github.credential_ref.starts_with("keychain:"));
    assert_eq!(connect_plan(None, &current), ConnectPlan::Create);
    assert_eq!(
        connect_plan(Some(&current), &current),
        ConnectPlan::Idempotent
    );
    let other_repo = HostConfig::parse(&host_toml(
        "other/repo",
        "orbstack",
        "unix:///var/run/docker.sock",
        "keychain:com.tailrocks.velnor.host/chainargos",
    ))?;
    assert_eq!(
        connect_plan(Some(&current), &other_repo),
        ConnectPlan::Rejected
    );
    let other_context = HostConfig::parse(&host_toml(
        "ChainArgos/java-monorepo",
        "colima",
        "unix:///var/run/docker.sock",
        "keychain:com.tailrocks.velnor.host/chainargos",
    ))?;
    assert_eq!(
        connect_plan(Some(&current), &other_context),
        ConnectPlan::Rejected
    );
    Ok(())
}

#[test]
fn changed_scale_set_and_registration_identity_are_rejected() -> Result<(), HostError> {
    let current = stored()?;
    let mut other_set = current.clone();
    other_set.github.scale_set_name = "other-scale-set".to_owned();
    assert_eq!(
        connect_plan(Some(&current), &other_set),
        ConnectPlan::Rejected
    );
    let mut other_endpoint = current.clone();
    other_endpoint.docker.endpoint = "unix:///Users/me/.orbstack/run/docker.sock".to_owned();
    assert_eq!(
        connect_plan(Some(&current), &other_endpoint),
        ConnectPlan::Rejected
    );
    let mut other_group = current.clone();
    other_group.github.runner_group_id = Some(4);
    assert_eq!(
        connect_plan(Some(&current), &other_group),
        ConnectPlan::Rejected
    );
    let mut other_group_name = current.clone();
    other_group_name.github.runner_group_name = Some("Other".to_owned());
    assert_eq!(
        connect_plan(Some(&current), &other_group_name),
        ConnectPlan::Rejected
    );
    let mut other_scope = current.clone();
    other_scope.github.registration_scope = Some(crate::RegistrationScopeKind::Repository);
    assert_eq!(
        connect_plan(Some(&current), &other_scope),
        ConnectPlan::Rejected
    );
    Ok(())
}

#[test]
fn changed_capacity_credential_and_trust_are_rejected() -> Result<(), HostError> {
    let current = stored()?;
    let mut other_capacity = current.clone();
    other_capacity.host.max_jobs = Some(2);
    assert_eq!(
        connect_plan(Some(&current), &other_capacity),
        ConnectPlan::Rejected
    );
    let mut other_drain = current.clone();
    other_drain.host.drain_timeout_secs = Some(900);
    assert_eq!(
        connect_plan(Some(&current), &other_drain),
        ConnectPlan::Rejected
    );
    let mut other_credential = current.clone();
    other_credential.github.credential_ref = "keychain:com.example/token".to_owned();
    assert_eq!(
        connect_plan(Some(&current), &other_credential),
        ConnectPlan::Rejected
    );
    let mut other_trust = current.clone();
    other_trust.trust = Some(crate::JobTrustPolicy {
        allowed_repositories: vec![current.github.repository.clone()],
        allowed_events: vec!["push".to_owned()],
        allowed_workflow_paths: vec![".github/workflows/ci.yml".to_owned()],
        allowed_group_workflows: Vec::new(),
        allow_forks: false,
    });
    assert_eq!(
        connect_plan(Some(&current), &other_trust),
        ConnectPlan::Rejected
    );
    Ok(())
}

#[test]
fn changed_image_profile_is_rejected() -> Result<(), HostError> {
    let current = stored()?;
    let mut other_image = current.clone();
    other_image.runner = Some(crate::RunnerConfig {
        image_profile: "ubuntu-24.04-amd64".to_owned(),
    });
    assert_eq!(
        connect_plan(Some(&current), &other_image),
        ConnectPlan::Rejected
    );
    Ok(())
}

#[test]
fn credential_is_a_keychain_ref() -> Result<(), HostError> {
    let good = host_toml(
        "ChainArgos/java-monorepo",
        "orbstack",
        "unix:///var/run/docker.sock",
        "keychain:com.tailrocks.velnor.host/chainargos",
    );
    assert!(
        HostConfig::parse(&good)?
            .github
            .credential_ref
            .starts_with("keychain:")
    );
    assert!(
        HostConfig::parse(&good.replace(
            "keychain:com.tailrocks.velnor.host/chainargos",
            "ghp_secret"
        ))
        .is_err()
    );
    assert!(
        HostConfig::parse(
            &good.replace("keychain:com.tailrocks.velnor.host/chainargos", "keychain:")
        )
        .is_err()
    );
    assert!(
        HostConfig::parse(&good.replacen("schema = 1\n", "schema = 1\npat = \"ghp_secret\"\n", 1))
            .is_err()
    );
    assert!(HostConfig::parse(&good.replacen(
        "credential_ref = \"keychain:com.tailrocks.velnor.host/chainargos\"\n",
        "credential_ref = \"keychain:com.tailrocks.velnor.host/chainargos\"\ntoken = \"ghp_secret\"\n",
        1
    ))
    .is_err());
    assert!(
        HostConfig::parse(&good.replace("unix:///var/run/docker.sock", "tcp://127.0.0.1:2375"))
            .is_err()
    );
    assert!(
        HostConfig::parse(
            &good.replace("unix:///var/run/docker.sock", "unix://var/run/docker.sock")
        )
        .is_err()
    );
    assert!(
        HostConfig::parse(&good.replace("ChainArgos/java-monorepo", "/java-monorepo")).is_err()
    );
    assert!(HostConfig::parse(&good.replace("ChainArgos/java-monorepo", "ChainArgos/")).is_err());
    Ok(())
}

#[test]
fn structs_used_by_connect_plan_stay_secret_free() {
    let config = HostConfig {
        schema: 1,
        managed_by: None,
        github: GithubSection {
            repository: "ChainArgos/java-monorepo".to_owned(),
            scale_set_name: "ubuntu-26.04-scale-set".to_owned(),
            credential_ref: "keychain:item".to_owned(),
            registration_scope: None,
            registration_scope_name: None,
            runner_group_id: None,
            runner_group_name: None,
        },
        host: HostLimits {
            max_jobs: Some(1),
            platform: None,
            drain_timeout_secs: None,
        },
        docker: DockerConfig {
            context: "orbstack".to_owned(),
            platform: "linux/amd64".to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
        },
        trust: None,
        runner: None,
    };
    assert_eq!(connect_plan(None, &config), ConnectPlan::Create);
}
