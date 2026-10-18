use super::{ControlState, handle_request};
use crate::config::{HostConfig, RepositoryConnection};
use crate::config_test_support::{basic_connection, host_config};
use crate::controller_ipc::wire::Request;
use crate::journal::Journal;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

#[tokio::test]
async fn detached_is_provable_while_the_host_is_draining() -> Result<(), String> {
    let state_path = test_state("global-drain-detached");
    fs::create_dir_all(&state_path).map_err(|error| error.to_string())?;
    fs::create_dir_all(state_path.join("connection-drain")).map_err(|error| error.to_string())?;
    fs::write(state_path.join("drain"), TOKEN).map_err(|error| error.to_string())?;
    fs::write(state_path.join("connection-drain/essential"), TOKEN)
        .map_err(|error| error.to_string())?;

    let binding = test_binding()?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| format!("scope: {error}"))?;
    if !scope
        .retire_if_drained()
        .await
        .map_err(|error| format!("retire: {error}"))?
    {
        return Err("empty connection did not retire".to_owned());
    }
    let state = ControlState(Arc::new(RwLock::new(Vec::new())));
    let response = handle_request(
        &journal,
        &state,
        &state_path,
        Request::Detached {
            binding,
            token: TOKEN.to_owned(),
        },
    )
    .await
    .map_err(|error| format!("detach request: {error}"))?;
    if response.value != Some(true) {
        return Err("global drain incorrectly blocked exact detach proof".to_owned());
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())?;
    Ok(())
}

#[tokio::test]
async fn reactivation_retries_after_crash_before_config_persist() -> Result<(), String> {
    let state_path = test_state("reactivation-retry");
    fs::create_dir_all(state_path.join("connection-drain")).map_err(|error| error.to_string())?;
    fs::write(state_path.join("connection-drain/essential"), TOKEN)
        .map_err(|error| error.to_string())?;
    let binding = test_binding()?;
    let journal = Journal::open(&state_path.join("launch.db"))
        .await
        .map_err(|error| format!("open: {error}"))?;
    let scope = journal
        .connection_scope(&binding)
        .await
        .map_err(|error| format!("scope: {error}"))?;
    if !scope
        .retire_if_drained()
        .await
        .map_err(|error| format!("retire: {error}"))?
    {
        return Err("empty connection did not retire".to_owned());
    }
    let state = ControlState(Arc::new(RwLock::new(Vec::new())));
    let request = Request::Reactivate {
        binding,
        token: TOKEN.to_owned(),
    };
    for attempt in 0..2 {
        let response = handle_request(&journal, &state, &state_path, request.clone())
            .await
            .map_err(|error| format!("reactivate attempt {attempt}: {error}"))?;
        if response.value != Some(true) {
            return Err(format!("reactivate attempt {attempt} was not acknowledged"));
        }
    }
    fs::remove_dir_all(state_path).map_err(|error| error.to_string())?;
    Ok(())
}

fn test_binding() -> Result<RepositoryConnection, String> {
    let connection = basic_connection(
        "essential",
        101,
        "donbeave/essential-mac",
        "essential",
        "essential-scale-set",
    );
    let text = host_config(&[connection], 1);
    let config = HostConfig::parse(&text).map_err(|error| error.to_string())?;
    let binding = config
        .connections()
        .first()
        .cloned()
        .ok_or_else(|| "test config had no binding".to_owned())?;
    Ok(binding)
}

fn test_state(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "velnor-control-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}
