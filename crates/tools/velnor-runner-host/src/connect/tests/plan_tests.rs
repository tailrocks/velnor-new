use super::{host_toml, stored};
use crate::{
    ConnectPlan, DockerConfig, GithubSection, HostConfig, HostError, HostLimits, connect_plan,
};

fn host_toml(repo: &str, context: &str, endpoint: &str, credential: &str) -> String {
    format!(
        concat!(
            "schema = 1\n",
            "[github]\n",
            "repository = \"{repo}\"\n",
            "scale_set_name = \"ubuntu-26.04-scale-set\"\n",
            "credential_ref = \"{credential}\"\n",
            "[host]\n",
            "max_jobs = 1\n",
            "[host.resources]\n",
            "runner_cpu_millicores = 1000\n",
            "runner_memory_bytes = 2147483648\n",
            "dind_cpu_millicores = 3000\n",
            "dind_memory_bytes = 6442450944\n",
            "[docker]\n",
            "context = \"{context}\"\n",
            "platform = \"linux/amd64\"\n",
            "endpoint = \"{endpoint}\"\n",
        ),
        repo = repo,
        context = context,
        endpoint = endpoint,
        credential = credential,
    )
}

fn stored() -> Result<HostConfig, HostError> {
    HostConfig::parse(&host_toml(
        "ChainArgos/java-monorepo",
        "orbstack",
        "unix:///var/run/docker.sock",
        "keychain:com.tailrocks.velnor.host/chainargos",
    ))
}

#[test]
fn same_binding_is_idempotent_and_other_repository_or_context_is_rejected() -> Result<(), HostError>
{
    let current = stored()?;
    assert_eq!(current.github.credential_ref.account(), "chainargos");
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
        allowed_head_branches: Vec::new(),
        workflow_rules: Vec::new(),
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
    let credential = HostConfig::parse(&good)?.github.credential_ref;
    assert_eq!(credential.service(), "com.tailrocks.velnor.host");
    assert_eq!(credential.account(), "chainargos");
    assert_eq!(
        credential.to_string(),
        "keychain:com.tailrocks.velnor.host/chainargos"
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
    for malformed in [
        "keychain:/account",
        "keychain:service/",
        "keychain:service/account/extra",
        "keychain:service/account with-space",
        "keychain:service/account\\tother",
        "keychain:service",
    ] {
        let malformed_config =
            good.replace("keychain:com.tailrocks.velnor.host/chainargos", malformed);
        assert!(HostConfig::parse(&malformed_config).is_err(), "{malformed}");
    }
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
fn resource_limits_are_required_and_aggregate_values_are_checked() {
    let valid = host_toml(
        "ChainArgos/java-monorepo",
        "orbstack",
        "unix:///var/run/docker.sock",
        "keychain:com.tailrocks.velnor.host/chainargos",
    );
    let resources = concat!(
        "[host.resources]\n",
        "runner_cpu_millicores = 1000\n",
        "runner_memory_bytes = 2147483648\n",
        "dind_cpu_millicores = 3000\n",
        "dind_memory_bytes = 6442450944\n",
    );
    assert!(HostConfig::parse(&valid.replace(resources, "")).is_err());
    assert!(
        HostConfig::parse(
            &valid.replace("runner_cpu_millicores = 1000", "runner_cpu_millicores = 0")
        )
        .is_err()
    );
    assert!(
        HostConfig::parse(&valid.replace(
            "runner_memory_bytes = 2147483648",
            "runner_memory_bytes = 1"
        ))
        .is_err()
    );

    let aggregate_overflow = valid
        .replace("max_jobs = 1", "max_jobs = 2")
        .replace(
            "runner_memory_bytes = 2147483648",
            "runner_memory_bytes = 4000000000000000000",
        )
        .replace(
            "dind_memory_bytes = 6442450944",
            "dind_memory_bytes = 4000000000000000000",
        );
    assert!(HostConfig::parse(&aggregate_overflow).is_err());
}

#[test]
fn disconnect_deletes_only_recorded_ownership() {
    assert_eq!(
        disconnect_effects(SetOwnership::Adopted, true),
        vec![DisconnectEffect::Drain]
    );
    assert_eq!(
        disconnect_effects(SetOwnership::Adopted, false),
        Vec::<DisconnectEffect>::new()
    );
    assert_eq!(
        disconnect_effects(SetOwnership::Created, true),
        vec![DisconnectEffect::Drain, DisconnectEffect::DeleteSet]
    );
    assert_eq!(
        disconnect_effects(SetOwnership::Created, false),
        vec![DisconnectEffect::DeleteSet]
    );
    let adopted = disconnect_effects(SetOwnership::Adopted, true);
    assert!(!adopted.contains(&DisconnectEffect::DeleteSet));
}

#[test]
fn structs_used_by_connect_plan_stay_secret_free() -> Result<(), HostError> {
    let config = HostConfig {
        schema: 1,
        managed_by: None,
        github: GithubSection {
            repository: "ChainArgos/java-monorepo".to_owned(),
            scale_set_name: "ubuntu-26.04-scale-set".to_owned(),
            credential_ref: crate::KeychainReference::parse("keychain:service/account")?,
        },
        host: HostLimits {
            max_jobs: 1,
            resources: crate::worker::ResourceBudgetConfig {
                runner_cpu_millicores: 1_000,
                runner_memory_bytes: 2_147_483_648,
                dind_cpu_millicores: 3_000,
                dind_memory_bytes: 6_442_450_944,
            },
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
    Ok(())
}
