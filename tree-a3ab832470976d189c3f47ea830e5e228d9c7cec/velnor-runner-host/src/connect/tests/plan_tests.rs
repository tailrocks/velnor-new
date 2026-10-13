use super::{host_toml, stored};
use crate::{
    ConnectPlan, DockerConfig, GithubSection, HostConfig, HostError, HostLimits, connect_plan,
};

#[test]
fn same_binding_is_idempotent_and_other_bindings_are_rejected() -> Result<(), HostError> {
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
        github: GithubSection {
            repository: "ChainArgos/java-monorepo".to_owned(),
            scale_set_name: "ubuntu-26.04-scale-set".to_owned(),
            credential_ref: "keychain:item".to_owned(),
        },
        host: HostLimits { max_jobs: 1 },
        docker: DockerConfig {
            context: "orbstack".to_owned(),
            platform: "linux/amd64".to_owned(),
            endpoint: "unix:///var/run/docker.sock".to_owned(),
        },
    };
    assert_eq!(connect_plan(None, &config), ConnectPlan::Create);
}
