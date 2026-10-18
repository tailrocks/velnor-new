#[test]
fn uninstall_keeps_the_plist_when_launchctl_stop_fails() -> Result<(), String> {
    let scratch = test_scratch("velnor-uninstall-test")?;
    let path = scratch.path().join("agent.plist");
    std::fs::write(&path, b"owned plist").map_err(|error| error.to_string())?;
    let result = uninstall_after_stop(ExitCode::from(1), &path, b"owned plist");
    let exists = path.is_file();
    if result != ExitCode::from(1) || !exists {
        return Err("uninstall hid a failed stop".to_owned());
    }
    Ok(())
}

#[test]
fn uninstall_keeps_a_foreign_replacement_after_successful_stop() -> Result<(), String> {
    let scratch = test_scratch("velnor-uninstall-foreign-test")?;
    let path = scratch.path().join("agent.plist");
    let expected = b"Velnor LaunchAgent configuration";
    let foreign = b"a different LaunchAgent replaced this file";
    std::fs::write(&path, foreign).map_err(|error| error.to_string())?;

    let result = uninstall_after_stop(ExitCode::SUCCESS, &path, expected);
    let after = std::fs::read(&path).map_err(|error| error.to_string())?;
    if result != ExitCode::from(1) || after != foreign {
        return Err("uninstall removed or accepted a foreign replacement".to_owned());
    }
    Ok(())
}

#[test]
fn uninstall_removes_only_an_exact_owned_plist() -> Result<(), String> {
    let scratch = test_scratch("velnor-uninstall-owned-test")?;
    let state = scratch.path().join("state");
    let logs = scratch.path().join("logs");
    let path = scratch.path().join("agent.plist");
    if !write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs) {
        return Err("owned plist fixture could not be written".to_owned());
    }
    let expected = std::fs::read(&path).map_err(|error| error.to_string())?;

    let result = uninstall_after_stop(ExitCode::SUCCESS, &path, &expected);
    if result != ExitCode::SUCCESS || !path_is_absent(&path) {
        return Err("uninstall did not remove the exact owned plist".to_owned());
    }
    Ok(())
}

#[test]
fn service_uid_comes_from_the_process_identity() {
    assert_eq!(
        super::read_uid(),
        (rustix::process::geteuid().as_raw() != 0).then_some(rustix::process::geteuid().as_raw())
    );
}

#[test]
fn uninstall_quarantines_a_replacement_arriving_after_preflight() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-uninstall-race-preflight")?;
    let state = scratch.path().join("state");
    let logs = scratch.path().join("logs");
    let path = scratch.path().join("agent.plist");
    if !write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs) {
        return Err("owned plist fixture could not be written".to_owned());
    }
    let expected = std::fs::read(&path).map_err(|error| error.to_string())?;
    let backup = scratch.path().join("retained-owned-plist");
    let foreign = b"foreign replacement after preflight";

    let result = super::uninstall_after_stop_with(
        ExitCode::SUCCESS,
        &path,
        &expected,
        || {
            std::fs::rename(&path, &backup).expect("retain the original owned entry");
            std::fs::write(&path, foreign).expect("write the foreign replacement");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("set replacement mode");
        },
        || {},
    );
    if result != ExitCode::from(1)
        || std::fs::read(&path).map_err(|error| error.to_string())? != foreign
        || std::fs::read(&backup).map_err(|error| error.to_string())? != expected
    {
        return Err("uninstall removed or overwrote the raced replacement".to_owned());
    }
    Ok(())
}

#[test]
fn uninstall_preserves_a_public_replacement_after_removing_its_owned_file() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-uninstall-race-after-quarantine")?;
    let state = scratch.path().join("state");
    let logs = scratch.path().join("logs");
    let path = scratch.path().join("agent.plist");
    if !write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs) {
        return Err("owned plist fixture could not be written".to_owned());
    }
    let expected = std::fs::read(&path).map_err(|error| error.to_string())?;
    let foreign = b"replacement installed while owned file is quarantined";

    let result = super::uninstall_after_stop_with(
        ExitCode::SUCCESS,
        &path,
        &expected,
        || {},
        || {
            std::fs::write(&path, foreign).expect("create concurrent replacement");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("set replacement mode");
        },
    );
    if result != ExitCode::from(1)
        || std::fs::read(&path).map_err(|error| error.to_string())? != foreign
    {
        return Err("uninstall removed a replacement at the public pathname".to_owned());
    }
    Ok(())
}

#[test]
fn uninstall_records_recovery_when_quarantine_cannot_be_restored() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-uninstall-recovery")?;
    let state = scratch.path().join("state");
    let logs = scratch.path().join("logs");
    let path = scratch.path().join("agent.plist");
    if !write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs) {
        return Err("owned plist fixture could not be written".to_owned());
    }
    let expected = std::fs::read(&path).map_err(|error| error.to_string())?;
    let backup = scratch.path().join("retained-owned-plist");
    let quarantined = b"unowned file moved to quarantine";
    let replacement = b"new public-path owner";

    let result = super::uninstall_after_stop_with(
        ExitCode::SUCCESS,
        &path,
        &expected,
        || {
            std::fs::rename(&path, &backup).expect("retain the owned plist");
            std::fs::write(&path, quarantined).expect("install first foreign entry");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("set first replacement mode");
        },
        || {
            std::fs::write(&path, replacement).expect("install second foreign entry");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .expect("set second replacement mode");
        },
    );
    let quarantine_dir = std::fs::read_dir(scratch.path())
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|entry| {
            entry.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .starts_with(".velnor-plist-quarantine-")
            })
        })
        .ok_or_else(|| "unrestorable quarantine was not retained".to_owned())?;
    let recovery =
        std::fs::read(quarantine_dir.join("entry")).map_err(|error| error.to_string())?;
    if result != ExitCode::from(1)
        || std::fs::read(&path).map_err(|error| error.to_string())? != replacement
        || recovery != quarantined
        || std::fs::read(backup).map_err(|error| error.to_string())? != expected
    {
        return Err("uninstall lost an unowned file or overwrote the new owner".to_owned());
    }
    Ok(())
}
