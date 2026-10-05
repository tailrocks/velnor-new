use super::*;

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_import_rejects_noncanonical_private_paths() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let lifecycle = steps()?;
    let store_link = fixture.stage("linked-store", "810004", true)?;
    let invalid_store = run_shell(
        &lifecycle.restore[1],
        &store_link,
        &restore_inputs("success", ""),
        &store_link.project,
        &store_link.temp,
        &[],
        "linked-store",
    )?;
    if invalid_store.success
        || !invalid_store
            .text
            .contains("primary MBX store identity is invalid")
    {
        return Err(format!(
            "generated import accepted a symlinked store: {}",
            invalid_store.text
        ));
    }

    let runner_link = fixture.stage("linked-runner", "810005", false)?;
    let alias = runner_link.root.join("alias");
    fs::create_dir(&alias).map_err(|err| err.to_string())?;
    symlink(&runner_link.runner_root, alias.join("velnor")).map_err(|err| err.to_string())?;
    let before = tree_hash(&runner_link.runner_root)?;
    let invalid_root = run_shell(
        &lifecycle.restore[1],
        &runner_link,
        &restore_inputs("success", ""),
        &runner_link.project,
        &alias,
        &[("RUNNER_TEMP", alias.as_os_str())],
        "linked-runner",
    )?;
    if invalid_root.success
        || !invalid_root
            .text
            .contains("MBX workspace or runner-temp parent is not canonical")
        || tree_hash(&runner_link.runner_root)? != before
    {
        return Err(format!(
            "generated import accepted or mutated a symlinked runner root: {}",
            invalid_root.text
        ));
    }

    let wrong = fixture.stage("wrong-identity", "810006", false)?;
    let before = tree_hash(&wrong.runner_root)?;
    let result = run_shell(
        &lifecycle.restore[1],
        &wrong,
        &restore_inputs("success", ""),
        &wrong.project,
        &wrong.temp,
        &[("GITHUB_RUN_ID", OsStr::new("999999"))],
        "wrong-run-id",
    )?;
    if result.success
        || !result.text.contains("MBX job root does not match this job")
        || tree_hash(&wrong.runner_root)? != before
    {
        return Err(format!(
            "generated import accepted a store outside its run identity: {}",
            result.text
        ));
    }
    Ok(())
}

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_preflight_rejects_symlink_temp_and_reserved_path_collisions() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let rustup = std::env::var_os("RUSTUP_HOME").ok_or("RUSTUP_HOME is unset")?;
    exercise_linked_runner_temp(&fixture, &rustup)?;
    exercise_preflight_root_collision(&fixture, &rustup)?;
    exercise_transport_lease_collision(&fixture, &rustup)?;
    exercise_pre_export_cleanup(&fixture, &steps()?)
}

fn exercise_linked_runner_temp(fixture: &Fixture, rustup: &OsStr) -> Result<(), String> {
    let linked = fixture.raw_stage("linked-temp", "810009")?;
    let alias = linked.root.join("temp-alias");
    symlink(&linked.temp, &alias).map_err(|err| err.to_string())?;
    let step = preflight_step(rustup, &linked.root.join("cargo"))?;
    let rejected = run_shell(
        &step,
        &linked,
        &BTreeMap::new(),
        &linked.project,
        &linked.temp,
        &[("RUNNER_TEMP", alias.as_os_str())],
        "linked-temp",
    )?;
    if rejected.success
        || !rejected.text.contains(
            "runner directories must be canonical, runner-owned, and not group/world writable",
        )
        || linked.runner_root.exists()
        || !env_pairs(&linked.env_file)?.is_empty()
        || !fs::read_to_string(&linked.github_path)
            .map_err(|err| err.to_string())?
            .is_empty()
    {
        return Err(format!(
            "preflight accepted a symlinked RUNNER_TEMP: {}",
            rejected.text
        ));
    }
    Ok(())
}

fn exercise_preflight_root_collision(fixture: &Fixture, rustup: &OsStr) -> Result<(), String> {
    let collision = fixture.raw_stage("preflight-collision", "810010")?;
    fs::create_dir(&collision.runner_root).map_err(|err| err.to_string())?;
    fs::set_permissions(&collision.runner_root, fs::Permissions::from_mode(0o700))
        .map_err(|err| err.to_string())?;
    fs::create_dir(&collision.job_root).map_err(|err| err.to_string())?;
    fs::set_permissions(&collision.job_root, fs::Permissions::from_mode(0o700))
        .map_err(|err| err.to_string())?;
    let sentinel = collision.job_root.join("sentinel");
    fs::write(&sentinel, b"reserved-path-owner").map_err(|err| err.to_string())?;
    let step = preflight_step(rustup, &collision.root.join("cargo"))?;
    let rejected = run_shell(
        &step,
        &collision,
        &BTreeMap::new(),
        &collision.project,
        &collision.temp,
        &[],
        "preflight-collision",
    )?;
    if rejected.success
        || !rejected.text.contains("MBX job root already exists")
        || fs::read(&sentinel).map_err(|err| err.to_string())? != b"reserved-path-owner"
        || !collision.job_root.exists()
        || collision.primary.exists()
        || collision.cold.exists()
        || collision.import_cwd.exists()
    {
        return Err(format!(
            "preflight did not fail closed on a reserved path: {}",
            rejected.text
        ));
    }
    Ok(())
}

