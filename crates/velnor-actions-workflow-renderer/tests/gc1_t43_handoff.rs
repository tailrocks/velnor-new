// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_bridge_rejects_untrusted_payload() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = write_bridge_source(root.path())?;
    let payload = root.path().join("payload");
    std::fs::create_dir_all(payload.join("controller"))?;
    let member = format!("controller/velnor-actions-{}", version());
    let bytes = b"pinned-controller-bytes";
    std::fs::write(payload.join(&member), bytes)?;
    let sha = sha256_hex(bytes);
    let payload_str = payload.to_str().ok_or_else(|| fail("payload path"))?;
    let linked = member_env(
        payload_str,
        &member,
        &sha,
        root.path()
            .join("linked")
            .to_str()
            .ok_or_else(|| fail("linked path"))?,
        &[],
    );
    std::os::unix::fs::symlink(payload.join(&member), payload.join("controller/link"))?;
    let outcome = run_bridge_source(&source, &linked)?;
    if outcome.code != 1 {
        return Err(fail("linked payload left its rejection"));
    }
    std::fs::remove_file(payload.join("controller/link"))?;
    std::fs::create_dir_all(payload.join("other"))?;
    std::fs::write(
        payload
            .join("other")
            .join(format!("velnor-actions-{}", version())),
        bytes,
    )?;
    let duplicate = member_env(
        payload_str,
        &member,
        &sha,
        root.path()
            .join("duplicate")
            .to_str()
            .ok_or_else(|| fail("duplicate path"))?,
        &[],
    );
    let outcome = run_bridge_source(&source, &duplicate)?;
    if outcome.code != 1 || !outcome.stderr.contains("duplicate_member") {
        return Err(fail("duplicate member left its rejection"));
    }
    std::fs::remove_file(
        payload
            .join("other")
            .join(format!("velnor-actions-{}", version())),
    )?;
    let staged_tight = root.path().join("tight");
    let staged_tight_str = staged_tight.to_str().ok_or_else(|| fail("tight path"))?;
    let tight = member_env(
        payload_str,
        &member,
        &sha,
        staged_tight_str,
        &[("VELNOR_BOOTSTRAP_MAX_BYTES", "4")],
    );
    let outcome = run_bridge_source(&source, &tight)?;
    if outcome.code != 1 || !outcome.stderr.contains("member_oversized") {
        return Err(fail("oversized member left its rejection"));
    }
    if staged_tight.exists() {
        return Err(fail("rejected member reached its staged path"));
    }
    Ok(())
}

