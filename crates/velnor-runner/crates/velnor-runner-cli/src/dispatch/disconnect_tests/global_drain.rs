use super::*;

#[test]
fn global_drain_wait_uses_the_custom_state_and_bounded_deadline() -> Result<(), String> {
    let dir = TempDir::new()?;
    let result = drain_with(dir.path(), true, |state, timeout| {
        if state == dir.path() && timeout == DRAIN_WAIT_BUDGET {
            Ok(true)
        } else {
            Err(HostError::Config)
        }
    });
    if result != std::process::ExitCode::SUCCESS || !dir.path().join("drain").is_file() {
        return Err("global drain did not persist or complete".to_owned());
    }
    Ok(())
}

#[test]
fn global_drain_timeout_keeps_the_marker_active() -> Result<(), String> {
    let dir = TempDir::new()?;
    let result = drain_with(dir.path(), true, |_, _| Ok(false));
    if result == std::process::ExitCode::SUCCESS || !dir.path().join("drain").is_file() {
        return Err("timed out drain cleared its marker".to_owned());
    }
    Ok(())
}

#[test]
fn global_drain_does_not_report_completion_after_resume() -> Result<(), String> {
    let dir = TempDir::new()?;
    let result = drain_with(dir.path(), true, |state, _| {
        if remove_flag(state, "drain") == std::process::ExitCode::SUCCESS {
            Ok(true)
        } else {
            Err(HostError::Journal)
        }
    });
    if result == std::process::ExitCode::SUCCESS || dir.path().join("drain").exists() {
        return Err("drain reported completion after resume".to_owned());
    }
    Ok(())
}

#[test]
fn old_global_drain_wait_does_not_complete_after_a_new_drain_generation() -> Result<(), String> {
    let dir = TempDir::new()?;
    let original = RefCell::<Option<MarkerToken>>::new(None);
    let result = drain_with(dir.path(), true, |state, _| {
        *original.borrow_mut() =
            marker_token(&state.join("drain")).map_err(|_| HostError::Config)?;
        if remove_flag(state, "drain") != std::process::ExitCode::SUCCESS {
            return Err(HostError::Journal);
        }
        if drain_with(state, false, |_, _| Err(HostError::Journal))
            != std::process::ExitCode::SUCCESS
        {
            return Err(HostError::Journal);
        }
        Ok(true)
    });
    let current = marker_token(&dir.path().join("drain")).map_err(|error| format!("{error:?}"))?;
    if result == std::process::ExitCode::SUCCESS
        || original.borrow().as_ref() == current.as_ref()
        || current.is_none()
    {
        return Err("an older drain waiter accepted a newer marker generation".to_owned());
    }
    Ok(())
}

#[test]
fn resume_clears_global_drain_and_keeps_connection_drain() -> Result<(), String> {
    let dir = TempDir::new()?;
    let id = connection_id("essential-mac")?;
    std::fs::create_dir_all(dir.path().join("connection-drain"))
        .map_err(|error| error.to_string())?;
    std::fs::write(dir.path().join("drain"), b"1").map_err(|error| error.to_string())?;
    std::fs::write(drain_marker(dir.path(), &id), b"1").map_err(|error| error.to_string())?;
    if remove_flag(dir.path(), "drain") != std::process::ExitCode::SUCCESS
        || dir.path().join("drain").exists()
        || !drain_marker(dir.path(), &id).is_file()
    {
        return Err("resume changed a connection drain".to_owned());
    }
    Ok(())
}

#[test]
fn resume_marker_result_distinguishes_removal_from_already_clear() -> Result<(), String> {
    let dir = TempDir::new()?;
    if remove_marker(dir.path(), "drain").map_err(|error| format!("{error:?}"))? {
        return Err("resume reported removing a missing marker".to_owned());
    }
    std::fs::write(dir.path().join("drain"), b"1").map_err(|error| error.to_string())?;
    if !remove_marker(dir.path(), "drain").map_err(|error| format!("{error:?}"))? {
        return Err("resume did not report removing an active marker".to_owned());
    }
    Ok(())
}
