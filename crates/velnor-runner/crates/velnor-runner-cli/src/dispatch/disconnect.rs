use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use velnor_runner_host::{
    ConnectionId, HostError, RepositoryConnection, wait_for_connection_detached,
    wait_for_connection_drain,
};

use super::DRAIN_WAIT_BUDGET;
use super::connection_state::{
    admission_lock, config_lock, connection_drain_marker, connection_lifecycle_lock,
    has_drain_proof, persist_host, read_connection_snapshot, read_host, same_connection_identity,
    write_connection_snapshot, write_drain_proof,
};
use super::marker::{MarkerToken, marker_token, report_marker_write_error, write_marker};

pub(super) fn disconnect(state: &Path, raw_connection_id: &str, wait: bool) -> ExitCode {
    disconnect_with(state, raw_connection_id, wait, wait_for_connection_drain)
}

pub(super) fn disconnect_with<F>(
    state: &Path,
    raw_connection_id: &str,
    wait: bool,
    wait_for_drain: F,
) -> ExitCode
where
    F: FnOnce(&Path, &ConnectionId, Duration) -> Result<bool, HostError>,
{
    disconnect_with_detach(
        state,
        raw_connection_id,
        wait,
        wait_for_drain,
        wait_for_connection_detached,
    )
}

pub(super) fn disconnect_with_detach<F, G>(
    state: &Path,
    raw_connection_id: &str,
    wait: bool,
    wait_for_drain: F,
    wait_for_detach: G,
) -> ExitCode
where
    F: FnOnce(&Path, &ConnectionId, Duration) -> Result<bool, HostError>,
    G: FnOnce(&Path, &RepositoryConnection, Duration) -> Result<bool, HostError>,
{
    let connection_id = match ConnectionId::parse(raw_connection_id) {
        Ok(connection_id) => connection_id,
        Err(_) => {
            eprintln!("Connection ID is invalid.");
            return ExitCode::from(1);
        }
    };
    let Ok(_lifecycle_lock) = connection_lifecycle_lock(state, &connection_id) else {
        eprintln!("Connection is locked.");
        return ExitCode::from(1);
    };
    let Some((original_connection, token, drain_proven)) =
        request_connection_drain(state, &connection_id)
    else {
        return ExitCode::from(1);
    };
    if !wait {
        if drain_proven {
            println!("Connection remains draining.");
        } else {
            println!("Drain requested. Connection stays configured.");
        }
        return ExitCode::SUCCESS;
    }
    if !drain_proven {
        match wait_for_drain(state, &connection_id, DRAIN_WAIT_BUDGET) {
            Ok(true) => {}
            Ok(false) => {
                let Ok(_admission_lock) = admission_lock(state) else {
                    eprintln!("Admission state is locked.");
                    return ExitCode::from(1);
                };
                match marker_token(&connection_drain_marker(state, &connection_id)) {
                    Ok(Some(current)) if current == token => {
                        eprintln!("Drain timed out. Connection stays configured.");
                    }
                    Ok(_) => eprintln!("Connection drain changed before work finished."),
                    Err(_) => eprintln!("Connection drain state is uncertain."),
                }
                return ExitCode::from(1);
            }
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::from(1);
            }
        }
    }
    finish_disconnect(
        state,
        &connection_id,
        &original_connection,
        &token,
        wait_for_detach,
    )
}

fn request_connection_drain(
    state: &Path,
    connection_id: &ConnectionId,
) -> Option<(RepositoryConnection, MarkerToken, bool)> {
    let Ok(_lock) = config_lock(state) else {
        eprintln!("Config is locked.");
        return None;
    };
    let config = match read_host(state) {
        Ok(config) => config,
        Err(_) => {
            eprintln!("Config cannot be read.");
            return None;
        }
    };
    let Ok(_admission_lock) = admission_lock(state) else {
        eprintln!("Admission state is locked.");
        return None;
    };
    let marker_path = connection_drain_marker(state, connection_id);
    let marker = match marker_token(&marker_path) {
        Ok(marker) => marker,
        Err(_) => {
            eprintln!("Connection drain state is uncertain.");
            return None;
        }
    };
    let configured = config
        .as_ref()
        .and_then(|config| config.connection(connection_id))
        .cloned();
    if let Some(connection) = configured {
        if marker.is_some() {
            let snapshot = match read_connection_snapshot(state, connection_id) {
                Ok(Some(snapshot)) => snapshot,
                Ok(None) | Err(_) => {
                    eprintln!("Connection drain state is uncertain.");
                    return None;
                }
            };
            if !matches!(same_connection_identity(&snapshot, &connection), Ok(true)) {
                eprintln!("Connection changed while draining.");
                return None;
            }
        }
        if let Err(error) = write_connection_snapshot(state, connection_id, &connection) {
            report_marker_write_error(error);
            return None;
        }
        match write_marker(&marker_path) {
            Ok(token) => Some((connection, token, false)),
            Err(error) => {
                report_marker_write_error(error);
                None
            }
        }
    } else if let Some(token) = marker {
        let snapshot = match read_connection_snapshot(state, connection_id) {
            Ok(Some(snapshot)) => snapshot,
            Ok(None) | Err(_) => {
                eprintln!("Connection drain state is uncertain.");
                return None;
            }
        };
        if !matches!(
            has_drain_proof(state, connection_id, &snapshot, &token),
            Ok(true)
        ) {
            eprintln!("Connection drain completion is not proven.");
            return None;
        }
        Some((snapshot, token, true))
    } else {
        eprintln!("Connection not found.");
        None
    }
}

