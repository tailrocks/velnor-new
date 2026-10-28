// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t09_preserves_contract() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let valid = context(temp.path())?;
    valid.validate()?;
    if valid.context_digest()? != valid.context_digest()? {
        return Err(io::Error::other("execution context digest drifted").into());
    }
    let bytes = valid.to_canonical_bytes()?;
    let roundtrip = ExecutionContextV1::parse_json(&String::from_utf8(bytes)?)?;
    roundtrip.validate()?;
    if roundtrip != valid
        || valid.selected_driver != CacheDriver::Cargo
        || valid.effective_cache_capability != CacheCapability::Write
        || !valid.trusted_default_writer
        || valid.mbx_target_views
    {
        return Err(io::Error::other("execution context contract drifted").into());
    }
    check_lane_separation(&valid)?;
    check_projection_preserves_task_bytes()?;
    check_child_command_reads_homes()?;
    check_lane_binding()?;
    Ok(())
}

fn reject_context(label: &str, valid: &ExecutionContextV1) -> Result<(), Box<dyn Error>> {
    if valid.validate().is_ok() {
        return Err(
            io::Error::other(format!("invalid execution context accepted: {label}")).into(),
        );
    }
    Ok(())
}

fn check_rejects_bad_context(root: &Path) -> Result<(), Box<dyn Error>> {
    let valid = context(root)?;
    let mut duplicated = valid.clone();
    duplicated.cargo_home.clone_from(&valid.rustup_home);
    reject_context("shared roots", &duplicated)?;
    let mut relative = valid.clone();
    "owned/gc1".clone_into(&mut relative.owned_root);
    reject_context("relative owned root", &relative)?;
    let mut empty_lanes = valid.clone();
    empty_lanes.lanes.clear();
    reject_context("empty lanes", &empty_lanes)?;
    let mut duplicate_lanes = valid.clone();
    duplicate_lanes.lanes.push(duplicate_lanes.lanes[0].clone());
    reject_context("duplicate lane id", &duplicate_lanes)?;
    let mut empty_adapters = valid.clone();
    empty_adapters.compiler_adapters.clear();
    reject_context("empty adapters", &empty_adapters)?;
    let mut duplicate_adapters = valid.clone();
    duplicate_adapters
        .compiler_adapters
        .push(duplicate_adapters.compiler_adapters[0].clone());
    reject_context("duplicate adapter path", &duplicate_adapters)?;
    let mut views = valid.clone();
    views.mbx_target_views = true;
    reject_context("mbx target views", &views)?;
    let mut bad_lane = valid.clone();
    "/absolute/target".clone_into(&mut bad_lane.lanes[0].target_dir);
    reject_context("absolute lane target", &bad_lane)?;
    Ok(())
}

fn check_rejects_bad_homes() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("home");
    let cwd = temp.path().join("repo");
    fs::create_dir_all(&home)?;
    fs::create_dir_all(&cwd)?;
    let qualified = qualified_at(&home, &cwd)?;
    let state = temp.path().join("owned/state/group");
    let cargo = temp.path().join("owned/cargo");
    let rustup = temp.path().join("owned/rustup");
    let tools = temp.path().join("owned/tools");
    if qualified
        .restored_home_overlay(&state, &cargo, &cargo, &tools)
        .is_ok()
    {
        return Err(io::Error::other("overlapping shared homes accepted").into());
    }
    if qualified
        .restored_home_overlay(Path::new("owned/state"), &cargo, &rustup, &tools)
        .is_ok()
    {
        return Err(io::Error::other("relative shared root accepted").into());
    }
    if qualified
        .restored_home_overlay(&home.join("state"), &cargo, &rustup, &tools)
        .is_ok()
    {
        return Err(io::Error::other("shared home inside private home accepted").into());
    }
    if qualified
        .restored_home_overlay(&state, &cwd, &rustup, &tools)
        .is_ok()
    {
        return Err(io::Error::other("shared home on check cwd accepted").into());
    }
    for lane in ["", ".", "..", "a/b", "a\\b"] {
        if qualified.lane_target_dir(&state, lane).is_ok() {
            return Err(io::Error::other(format!("invalid lane accepted: {lane}")).into());
        }
        if qualified.lane_build_dir(&state, lane).is_ok() {
            return Err(io::Error::other(format!("invalid lane accepted: {lane}")).into());
        }
    }
    if qualified
        .lane_target_dir(Path::new("owned/state"), "build")
        .is_ok()
    {
        return Err(io::Error::other("relative lane state accepted").into());
    }
    if QualifiedCheck::new(
        cwd.clone(),
        cwd.clone(),
        discovered(),
        CheckCapabilityProof { container: None },
        &[],
        velnor_actions_mise::MISE_VERSION.to_owned(),
    )
    .is_ok()
    {
        return Err(io::Error::other("overlapping check homes accepted").into());
    }
    Ok(())
}

#[test]
fn gc1_t09_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    check_rejects_bad_context(temp.path())?;
    check_rejects_bad_homes()?;
    Ok(())
}
