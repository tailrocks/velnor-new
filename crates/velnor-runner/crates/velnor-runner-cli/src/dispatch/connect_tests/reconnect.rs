use super::*;

#[test]
fn connect_reactivates_an_exact_drained_binding_and_preserves_siblings() -> Result<(), String> {
    let dir = TempDir::new("reconnect-exact")?;
    let requested = request(dir.path());
    let original = connect_then_disconnect(&dir, &requested)?;
    let mut input = Cursor::new(b"reconnected-token\n");
    let stored = RefCell::new(false);
    connect_with_store_and_reactivate(
        &mut input,
        TEST_SERVICE,
        &requested,
        |_, _, _| {
            *stored.borrow_mut() = true;
            Ok(())
        },
        |state, connection| {
            if state != dir.path() || connection != &original {
                return Err(velnor_runner_host::HostError::Config);
            }
            Ok(true)
        },
    )
    .map_err(|error| error.to_string())?;

    let config = read_host_config(dir.path())?;
    let id = velnor_runner_host::ConnectionId::parse("example-repo")
        .map_err(|error| error.to_string())?;
    let sibling = velnor_runner_host::ConnectionId::parse("java-integration")
        .map_err(|error| error.to_string())?;
    if !*stored.borrow()
        || config.connection(&id) != Some(&original)
        || config.connection(&sibling).is_none()
        || drain_marker(dir.path(), &id).exists()
        || binding_snapshot(dir.path(), &id).exists()
        || dir
            .path()
            .join("connection-drain/example-repo~drained")
            .exists()
    {
        return Err("exact reconnect did not restore only the drained binding".to_owned());
    }
    Ok(())
}

