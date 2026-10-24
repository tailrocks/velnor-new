use super::*;

#[test]
fn disconnect_reports_success_only_after_detach_and_retries_the_same_generation()
-> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let id = connection_id("essential-mac")?;
    let original = read_config(dir.path())?
        .connection(&id)
        .cloned()
        .ok_or("missing selected connection")?;
    let first = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        true,
        |_, _, _| Ok(true),
        |_, _, _| Ok(false),
    );
    let token = marker_token(&drain_marker(dir.path(), &id))
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing drain token")?;
    let config = read_config(dir.path())?;
    let sibling = connection_id("java-integration")?;
    if first == std::process::ExitCode::SUCCESS
        || config.connection(&id).is_some()
        || config.connection(&sibling).is_none()
        || !drain_proof(dir.path(), &id).is_file()
    {
        return Err("disconnect reported success before detach acknowledgement".to_owned());
    }

    let retried = disconnect_with_detach(
        dir.path(),
        id.as_str(),
        true,
        |_, _, _| Err(HostError::Journal),
        |state, binding, timeout| {
            if state != dir.path()
                || binding != &original
                || timeout != DRAIN_WAIT_BUDGET
                || marker_token(&drain_marker(state, &id)).ok().flatten() != Some(token.clone())
            {
                return Err(HostError::Config);
            }
            if read_config(state)
                .map_err(|_| HostError::Config)?
                .connection(&id)
                .is_some()
            {
                return Err(HostError::Config);
            }
            Ok(true)
        },
    );
    let config = read_config(dir.path())?;
    if retried != std::process::ExitCode::SUCCESS
        || config.connection(&id).is_some()
        || config.connection(&sibling).is_none()
        || marker_token(&drain_marker(dir.path(), &id)).ok().flatten() != Some(token)
    {
        return Err("detach retry changed its token or sibling connection".to_owned());
    }
    Ok(())
}

#[test]
fn disconnect_rejects_removed_config_without_drain_proof() -> Result<(), String> {
    let dir = TempDir::new()?;
    write_config(dir.path(), CONFIG)?;
    let first = disconnect_with_detach(
        dir.path(),
        "essential-mac",
        true,
        |_, _, _| Ok(false),
        |_, _, _| Err(HostError::Journal),
    );
    let id = connection_id("essential-mac")?;
    let token = marker_token(&drain_marker(dir.path(), &id))
        .map_err(|error| format!("{error:?}"))?
        .ok_or("missing drain token")?;
    let mut config = read_config(dir.path())?;
    config
        .remove_connection(&id)
        .map_err(|error| error.to_string())?;
    write_config(
        dir.path(),
        &config.to_toml().map_err(|error| error.to_string())?,
    )?;
    if first == std::process::ExitCode::SUCCESS || drain_proof(dir.path(), &id).exists() {
        return Err("incomplete drain wrote completion proof".to_owned());
    }
    let retried = disconnect_with_detach(
        dir.path(),
        id.as_str(),
        true,
        |_, _, _| Err(HostError::Journal),
        |_, _, _| Err(HostError::Journal),
    );
    let final_config = read_config(dir.path())?;
    let sibling = connection_id("java-integration")?;
    if retried == std::process::ExitCode::SUCCESS
        || final_config.connection(&sibling).is_none()
        || marker_token(&drain_marker(dir.path(), &id)).ok().flatten() != Some(token)
    {
        return Err("incomplete cleanup was reopened".to_owned());
    }
    Ok(())
}
