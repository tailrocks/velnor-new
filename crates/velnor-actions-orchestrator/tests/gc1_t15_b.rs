// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_rejects_unknown_shared_context(root: &Path) -> Result<(), Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let home = root.join("velnor-check-9-4");
    let cwd = root.join("repo");
    fs::create_dir_all(&home)?;
    let qualified = qualified_at(&home, &cwd, tools.clone())?;
    for id in ["ghost", "../escape", "", ".", ".."] {
        if qualified.qualified_shared_tool_home(id).is_ok()
            || qualified.qualified_shared_prefix(id).is_ok()
            || qualified
                .qualified_shared_archive(id, &sha256_hex(b"archive-bytes"))
                .is_ok()
            || qualified.qualified_fetch_command(id, false, 0).is_ok()
        {
            return Err(fail(&format!("unknown tool admitted: {id}")));
        }
    }
    for digest in [
        "",
        "zz",
        &"a".repeat(63),
        &"a".repeat(65),
        &"A".repeat(64),
        "archive-bytes",
    ] {
        if qualified.qualified_shared_archive("mytool", digest).is_ok() {
            return Err(fail(&format!("invalid archive digest admitted: {digest}")));
        }
    }
    if QualifiedCheck::new(
        PathBuf::from("relative/home"),
        cwd.clone(),
        discovered(platform, tools),
        CheckCapabilityProof { container: None },
        &[],
        velnor_actions_mise::MISE_VERSION.to_owned(),
    )
    .is_ok()
    {
        return Err(fail("relative check home admitted"));
    }
    Ok(())
}

fn check_rejects_unverified_prefixes(root: &Path) -> Result<(), Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let tool = tools
        .iter()
        .find(|tool| tool.id == "mytool")
        .cloned()
        .ok_or_else(|| fail("fixture lost mytool"))?;
    let declared = tool.platforms[0].executables[0].clone();
    let probe_root = root.join("probe-reject");
    let bin = probe_root.join("bin");
    let cargo = probe_root.join("cargo");
    let rustup = probe_root.join("rustup");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&cargo)?;
    fs::create_dir_all(&rustup)?;
    let script = script_bytes();
    let path = stage_probe_script(&probe_root, &script)?;
    let homes = QualifiedProbeHomes {
        home: probe_root,
        bin_dir: bin,
        cargo_home: cargo,
        rust_home: rustup,
        compiler_toolchain: None,
    };
    let observed = QualifiedExecutableObservation {
        name: "mytool".to_owned(),
        path: path.clone(),
        sha256: sha256_hex(&script),
    };
    let deadline = CheckDeadline::after(Duration::from_secs(120))?;
    let proof =
        verify_qualified_executable(&tool, platform, &declared, &observed, &homes, deadline)?;
    let mut stdout_tampered = proof.clone();
    stdout_tampered.stdout.push_str("tampered");
    if validate_executable_proofs(&tool, platform, std::slice::from_ref(&stdout_tampered)).is_ok() {
        return Err(fail("stdout-tampered proof verified"));
    }
    let undeclared = QualifiedToolExecutable {
        name: "ghost".to_owned(),
        ..declared.clone()
    };
    let ghost_observed = QualifiedExecutableObservation {
        name: "ghost".to_owned(),
        ..observed.clone()
    };
    if verify_qualified_executable(
        &tool,
        platform,
        &undeclared,
        &ghost_observed,
        &homes,
        deadline,
    )
    .is_ok()
    {
        return Err(fail("undeclared executable verified"));
    }
    let other = match platform {
        CheckPlatform::LinuxX64 => CheckPlatform::MacosArm64,
        _ => CheckPlatform::LinuxX64,
    };
    if validate_executable_proofs(&tool, other, std::slice::from_ref(&proof)).is_ok() {
        return Err(fail("foreign platform proofs verified"));
    }
    let mut escaping = tool.clone();
    "../escape".clone_into(&mut escaping.id);
    if escaping.validate("qualified_tools", &escaping.id).is_ok() {
        return Err(fail("escaping tool identity validated"));
    }
    Ok(())
}

fn check_rejects_shared_root_conflicts(root: &Path) -> Result<(), Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let home = root.join("velnor-check-9-5");
    let cwd = root.join("repo");
    fs::create_dir_all(&home)?;
    let qualified = qualified_at(&home, &cwd, tools)?;
    let state = root.join("owned").join("state");
    let cargo = root.join("owned").join("cargo");
    let rustup = root.join("owned").join("rustup");
    if qualified
        .restored_home_overlay(&home, &cargo, &rustup, &root.join("owned").join("tools"))
        .is_ok()
        || qualified
            .restored_home_overlay(Path::new("owned/state"), &cargo, &rustup, &state)
            .is_ok()
        || qualified
            .restored_home_overlay(&state, &cargo, &cargo, &state)
            .is_ok()
    {
        return Err(fail("conflicting shared roots admitted"));
    }
    Ok(())
}

#[test]
fn gc1_t15_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let (_temp, root) = canonical_temp()?;
    check_rejects_unknown_shared_context(&root)?;
    check_rejects_unverified_prefixes(&root)?;
    check_rejects_shared_root_conflicts(&root)?;
    Ok(())
}
