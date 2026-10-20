use crate::config_test_support::{basic_connection, host_config};
use crate::{
    ConnectPlan, DisconnectEffect, HostConfig, HostError, KeychainReference, SetOwnership,
    connect_plan, disconnect_effects,
};

fn stored() -> Result<HostConfig, HostError> {
    HostConfig::parse(&host_config(
        &[basic_connection(
            "chainargos",
            202,
            "ChainArgos/java-monorepo",
            "chainargos",
            "chainargos-sea-scale-set",
        )],
        1,
    ))
}

fn essential() -> String {
    basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    )
}

#[test]
fn connect_adds_connections_and_keeps_existing_bindings() -> Result<(), HostError> {
    let current = stored()?;
    assert_eq!(
        current
            .connection(&crate::ConnectionId::parse("chainargos")?)
            .map(|connection| connection.credential_ref.account()),
        Some("chainargos")
    );
    assert_eq!(connect_plan(None, &current), ConnectPlan::Create);
    assert_eq!(
        connect_plan(Some(&current), &current),
        ConnectPlan::Idempotent
    );
    let mut extended = current.clone();
    extended.add_connection(
        crate::HostConfig::parse(&host_config(&[essential()], 1))?.connections()[0].clone(),
    )?;
    assert_eq!(
        connect_plan(Some(&current), &extended),
        ConnectPlan::AddConnection
    );
    assert_eq!(extended.host.max_jobs, 1);
    let round_trip = HostConfig::parse(&extended.to_toml()?)?;
    assert_eq!(
        connect_plan(Some(&extended), &round_trip),
        ConnectPlan::Idempotent
    );
    Ok(())
}

#[test]
fn connect_rejects_removed_mutated_or_replaced_bindings() -> Result<(), HostError> {
    let current = stored()?;
    let changed_repository = HostConfig::parse(&host_config(
        &[basic_connection(
            "chainargos",
            202,
            "other/repository",
            "chainargos",
            "chainargos-sea-scale-set",
        )],
        1,
    ))?;
    assert_eq!(
        connect_plan(Some(&current), &changed_repository),
        ConnectPlan::Rejected
    );
    let changed_selector = HostConfig::parse(&host_config(
        &[basic_connection(
            "chainargos",
            202,
            "ChainArgos/java-monorepo",
            "chainargos",
            "other-scale-set",
        )],
        1,
    ))?;
    assert_eq!(
        connect_plan(Some(&current), &changed_selector),
        ConnectPlan::Rejected
    );
    let mut extended = current.clone();
    extended.add_connection(
        crate::HostConfig::parse(&host_config(&[essential()], 1))?.connections()[0].clone(),
    )?;
    let removed = HostConfig::parse(&host_config(&[essential()], 1))?;
    assert_eq!(
        connect_plan(Some(&extended), &removed),
        ConnectPlan::Rejected
    );
    let replaced = HostConfig::parse(&host_config(
        &[
            basic_connection(
                "different-id",
                202,
                "ChainArgos/java-monorepo",
                "chainargos",
                "chainargos-sea-scale-set",
            ),
            essential(),
        ],
        1,
    ))?;
    assert_eq!(
        connect_plan(Some(&extended), &replaced),
        ConnectPlan::Rejected
    );
    Ok(())
}

#[test]
fn connect_rejects_changes_to_shared_host_or_docker_settings() -> Result<(), HostError> {
    let current = stored()?;
    let more_jobs = HostConfig::parse(&host_config(
        &[basic_connection(
            "chainargos",
            202,
            "ChainArgos/java-monorepo",
            "chainargos",
            "chainargos-sea-scale-set",
        )],
        2,
    ))?;
    assert_eq!(
        connect_plan(Some(&current), &more_jobs),
        ConnectPlan::Rejected
    );
    let changed_docker = HostConfig::parse(
        &host_config(
            &[basic_connection(
                "chainargos",
                202,
                "ChainArgos/java-monorepo",
                "chainargos",
                "chainargos-sea-scale-set",
            )],
            1,
        )
        .replace("context = \"orbstack\"", "context = \"colima\""),
    )?;
    assert_eq!(
        connect_plan(Some(&current), &changed_docker),
        ConnectPlan::Rejected
    );
    Ok(())
}

#[test]
fn connect_is_idempotent_when_connection_order_changes() -> Result<(), HostError> {
    let ordered = HostConfig::parse(&host_config(&[stored_connection(), essential()], 1))?;
    let reversed = HostConfig::parse(&host_config(&[essential(), stored_connection()], 1))?;
    assert_eq!(
        connect_plan(Some(&ordered), &reversed),
        ConnectPlan::Idempotent
    );
    Ok(())
}