#[test]
fn connect_recovers_after_config_persisted_but_marker_cleanup_did_not() -> Result<(), String> {
    let dir = TempDir::new("reconnect-crash-window")?;
    let requested = request(dir.path());
    let binding = connect_then_disconnect(&dir, &requested)?;
    let id = velnor_runner_host::ConnectionId::parse("example-repo")
        .map_err(|error| error.to_string())?;
    let token = marker_token(&drain_marker(dir.path(), &id))
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing drain token")?;
    write_reactivation_proof(dir.path(), &binding, &token).map_err(|error| format!("{error:?}"))?;

    let mut config = read_host_config(dir.path())?;
    config
        .add_connection(binding.clone())
        .map_err(|error| error.to_string())?;
    std::fs::write(
        dir.path().join("host.toml"),
        config.to_toml().map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let mut input = Cursor::new(b"retry-token\n");
    let reactivation_called = RefCell::new(false);
    connect_with_store_and_reactivate(
        &mut input,
        TEST_SERVICE,
        &requested,
        |_, _, _| Ok(()),
        |_, _| {
            *reactivation_called.borrow_mut() = true;
            Err(velnor_runner_host::HostError::Journal)
        },
    )
    .map_err(|error| error.to_string())?;
    if *reactivation_called.borrow()
        || drain_marker(dir.path(), &id).exists()
        || binding_snapshot(dir.path(), &id).exists()
        || dir
            .path()
            .join("connection-drain/example-repo~drained")
            .exists()
    {
        return Err("retry did not finish the previously approved reconnect".to_owned());
    }
    Ok(())
}

#[test]
fn connect_rejects_a_changed_binding_without_touching_retired_scope() -> Result<(), String> {
    let dir = TempDir::new("reconnect-changed")?;
    let mut original_request = request(dir.path());
    let original = connect_then_disconnect(&dir, &original_request)?;
    original_request.branches = vec!["release".to_owned()];
    let mut input = Cursor::new(b"changed-token\n");
    let stored = RefCell::new(false);
    let reactivated = RefCell::new(false);
    let result = connect_with_store_and_reactivate(
        &mut input,
        TEST_SERVICE,
        &original_request,
        |_, _, _| {
            *stored.borrow_mut() = true;
            Ok(())
        },
        |_, _| {
            *reactivated.borrow_mut() = true;
            Ok(true)
        },
    );
    let config = read_host_config(dir.path())?;
    let id = velnor_runner_host::ConnectionId::parse("example-repo")
        .map_err(|error| error.to_string())?;
    let sibling = velnor_runner_host::ConnectionId::parse("java-integration")
        .map_err(|error| error.to_string())?;
    let snapshot =
        std::fs::read(binding_snapshot(dir.path(), &id)).map_err(|error| error.to_string())?;
    let original_identity = original
        .canonical_identity_bytes()
        .map_err(|error| error.to_string())?;
    if !matches!(result, Err(ConnectError::Rejected))
        || *stored.borrow()
        || *reactivated.borrow()
        || config.connection(&id).is_some()
        || config.connection(&sibling).is_none()
        || !drain_marker(dir.path(), &id).is_file()
        || !binding_snapshot(dir.path(), &id).is_file()
        || snapshot != original_identity
    {
        return Err("changed reconnect altered or reactivated the retired binding".to_owned());
    }
    Ok(())
}

#[test]
fn connect_keeps_the_marker_when_host_cannot_reactivate() -> Result<(), String> {
    let dir = TempDir::new("reconnect-incomplete")?;
    let requested = request(dir.path());
    connect_then_disconnect(&dir, &requested)?;
    let mut input = Cursor::new(b"token\n");
    let stored = RefCell::new(false);
    let result = connect_with_store_and_reactivate(
        &mut input,
        TEST_SERVICE,
        &requested,
        |_, _, _| {
            *stored.borrow_mut() = true;
            Ok(())
        },
        |_, _| Ok(false),
    );
    let config = read_host_config(dir.path())?;
    let id = velnor_runner_host::ConnectionId::parse("example-repo")
        .map_err(|error| error.to_string())?;
    let sibling = velnor_runner_host::ConnectionId::parse("java-integration")
        .map_err(|error| error.to_string())?;
    if !matches!(result, Err(ConnectError::Rejected))
        || *stored.borrow()
        || config.connection(&id).is_some()
        || config.connection(&sibling).is_none()
        || !drain_marker(dir.path(), &id).is_file()
        || !binding_snapshot(dir.path(), &id).is_file()
    {
        return Err("failed reactivation opened admission or changed config".to_owned());
    }
    Ok(())
}

fn connect_then_disconnect(
    dir: &TempDir,
    requested: &ConnectRequest<'_>,
) -> Result<velnor_runner_host::RepositoryConnection, String> {
    let id = velnor_runner_host::ConnectionId::parse(requested.connection_id)
        .map_err(|error| error.to_string())?;
    let credential = velnor_runner_host::KeychainReference::parse(&format!(
        "keychain:{TEST_SERVICE}/{}",
        id.as_str()
    ))
    .map_err(|error| error.to_string())?;
    let original =
        connection_from(requested, id.clone(), credential).map_err(|error| error.to_string())?;
    let mut input = Cursor::new(b"initial-token\n");
    connect_with_store_and_reactivate(
        &mut input,
        TEST_SERVICE,
        requested,
        |_, _, _| Ok(()),
        |_, _| Err(velnor_runner_host::HostError::Journal),
    )
    .map_err(|error| error.to_string())?;
    let mut sibling = request(dir.path());
    sibling.connection_id = "java-integration";
    sibling.repo = "ChainArgos/java-monorepo";
    sibling.repository_id = 456;
    sibling.scale_set = "java-scale-set";
    sibling.scale_set_labels = vec!["velnor".to_owned(), "java-scale-set".to_owned()];
    sibling.branches = vec!["sea/integration".to_owned()];
    let mut sibling_input = Cursor::new(b"sibling-token\n");
    connect_with_store_and_reactivate(
        &mut sibling_input,
        TEST_SERVICE,
        &sibling,
        |_, _, _| Ok(()),
        |_, _| Err(velnor_runner_host::HostError::Journal),
    )
    .map_err(|error| error.to_string())?;
    let disconnected = disconnect_with_detach(
        dir.path(),
        id.as_str(),
        true,
        |_, _, _| Ok(true),
        |_, _, _| Ok(true),
    );
    if disconnected != std::process::ExitCode::SUCCESS {
        return Err("could not create a completed scoped disconnect".to_owned());
    }
    Ok(original)
}

fn read_host_config(state: &Path) -> Result<velnor_runner_host::HostConfig, String> {
    let text =
        std::fs::read_to_string(state.join("host.toml")).map_err(|error| error.to_string())?;
    velnor_runner_host::HostConfig::parse(&text).map_err(|error| error.to_string())
}

fn drain_marker(state: &Path, id: &velnor_runner_host::ConnectionId) -> PathBuf {
    state.join("connection-drain").join(id.as_str())
}

fn binding_snapshot(state: &Path, id: &velnor_runner_host::ConnectionId) -> PathBuf {
    state
        .join("connection-drain")
        .join(format!("{}~binding", id.as_str()))
}