fn finish_disconnect(
    state: &Path,
    connection_id: &ConnectionId,
    original_connection: &RepositoryConnection,
    token: &MarkerToken,
    wait_for_detach: impl FnOnce(&Path, &RepositoryConnection, Duration) -> Result<bool, HostError>,
) -> ExitCode {
    if let Err(message) = remove_connection_config(state, connection_id, original_connection, token)
    {
        eprintln!("{message}");
        return ExitCode::from(1);
    }
    match wait_for_detach(state, original_connection, DRAIN_WAIT_BUDGET) {
        Ok(true) => {
            match verify_detached_disconnect(state, connection_id, original_connection, token) {
                Ok(()) => {
                    println!("Connection detached. The existing Scale Set stays.");
                    ExitCode::SUCCESS
                }
                Err(message) => {
                    eprintln!("{message}");
                    ExitCode::from(1)
                }
            }
        }
        Ok(false) => {
            report_detach_timeout(state, connection_id, token);
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn remove_connection_config(
    state: &Path,
    connection_id: &ConnectionId,
    original_connection: &RepositoryConnection,
    token: &MarkerToken,
) -> Result<(), &'static str> {
    let _config_lock = config_lock(state).map_err(|_| "Config is locked.")?;
    let _admission_lock = admission_lock(state).map_err(|_| "Admission state is locked.")?;
    verify_disconnect_marker(state, connection_id, original_connection, token)?;
    let mut config = match read_host(state).map_err(|_| "Config read failed.")? {
        Some(config) => config,
        None => {
            if has_drain_proof(state, connection_id, original_connection, token) != Ok(true) {
                return Err("Connection drain completion is not proven.");
            }
            return Ok(());
        }
    };
    let Some(configured) = config.connection(connection_id) else {
        if has_drain_proof(state, connection_id, original_connection, token) != Ok(true) {
            return Err("Connection drain completion is not proven.");
        }
        return Ok(());
    };
    if configured != original_connection {
        return Err("Connection changed while draining.");
    }
    write_drain_proof(state, original_connection, token)
        .map_err(|_| "Connection drain proof could not be saved.")?;
    if !config
        .remove_connection(connection_id)
        .map_err(|_| "Config update failed.")?
    {
        return Err("Connection is no longer configured.");
    }
    let text = config.to_toml().map_err(|_| "Config update failed.")?;
    persist_host(state, &text).map_err(|_| "Configuration update is uncertain. Examine host.toml.")
}

fn verify_disconnect_marker(
    state: &Path,
    connection_id: &ConnectionId,
    connection: &RepositoryConnection,
    token: &MarkerToken,
) -> Result<(), &'static str> {
    if marker_token(&connection_drain_marker(state, connection_id))
        .map_err(|_| "Connection drain state is uncertain.")?
        != Some(token.clone())
    {
        return Err("Connection drain changed before detach completed.");
    }
    let snapshot = read_connection_snapshot(state, connection_id)
        .map_err(|_| "Connection drain state is uncertain.")?
        .ok_or("Connection drain state is uncertain.")?;
    if !matches!(same_connection_identity(&snapshot, connection), Ok(true)) {
        return Err("Connection changed while draining.");
    }
    Ok(())
}

fn verify_detached_disconnect(
    state: &Path,
    connection_id: &ConnectionId,
    connection: &RepositoryConnection,
    token: &MarkerToken,
) -> Result<(), &'static str> {
    let _config_lock = config_lock(state).map_err(|_| "Config is locked.")?;
    let _admission_lock = admission_lock(state).map_err(|_| "Admission state is locked.")?;
    verify_disconnect_marker(state, connection_id, connection, token)?;
    if read_host(state)
        .map_err(|_| "Config read failed.")?
        .is_some_and(|config| config.connection(connection_id).is_some())
    {
        return Err("Connection remains configured.");
    }
    Ok(())
}

fn report_detach_timeout(state: &Path, connection_id: &ConnectionId, token: &MarkerToken) {
    let Ok(_admission_lock) = admission_lock(state) else {
        eprintln!("Admission state is locked.");
        return;
    };
    match marker_token(&connection_drain_marker(state, connection_id)) {
        Ok(Some(current)) if current == *token => {
            eprintln!("Connection detach timed out. The connection stays draining.");
        }
        Ok(_) => eprintln!("Connection drain changed before detach completed."),
        Err(_) => eprintln!("Connection drain state is uncertain."),
    }
}