fn exercise_transport_lease_collision(fixture: &Fixture, rustup: &OsStr) -> Result<(), String> {
    let lease_collision = fixture.raw_stage("transport-lease-collision", "810013")?;
    let lease = lease_collision.project.join(".velnor-mbx-cache.lock");
    fs::create_dir(&lease).map_err(|err| err.to_string())?;
    fs::set_permissions(&lease, fs::Permissions::from_mode(0o700))
        .map_err(|err| err.to_string())?;
    let lease_sentinel = lease.join("owner-sentinel");
    fs::write(&lease_sentinel, b"preserve existing lease owner").map_err(|err| err.to_string())?;
    let step = preflight_step(rustup, &lease_collision.root.join("cargo"))?;
    let rejected = run_shell(
        &step,
        &lease_collision,
        &BTreeMap::new(),
        &lease_collision.project,
        &lease_collision.temp,
        &[],
        "transport-lease-collision",
    )?;
    if rejected.success
        || fs::read(&lease_sentinel).map_err(|err| err.to_string())?
            != b"preserve existing lease owner"
        || !env_pairs(&lease_collision.env_file)?.is_empty()
        || !fs::read_to_string(&lease_collision.github_path)
            .map_err(|err| err.to_string())?
            .is_empty()
        || lease_collision.job_root.exists()
        || lease_collision.primary.exists()
        || lease_collision.bundle.exists()
    {
        return Err(format!(
            "preflight mutated an existing transport lease: {}",
            rejected.text
        ));
    }
    Ok(())
}

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_export_skips_empty_group() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let lifecycle = steps()?;
    let stage = fixture.stage("empty-export", "810007", false)?;
    let (miss, selected) = import_miss(&lifecycle, &stage)?;
    let export = run_shell(
        &lifecycle.save[0],
        &stage,
        &miss.outputs,
        &stage.project,
        &stage.temp,
        &[],
        "empty-export",
    )?;
    if !export.success
        || export.outputs.get("ready").map(String::as_str) != Some("false")
        || export.outputs.get("export_available").map(String::as_str) != Some("false")
        || selected.store != stage.primary
    {
        return Err(format!(
            "official CLI empty group did not skip save: {} {:?}",
            export.text, export.outputs
        ));
    }
    cleanup_after_export(&lifecycle, &stage, &export.outputs)?;
    Ok(())
}

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_export_hard_fails_before_save_on_wrong_identity() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let lifecycle = steps()?;
    let stage = fixture.stage("export-wrong-identity", "810011", false)?;
    let (miss, _) = import_miss(&lifecycle, &stage)?;
    let rejected = run_shell(
        &lifecycle.save[0],
        &stage,
        &miss.outputs,
        &stage.project,
        &stage.temp,
        &[("GITHUB_RUN_ID", OsStr::new("999999"))],
        "export-wrong-identity",
    )?;
    let save_condition = lifecycle.save[1]
        .condition
        .as_deref()
        .ok_or("cache save is missing its generated condition")?;
    if rejected.success
        || !rejected
            .text
            .contains("MBX job root does not match this job")
        || !rejected.outputs.is_empty()
        || stage.bundle.exists()
        || lifecycle.save[0].continue_on_error.is_some()
        || lifecycle.save[1].continue_on_error != Some(true)
        || !save_condition.contains("success()")
        || !save_condition.contains("outputs.ready == 'true'")
    {
        return Err(format!(
            "authority failure was softened or left save eligible: {} {:?}",
            rejected.text, rejected.outputs
        ));
    }
    Ok(())
}

#[test]
#[ignore = "selected by mandatory official MBX 1.21.1 generated-helper qualification"]
fn generated_export_failure_skips_cache_save() -> Result<(), String> {
    let fixture = Fixture::new()?;
    let lifecycle = steps()?;
    let stage = fixture.stage("unavailable-export", "810008", false)?;
    let (miss, _) = import_miss(&lifecycle, &stage)?;
    inject_enospc(&stage)?;
    let failure = run_shell(
        &lifecycle.save[0],
        &stage,
        &miss.outputs,
        &stage.project,
        &stage.temp,
        &[],
        "injected-enospc",
    )?;
    if !failure.success
        || failure.outputs.get("ready").map(String::as_str) != Some("false")
        || failure.outputs.get("export_available").map(String::as_str) != Some("false")
        || !failure.text.contains("injected: No space left on device")
    {
        return Err(format!(
            "generated export did not soft-skip injected ENOSPC: {} {:?}",
            failure.text, failure.outputs
        ));
    }
    cleanup_after_export(&lifecycle, &stage, &failure.outputs)?;
    if lifecycle.save[1]
        .condition
        .as_deref()
        .is_none_or(|condition| !condition.contains("outputs.ready == 'true'"))
    {
        return Err("cache save is not gated on a useful export after ENOSPC".to_owned());
    }
    eprintln!("ENOSPC is injected only for failure handling; all cache behavior uses official MBX");
    exercise_metadata_cancellation(&fixture, &lifecycle)?;
    Ok(())
}