pub(crate) fn check_wrapper_executes_handoff() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let payload = root.path().join("payload");
    let run_root = root.path().join("run");
    std::fs::create_dir_all(payload.join("controller"))?;
    std::fs::create_dir_all(&run_root)?;
    let member = format!("controller/velnor-actions-{}", version());
    let bytes = b"pinned-controller-bytes";
    std::fs::write(payload.join(&member), bytes)?;
    let sha = sha256_hex(bytes);
    let staged = format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{}", version());
    let payload_str = payload.to_str().ok_or_else(|| fail("payload path"))?;
    let run_root_str = run_root.to_str().ok_or_else(|| fail("run root path"))?;
    let resolved_staged = run_root
        .join("velnor/bin")
        .join(format!("velnor-actions-{}", version()));
    let resolved_marker = format!(
        "{}{BOOTSTRAP_MARKER_SUFFIX}",
        resolved_staged
            .to_str()
            .ok_or_else(|| fail("marker path"))?
    );
    let over = BootstrapBridge {
        payload_dir: payload_str.to_owned(),
        member: member.clone(),
        staged_path: staged.clone(),
        max_bytes: BOOTSTRAP_MAX_BYTES,
        expected_key: None,
        matched_key: None,
        shadow_state: None,
    };
    let script = bootstrap_bridge_script(&over)?;
    let run_wrapper = |member: &str| {
        Command::new("sh")
            .arg("-c")
            .arg(&script)
            .env("RUNNER_TEMP", run_root_str)
            .env(BOOTSTRAP_MEMBER_ENV, member)
            .env(BOOTSTRAP_EXPECTED_SHA_ENV, &sha)
            .env(BOOTSTRAP_MAX_BYTES_ENV, BOOTSTRAP_MAX_BYTES.to_string())
            .output()
    };
    let output = run_wrapper(&member)?;
    if !output.status.success() {
        return Err(fail(&format!(
            "rendered wrapper refused warm payload:{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    if std::fs::read(&resolved_staged)? != bytes {
        return Err(fail("rendered wrapper staged unverified bytes"));
    }
    if !std::path::Path::new(&resolved_marker).is_file() {
        return Err(fail("rendered wrapper left its verified marker"));
    }
    let pin = pin()?;
    let steps =
        provision_bootstrap_acquire_steps(&pin, &provenance(&pin), &acquire_argv(&staged), &over)?;
    let (run, _) = shell_of(&steps[0])?;
    require(&run[2], &script, "emitted bridge step")?;
    let output = run_wrapper("controller/absent-binary")?;
    if !output.status.success() {
        return Err(fail("rendered wrapper failed its cold miss"));
    }
    if std::path::Path::new(&resolved_marker).exists() {
        return Err(fail("cold miss kept its verified marker"));
    }
    Ok(())
}

pub(crate) fn check_entry_key_wiring() -> Result<(), Box<dyn Error>> {
    let pin = pin()?;
    let staged = staged();
    let mut keyed = bridge("/tmp/velnor-gc1-t43-payload", &staged);
    keyed.expected_key = Some("mise-tools-v2-exact".to_owned());
    keyed.matched_key = Some("mise-tools-v2-exact".to_owned());
    keyed.shadow_state = Some(BOOTSTRAP_SHADOW_CLEAR.to_owned());
    let steps =
        provision_bootstrap_acquire_steps(&pin, &provenance(&pin), &acquire_argv(&staged), &keyed)?;
    let (_, env) = shell_of(&steps[0])?;
    if env.get(BOOTSTRAP_EXPECTED_KEY_ENV).map(String::as_str) != Some("mise-tools-v2-exact") {
        return Err(fail("bridge left its exact entry key"));
    }
    if env.get(BOOTSTRAP_MATCHED_KEY_ENV).map(String::as_str) != Some("mise-tools-v2-exact") {
        return Err(fail("bridge left its matched entry key"));
    }
    if env.get(BOOTSTRAP_SHADOW_ENV).map(String::as_str) != Some(BOOTSTRAP_SHADOW_CLEAR) {
        return Err(fail("bridge left its shadow state"));
    }
    Ok(())
}

pub(crate) fn check_provenance_preserved() -> Result<(), Box<dyn Error>> {
    let pin = pin()?;
    let staged = staged();
    let step = provision_acquire_step(&provenance(&pin), acquire_argv(&staged))?;
    if step.role != Some(StepRole::AcquireVelnor) {
        return Err(fail("acquire step left its role"));
    }
    let (run, env) = shell_of(&step)?;
    if env.get(ASSET_SHA_ENV).map(String::as_str) != Some(pin.sha256.as_str()) {
        return Err(fail("acquire step left its independent digest"));
    }
    require(&run[2], STAGED_BINARY_PREFIX, "staged binary")?;
    match provision_acquire_step(&HelperProvenance::SeedRequired, acquire_argv(&staged)) {
        Ok(_) => Err(fail("seed bootstrap accepted without a release")),
        Err(err) => require(&err.to_string(), SEED_REMEDIATION, "seed remediation"),
    }
}

#[test]
fn gc1_t43_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_descriptor_binding()?;
    check_composer_shape()?;
    check_bridge_script_is_fixed()?;
    check_entry_key_wiring()?;
    check_bridge_executes_contract()?;
    check_bridge_rejects_untrusted_payload()?;
    check_wrapper_executes_handoff()?;
    check_provenance_preserved()?;
    Ok(())
}