#[test]
fn disconnect_removes_only_the_selected_binding_and_keeps_shared_settings() -> Result<(), HostError>
{
    let mut config = HostConfig::parse(&host_config(&[stored_connection(), essential()], 1))?;
    let chainargos = crate::ConnectionId::parse("chainargos")?;
    assert!(config.remove_connection(&chainargos)?);
    assert!(!config.remove_connection(&chainargos)?);
    assert_eq!(config.connections().len(), 1);
    assert_eq!(config.connections()[0].connection_id.as_str(), "essential");
    assert_eq!(config.host.max_jobs, 1);
    assert_eq!(config.docker.context, "orbstack");

    let essential_id = crate::ConnectionId::parse("essential")?;
    assert!(config.remove_connection(&essential_id)?);
    assert_eq!(config.connections(), []);
    let round_trip = HostConfig::parse(&config.to_toml()?)?;
    assert_eq!(round_trip, config);
    assert_eq!(round_trip.host.max_jobs, 1);
    assert_eq!(round_trip.docker.context, "orbstack");
    assert_eq!(connect_plan(None, &round_trip), ConnectPlan::Rejected);

    let new_connection = HostConfig::parse(&host_config(&[essential()], 1))?;
    let mut reconnected = round_trip.clone();
    reconnected.add_connection(new_connection.connections()[0].clone())?;
    assert_eq!(
        connect_plan(Some(&round_trip), &reconnected),
        ConnectPlan::AddConnection
    );
    Ok(())
}

fn stored_connection() -> String {
    basic_connection(
        "chainargos",
        202,
        "ChainArgos/java-monorepo",
        "chainargos",
        "chainargos-sea-scale-set",
    )
}

#[test]
fn keychain_reference_rejects_token_fields_and_bad_endpoints() -> Result<(), HostError> {
    let config = host_config(
        &[basic_connection(
            "chainargos",
            202,
            "ChainArgos/java-monorepo",
            "chainargos",
            "chainargos-sea-scale-set",
        )],
        1,
    );
    let credential = stored()?.connections()[0].credential_ref.clone();
    assert_eq!(credential.service(), "com.tailrocks.velnor.host");
    assert_eq!(credential.account(), "chainargos");
    assert_eq!(
        credential.to_string(),
        "keychain:com.tailrocks.velnor.host/chainargos"
    );
    assert!(
        HostConfig::parse(&config.replace(
            "keychain:com.tailrocks.velnor.host/chainargos",
            "ghp_secret"
        ))
        .is_err()
    );
    assert!(
        HostConfig::parse(
            &config.replace("keychain:com.tailrocks.velnor.host/chainargos", "keychain:")
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
        assert!(
            HostConfig::parse(
                &config.replace("keychain:com.tailrocks.velnor.host/chainargos", malformed)
            )
            .is_err()
        );
    }
    assert!(
        HostConfig::parse(&config.replace(
            "endpoint = \"unix:///var/run/docker.sock\"",
            "endpoint = \"tcp://127.0.0.1:2375\""
        ))
        .is_err()
    );
    Ok(())
}

#[test]
fn resources_are_required_and_checked_against_the_shared_job_limit() {
    let valid = host_config(
        &[basic_connection(
            "chainargos",
            202,
            "ChainArgos/java-monorepo",
            "chainargos",
            "chainargos-sea-scale-set",
        )],
        1,
    );
    let resources = concat!(
        "[host.resources]\n",
        "runner_cpu_millicores = 1000\n",
        "runner_memory_bytes = 2147483648\n",
        "dind_cpu_millicores = 3000\n",
        "dind_memory_bytes = 6442450944\n",
        "operational_memory_reserve_bytes = 1073741824\n",
    );
    assert!(HostConfig::parse(&valid.replace(resources, "")).is_err());
    assert!(
        HostConfig::parse(&valid.replace("operational_memory_reserve_bytes = 1073741824\n", ""))
            .is_err()
    );
    assert!(
        HostConfig::parse(&valid.replace(
            "operational_memory_reserve_bytes = 1073741824",
            "operational_memory_reserve_bytes = 0",
        ))
        .is_err()
    );
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
fn disconnect_deletes_only_controller_created_sets() {
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
}

#[test]
fn keychain_reference_constructor_keeps_credentials_secret_free() -> Result<(), HostError> {
    let reference = KeychainReference::parse("keychain:service/account")?;
    assert_eq!(reference.service(), "service");
    assert_eq!(reference.account(), "account");
    Ok(())
}
