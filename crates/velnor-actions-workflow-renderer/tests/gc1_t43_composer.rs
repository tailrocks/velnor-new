// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_descriptor_binding() -> Result<(), Box<dyn Error>> {
    let first = pin()?;
    let again = pin()?;
    if first != again {
        return Err(fail("controller pin drifted between renders"));
    }
    if first.version != version() {
        return Err(fail("controller pin left its generator version"));
    }
    if first.target_triple != "x86_64-unknown-linux-gnu" {
        return Err(fail("controller pin left its runner target"));
    }
    require(&first.url, &artifact("x86_64-unknown-linux-gnu"), "pin url")?;
    if first.sha256.len() != 64 || first.commit != commit() {
        return Err(fail("controller pin left its release identity"));
    }
    Ok(())
}

pub(crate) fn check_composer_shape() -> Result<(), Box<dyn Error>> {
    let pin = pin()?;
    let staged = staged();
    let payload = "/tmp/velnor-gc1-t43-payload";
    let steps = provision_bootstrap_acquire_steps(
        &pin,
        &provenance(&pin),
        &acquire_argv(&staged),
        &bridge(payload, &staged),
    )?;
    if steps.len() != 2 {
        return Err(fail("bootstrap composer left its two-step handoff"));
    }
    let (bridge_run, bridge_env) = shell_of(&steps[0])?;
    if steps[0].name != BOOTSTRAP_STEP_NAME || steps[0].role.is_some() {
        return Err(fail("bridge step left its read-only identity"));
    }
    if bridge_env.get(BOOTSTRAP_MEMBER_ENV).map(String::as_str)
        != Some(format!("controller/velnor-actions-{}", version()).as_str())
    {
        return Err(fail("bridge left its exact member selection"));
    }
    if bridge_env
        .get(BOOTSTRAP_EXPECTED_SHA_ENV)
        .map(String::as_str)
        != Some(pin.sha256.as_str())
    {
        return Err(fail("bridge left its independent digest"));
    }
    if bridge_env.get(BOOTSTRAP_MAX_BYTES_ENV).map(String::as_str)
        != Some(BOOTSTRAP_MAX_BYTES.to_string().as_str())
    {
        return Err(fail("bridge left its size bound"));
    }
    let marker = format!("{staged}{BOOTSTRAP_MARKER_SUFFIX}");
    require(&bridge_run[2], &marker, "bridge marker")?;
    require(&bridge_run[2], "python3 -", "bridge interpreter")?;
    let (acquire_run, acquire_env) = shell_of(&steps[1])?;
    if steps[1].role != Some(StepRole::AcquireVelnor) {
        return Err(fail("guarded step left its acquire role"));
    }
    if acquire_env.get(ASSET_SHA_ENV).map(String::as_str) != Some(pin.sha256.as_str())
        || acquire_env.get(ASSET_URL_ENV).map(String::as_str) != Some(pin.url.as_str())
        || acquire_env.get(RELEASE_COMMIT_ENV).map(String::as_str) != Some(pin.commit.as_str())
    {
        return Err(fail("guarded step left its release provenance"));
    }
    require(
        &acquire_run[2],
        &format!("if [ -f \"{marker}\" ]; then exit 0;"),
        "warm skip guard",
    )?;
    require(&acquire_run[2], "$VELNOR_ASSET_URL", "cold miss download")?;
    require(&acquire_run[2], "sha256sum -c -", "cold miss digest check")?;
    require(&acquire_run[2], STAGED_BINARY_PREFIX, "staged binary")?;
    for (index, step) in steps.iter().enumerate() {
        let (run, _) = shell_of(step)?;
        for marker in ["actions/cache/save", "actions/cache/restore", "cache/save"] {
            forbid(&run[2], marker, &format!("bootstrap step {index}"))?;
        }
    }
    Ok(())
}

pub(crate) fn check_bridge_script_is_fixed() -> Result<(), Box<dyn Error>> {
    for marker in [
        "VELNOR_BOOTSTRAP_EXPECTED_SHA256",
        "VELNOR_BOOTSTRAP_MATCHED_KEY",
        "VELNOR_BOOTSTRAP_SHADOW_STATE",
        "compare_digest",
        "S_ISLNK",
        "duplicate_member",
        "member_oversized",
        "digest_mismatch",
        "cold_miss",
    ] {
        require(CACHE_BOOTSTRAP_SCRIPT, marker, "fixed bridge")?;
    }
    for forbidden in [
        "urllib",
        "http.client",
        "socket",
        "subprocess",
        "os.system",
        "os.exec",
        "os.popen",
        "eval(",
        "exec(",
        "compile(",
        "__import__",
        "shutil",
        "glob",
        "fnmatch",
        "actions/cache",
        "curl",
        "wget",
    ] {
        forbid(CACHE_BOOTSTRAP_SCRIPT, forbidden, "fixed bridge")?;
    }
    let mut imports: Vec<String> = CACHE_BOOTSTRAP_SCRIPT
        .lines()
        .filter_map(|line| {
            line.strip_prefix("import ")
                .or_else(|| {
                    line.strip_prefix("from ")
                        .and_then(|rest| rest.split(' ').next())
                })
                .map(str::to_owned)
        })
        .collect();
    imports.sort();
    imports.dedup();
    if imports != ["hashlib", "hmac", "os", "stat", "sys"] {
        return Err(fail("fixed bridge left its standard-library-only imports"));
    }
    Ok(())
}
