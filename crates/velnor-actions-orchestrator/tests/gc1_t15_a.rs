// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_commands_share_qualified_context(
    qualified: &QualifiedCheck,
    archive_sha: &str,
) -> Result<(), Box<dyn Error>> {
    let fetch = qualified.qualified_fetch_command("mytool", false, 0)?;
    if fetch.program() != OsStr::new("/usr/bin/curl") {
        return Err(fail("fetch program drifted"));
    }
    let private_home = qualified.qualified_tool_home("mytool")?;
    let download = private_home.join("downloads").join(archive_sha);
    let argv = fetch.argv();
    if !argv
        .iter()
        .any(|arg| arg.as_os_str() == download.as_os_str())
        || !argv.iter().any(|arg| {
            arg == "https://github.com/acme/mytool/releases/download/v1.2.3/mytool.tar.gz"
        })
    {
        return Err(fail("fetch command left its qualified home"));
    }
    let link = qualified.qualified_link_command("mytool")?;
    let link_argv = link.argv();
    let prefix = private_home.join("prefix");
    if !link_argv.iter().any(|arg| arg == "aqua:acme/mytool@1.2.3")
        || !link_argv
            .iter()
            .any(|arg| arg.as_os_str() == prefix.as_os_str())
    {
        return Err(fail("link command left its qualified prefix"));
    }
    if qualified.qualified_source_command("mytool").is_ok() {
        return Err(fail("binary tool admitted a source build"));
    }
    if qualified
        .qualified_fetch_command("srctool", true, 0)
        .is_ok()
    {
        return Err(fail("absent dependency artifact admitted"));
    }
    let source = qualified.qualified_source_command("srctool")?;
    let source_argv = source.argv();
    let private_src = qualified.qualified_tool_home("srctool")?;
    let root = source_argv
        .windows(2)
        .find(|pair| pair[0] == "--root")
        .map(|pair| pair[1].clone())
        .ok_or_else(|| fail("source build lost its root"))?;
    if root != private_src.join("prefix").into_os_string() {
        return Err(fail("source build left its qualified prefix"));
    }
    if !source_argv.iter().any(|arg| arg == "--offline") {
        return Err(fail("source build lost its offline guard"));
    }
    let env: std::collections::BTreeMap<_, _> = source.full_env().into_iter().collect();
    let rust_prefix = qualified.qualified_tool_home("rust")?.join("prefix");
    if env.get(&OsString::from("RUSTUP_TOOLCHAIN")) != Some(&rust_prefix.into_os_string())
        || env.get(&OsString::from("CARGO_NET_OFFLINE")) != Some(&OsString::from("true"))
    {
        return Err(fail("source environment lost its compiler binding"));
    }
    Ok(())
}

fn check_archives_rehash_before_origin(root: &Path, archive: &[u8]) -> Result<(), Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let home = root.join("velnor-check-9-2");
    let cwd = root.join("repo");
    fs::create_dir_all(&home)?;
    let qualified = qualified_at(&home, &cwd, tools)?;
    let declared = sha256_hex(archive);
    let stored = qualified.qualified_shared_archive("mytool", &declared)?;
    if let Some(parent) = stored.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&stored, archive)?;
    if sha256_hex(&fs::read(&stored)?) != declared {
        return Err(fail("verified archive bytes rejected"));
    }
    if sha256_hex(b"tampered archive bytes") == declared {
        return Err(fail("tampered archive bytes verified"));
    }
    Ok(())
}

pub(crate) fn stage_probe_script(dir: &Path, bytes: &[u8]) -> Result<PathBuf, Box<dyn Error>> {
    let script = dir.join("mytool");
    fs::write(&script, bytes)?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    Ok(script)
}

