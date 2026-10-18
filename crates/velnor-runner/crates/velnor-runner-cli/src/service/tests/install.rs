#[test]
fn service_start_requires_the_installed_state_argument() -> Result<(), String> {
    let scratch = test_scratch("velnor-service-start-test")?;
    let root = scratch.path();
    let state = root.join("state");
    let logs = root.join("logs");
    let other_state = root.join("other-state");
    let plist_path = root.join("agent.plist");
    let binary = Path::new("/opt/velnor-host");
    let result = write_plist(binary, &state, &plist_path, &logs);
    std::fs::create_dir_all(&other_state).map_err(|error| error.to_string())?;
    if !result
        || !plist_matches(&plist_path, binary, &state, &logs)
        || plist_matches(&plist_path, binary, &other_state, &logs)
    {
        return Err("service did not bind start to the installed state".to_owned());
    }
    let text = std::fs::read_to_string(&plist_path).map_err(|error| error.to_string())?;
    if !text.contains("<string>/opt/velnor-host</string>\n<string>--state</string><string>")
        || !text.contains("<string>daemon</string>\n<string>run</string>")
    {
        return Err("LaunchAgent arguments are not exact".to_owned());
    }
    Ok(())
}

#[test]
fn plist_install_does_not_follow_a_symlink() -> Result<(), String> {
    let scratch = test_scratch("velnor-service-link-test")?;
    let root = scratch.path();
    let state = root.join("state");
    let logs = root.join("logs");
    let target = root.join("target.plist");
    let path = root.join("agent.plist");
    std::fs::write(&target, b"keep this target").map_err(|error| error.to_string())?;
    std::os::unix::fs::symlink(&target, &path).map_err(|error| error.to_string())?;
    let written = write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs);
    let target_text = std::fs::read(&target).map_err(|error| error.to_string())?;
    let path_is_link = std::fs::symlink_metadata(&path)
        .map_err(|error| error.to_string())?
        .file_type()
        .is_symlink();
    if written || target_text != b"keep this target" || !path_is_link {
        return Err("plist install followed a symlink".to_owned());
    }
    Ok(())
}

#[test]
fn plist_install_preserves_a_different_existing_file() -> Result<(), String> {
    let scratch = test_scratch("velnor-service-conflict-test")?;
    let root = scratch.path();
    let state = root.join("state");
    let logs = root.join("logs");
    let path = root.join("agent.plist");
    let original = b"another LaunchAgent owns this path";
    std::fs::write(&path, original).map_err(|error| error.to_string())?;

    let written = write_plist(Path::new("/opt/velnor-host"), &state, &path, &logs);
    let after = std::fs::read(&path).map_err(|error| error.to_string())?;
    if written || after != original {
        return Err("install replaced a different LaunchAgent file".to_owned());
    }
    Ok(())
}

#[test]
fn plist_install_is_idempotent_for_its_exact_private_file() -> Result<(), String> {
    let scratch = test_scratch("velnor-service-idempotent-test")?;
    let root = scratch.path();
    let state = root.join("state");
    let logs = root.join("logs");
    let path = root.join("agent.plist");
    let binary = Path::new("/opt/velnor-host");
    if !write_plist(binary, &state, &path, &logs) {
        return Err("first install failed".to_owned());
    }
    let before = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    let installed = write_plist(binary, &state, &path, &logs);
    let after = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !installed || !same_file(&before, &after) || after.mode() & 0o777 != 0o600 {
        return Err("exact owned plist was not preserved idempotently".to_owned());
    }
    Ok(())
}

#[test]
fn plist_install_rejects_matching_content_with_unsafe_permissions() -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let scratch = test_scratch("velnor-service-mode-test")?;
    let root = scratch.path();
    let state = root.join("state");
    let logs = root.join("logs");
    let path = root.join("agent.plist");
    let binary = Path::new("/opt/velnor-host");
    if !write_plist(binary, &state, &path, &logs) {
        return Err("first install failed".to_owned());
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .map_err(|error| error.to_string())?;
    let unchanged = std::fs::read(&path).map_err(|error| error.to_string())?;
    let installed = write_plist(binary, &state, &path, &logs);
    let after = std::fs::read(&path).map_err(|error| error.to_string())?;
    if installed || after != unchanged {
        return Err("install accepted or changed a plist with unsafe permissions".to_owned());
    }
    Ok(())
}
