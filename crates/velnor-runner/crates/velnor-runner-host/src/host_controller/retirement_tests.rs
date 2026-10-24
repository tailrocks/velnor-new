use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::config::HostConfig;
use crate::config_test_support::{basic_connection, host_config};
use crate::controller_ipc::ControlState;
use crate::journal::Journal;

use super::retirement::{proof_is_current, removed_bindings, retire_if_removed};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

#[tokio::test]
async fn restart_retires_an_exact_drained_binding_from_cli_proof() -> Result<(), String> {
    let state = test_state("retire-restart");
    write_proofs(&state)?;
    let connection = connection()?;
    let active =
        HostConfig::parse(&host_config(&[sibling_text()], 1)).map_err(|error| error.to_string())?;
    fs::write(state.join("host.toml"), host_config(&[sibling_text()], 1))
        .map_err(|error| error.to_string())?;
    let journal = Journal::open(&state.join("launch.db"))
        .await
        .map_err(|error| error.to_string())?;
    journal
        .connection_scope(&connection)
        .await
        .map_err(|error| error.to_string())?;

    let candidates = removed_bindings(&state, &active).map_err(|error| error.to_string())?;
    if candidates.as_slice() != [connection.clone()] {
        return Err("restart scan did not recover the exact removed binding".to_owned());
    }
    if !proof_is_current(&state, &connection).map_err(|error| error.to_string())? {
        return Err("drain proof failed its pre-mutation recheck".to_owned());
    }
    let still_published = ControlState::new(
        &HostConfig::parse(&host_config(&[connection_text()], 1))
            .map_err(|error| error.to_string())?,
    );
    if retire_if_removed(&state, &journal, &still_published, &connection)
        .await
        .map_err(|error| error.to_string())?
    {
        return Err("a still-published binding was retired".to_owned());
    }
    let detached = ControlState::new(&active);
    if !retire_if_removed(&state, &journal, &detached, &connection)
        .await
        .map_err(|error| error.to_string())?
        || !journal
            .is_retired_and_drained(&connection)
            .await
            .map_err(|error| error.to_string())?
    {
        return Err("restart did not retire the already drained scope".to_owned());
    }
    fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn restart_scan_skips_configured_or_unproven_bindings() -> Result<(), String> {
    let state = test_state("retire-gates");
    write_proofs(&state)?;
    let configured = HostConfig::parse(&host_config(&[connection_text()], 1))
        .map_err(|error| error.to_string())?;
    if !removed_bindings(&state, &configured)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err("configured binding was queued for retirement".to_owned());
    }
    fs::write(
        state.join("connection-drain/essential"),
        "fedcba9876543210fedcba9876543210",
    )
    .map_err(|error| error.to_string())?;
    let absent =
        HostConfig::parse(&host_config(&[sibling_text()], 1)).map_err(|error| error.to_string())?;
    if !removed_bindings(&state, &absent)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err("stale token was accepted for restart retirement".to_owned());
    }
    fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn restart_scan_rejects_a_group_accessible_proof_directory() -> Result<(), String> {
    let state = test_state("public-proof-directory");
    write_proofs(&state)?;
    let directory = state.join("connection-drain");
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))
        .map_err(|error| error.to_string())?;
    let absent =
        HostConfig::parse(&host_config(&[sibling_text()], 1)).map_err(|error| error.to_string())?;
    if !matches!(
        removed_bindings(&state, &absent),
        Err(crate::error::HostError::Path)
    ) {
        return Err("group-accessible proof directory was trusted".to_owned());
    }
    fs::remove_dir_all(state).map_err(|error| error.to_string())?;
    Ok(())
}

fn write_proofs(state: &std::path::Path) -> Result<(), String> {
    let directory = state.join("connection-drain");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
        .map_err(|error| error.to_string())?;
    let connection = connection()?;
    let identity = connection
        .canonical_identity_bytes()
        .map_err(|error| error.to_string())?;
    fs::write(directory.join("essential"), TOKEN).map_err(|error| error.to_string())?;
    fs::write(directory.join("essential~binding"), &identity).map_err(|error| error.to_string())?;
    let mut proof = TOKEN.as_bytes().to_vec();
    proof.push(b'\n');
    proof.extend_from_slice(&identity);
    fs::write(directory.join("essential~drained"), proof).map_err(|error| error.to_string())?;
    for file in ["essential", "essential~binding", "essential~drained"] {
        fs::set_permissions(directory.join(file), fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn connection() -> Result<crate::config::RepositoryConnection, String> {
    let config = HostConfig::parse(&host_config(&[connection_text()], 1))
        .map_err(|error| error.to_string())?;
    config
        .connections()
        .first()
        .cloned()
        .ok_or_else(|| "connection fixture did not parse".to_owned())
}

fn connection_text() -> String {
    basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    )
}

fn sibling_text() -> String {
    basic_connection(
        "chainargos",
        202,
        "ChainArgos/java-monorepo",
        "chainargos",
        "chainargos-scale-set",
    )
}

fn test_state(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-host-retirement-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}