fn check_proofs_survive_hydration(root: &Path, script: &[u8]) -> Result<(), Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let tool = tools
        .iter()
        .find(|tool| tool.id == "mytool")
        .cloned()
        .ok_or_else(|| fail("fixture lost mytool"))?;
    let declared = tool.platforms[0].executables[0].clone();
    let probe_root = root.join("probe-home");
    let bin = probe_root.join("bin");
    let cargo = probe_root.join("cargo");
    let rustup = probe_root.join("rustup");
    fs::create_dir_all(&bin)?;
    fs::create_dir_all(&cargo)?;
    fs::create_dir_all(&rustup)?;
    let path = stage_probe_script(&probe_root, script)?;
    let homes = QualifiedProbeHomes {
        home: probe_root.clone(),
        bin_dir: bin,
        cargo_home: cargo,
        rust_home: rustup,
        compiler_toolchain: None,
    };
    let observed = QualifiedExecutableObservation {
        name: "mytool".to_owned(),
        path: path.clone(),
        sha256: sha256_hex(script),
    };
    let deadline = CheckDeadline::after(Duration::from_secs(120))?;
    let proof =
        verify_qualified_executable(&tool, platform, &declared, &observed, &homes, deadline)?;
    if proof.tool_id != "mytool"
        || proof.tool_version != TOOL_VERSION
        || proof.observed.path != path
    {
        return Err(fail("tool proof lost its identity"));
    }
    validate_executable_proofs(&tool, platform, std::slice::from_ref(&proof))?;
    if validate_executable_proofs(&tool, platform, &[]).is_ok() {
        return Err(fail("empty proofs verified"));
    }
    let mut tampered = proof.clone();
    tampered.observed.sha256.clone_from(&tree_placeholder(7));
    if validate_executable_proofs(&tool, platform, std::slice::from_ref(&tampered)).is_ok() {
        return Err(fail("tampered proof verified"));
    }
    let wrong_observed = QualifiedExecutableObservation {
        name: "mytool".to_owned(),
        path,
        sha256: tree_placeholder(7),
    };
    if verify_qualified_executable(
        &tool,
        platform,
        &declared,
        &wrong_observed,
        &homes,
        deadline,
    )
    .is_ok()
    {
        return Err(fail("tampered executable observed"));
    }
    Ok(())
}

fn check_source_identity_preserved() -> Result<(), Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let source = tools
        .iter()
        .find(|tool| tool.id == "srctool")
        .cloned()
        .ok_or_else(|| fail("fixture lost srctool"))?;
    let bytes = canonical_json_bytes(&source).map_err(io::Error::other)?;
    if !bytes
        .windows(SOURCE_LOCK.len())
        .any(|window| window == SOURCE_LOCK.as_bytes())
    {
        return Err(fail("source-build identity lost its lock"));
    }
    let binary = tools
        .iter()
        .find(|tool| tool.id == "mytool")
        .cloned()
        .ok_or_else(|| fail("fixture lost mytool"))?;
    if digest_b3(&bytes) == digest_b3(&canonical_json_bytes(&binary).map_err(io::Error::other)?) {
        return Err(fail("tool identities collided"));
    }
    Ok(())
}

fn check_teardown_keeps_shared_prefixes(root: &Path) -> Result<(), Box<dyn Error>> {
    let platform = host_platform()?;
    let (tools, _) = fixture_tools(platform)?;
    let home = root.join("velnor-check-9-3");
    let cwd = root.join("repo");
    fs::create_dir_all(&home)?;
    let qualified = qualified_at(&home, &cwd, tools)?;
    let state = root.join("owned").join("state");
    let cargo = root.join("owned").join("cargo");
    let rustup = root.join("owned").join("rustup");
    let tools_root = root.join("owned").join("tools");
    qualified.restored_home_overlay(&state, &cargo, &rustup, &tools_root)?;
    if qualified
        .restored_home_overlay(&state, &cargo, &rustup, &home.join("tools"))
        .is_ok()
    {
        return Err(fail("shared prefix inside a private home admitted"));
    }
    if qualified
        .restored_home_overlay(&home, &cargo, &rustup, &tools_root)
        .is_ok()
    {
        return Err(fail("private home inside shared state admitted"));
    }
    Ok(())
}

#[test]
fn gc1_t15_preserves_contract() -> Result<(), Box<dyn Error>> {
    let (_temp, root) = canonical_temp()?;
    let platform = host_platform()?;
    let (tools, archive) = fixture_tools(platform)?;
    let qualified = check_shared_paths_deterministic(&root)?;
    check_commands_share_qualified_context(&qualified, &sha256_hex(&archive))?;
    check_archives_rehash_before_origin(&root, &archive)?;
    check_proofs_survive_hydration(&root, &script_bytes())?;
    check_source_identity_preserved()?;
    check_teardown_keeps_shared_prefixes(&root)?;
    let _ = tools;
    Ok(())
}
