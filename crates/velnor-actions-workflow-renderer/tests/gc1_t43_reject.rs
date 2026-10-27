// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn expect_bootstrap_error(
    pin: &BootstrapControllerPin,
    provenance: &HelperProvenance,
    argv: &[String],
    bridge: &BootstrapBridge,
    needle: &str,
) -> Result<(), Box<dyn Error>> {
    match provision_bootstrap_acquire_steps(pin, provenance, argv, bridge) {
        Ok(_) => Err(fail(&format!("invalid bootstrap accepted:{needle}"))),
        Err(err) => require(&err.to_string(), needle, "bootstrap"),
    }
}

pub(crate) fn check_rejects_bad_descriptor() -> Result<(), Box<dyn Error>> {
    match BootstrapControllerPin::from_release_manifest(&manifest(), "solaris", &version()) {
        Ok(_) => return Err(fail("unknown runner target accepted")),
        Err(err) => require(&err.to_string(), "bootstrap_runner_target", "target")?,
    }
    let off_version = manifest_for("0.0.0");
    match BootstrapControllerPin::from_release_manifest(&off_version, "ubuntu-24.04", "0.0.1") {
        Ok(_) => return Err(fail("descriptor version drift accepted")),
        Err(err) => require(&err.to_string(), "bootstrap_descriptor_version", "version")?,
    }
    match BootstrapControllerPin::from_release_manifest(&off_version, "ubuntu-24.04", "0.0.0") {
        Ok(_) => return Err(fail("foreign generator release accepted as controller")),
        Err(err) => require(&err.to_string(), "bootstrap_generator_version", "generator")?,
    }
    let mut missing = manifest();
    missing.targets.pop();
    if BootstrapControllerPin::from_release_manifest(&missing, "ubuntu-24.04", &version()).is_ok() {
        return Err(fail("incomplete release descriptor accepted"));
    }
    let mut tampered = manifest();
    "https://example.com/evil".clone_into(&mut tampered.targets[0].artifact);
    if BootstrapControllerPin::from_release_manifest(&tampered, "ubuntu-24.04", &version()).is_ok()
    {
        return Err(fail("unapproved controller origin accepted"));
    }
    Ok(())
}

pub(crate) fn check_rejects_pin_provenance_split() -> Result<(), Box<dyn Error>> {
    let pin = pin()?;
    let staged = staged();
    let payload = "/tmp/velnor-gc1-t43-payload";
    for altered in [
        HelperProvenance::ReleaseAsset {
            url: "https://github.com/tailrocks/velnor-new/releases/download/v0.0.0/evil".to_owned(),
            sha256: pin.sha256.clone(),
            commit: pin.commit.clone(),
        },
        HelperProvenance::ReleaseAsset {
            url: pin.url.clone(),
            sha256: "f".repeat(64),
            commit: pin.commit.clone(),
        },
        HelperProvenance::ReleaseAsset {
            url: pin.url.clone(),
            sha256: pin.sha256.clone(),
            commit: "e".repeat(40),
        },
    ] {
        expect_bootstrap_error(
            &pin,
            &altered,
            &acquire_argv(&staged),
            &bridge(payload, &staged),
            "bootstrap_pin_provenance_mismatch",
        )?;
    }
    expect_bootstrap_error(
        &pin,
        &HelperProvenance::SeedRequired,
        &acquire_argv(&staged),
        &bridge(payload, &staged),
        SEED_REMEDIATION,
    )?;
    Ok(())
}

pub(crate) fn check_rejects_bad_bridge() -> Result<(), Box<dyn Error>> {
    let pin = pin()?;
    let staged = staged();
    let provenance = provenance(&pin);
    let payload = "/tmp/velnor-gc1-t43-payload";
    let good = bridge(payload, &staged);
    let mut cases: Vec<(BootstrapBridge, &str)> = Vec::new();
    let mut traversal = good.clone();
    "../escape".clone_into(&mut traversal.member);
    cases.push((traversal, "bootstrap_member"));
    let mut absolute_member = good.clone();
    "/etc/passwd".clone_into(&mut absolute_member.member);
    cases.push((absolute_member, "bootstrap_member"));
    let mut glob = good.clone();
    "controller/*".clone_into(&mut glob.member);
    cases.push((glob, "bootstrap_member"));
    let mut relative_payload = good.clone();
    "relative/payload".clone_into(&mut relative_payload.payload_dir);
    cases.push((relative_payload, "bootstrap_payload_dir"));
    let mut relative_staged = good.clone();
    "relative/staged".clone_into(&mut relative_staged.staged_path);
    cases.push((relative_staged, "bootstrap_staged_path"));
    let mut substitution = good.clone();
    "$RUNNER_TEMP/$(evil)".clone_into(&mut substitution.staged_path);
    cases.push((substitution, "bootstrap_staged_path"));
    let mut unbounded = good.clone();
    unbounded.max_bytes = 0;
    cases.push((unbounded, "bootstrap_max_bytes"));
    let mut overflowing = good.clone();
    overflowing.max_bytes = u64::MAX;
    cases.push((overflowing, "bootstrap_max_bytes"));
    let mut dangling = good.clone();
    dangling.expected_key = Some("mise-tools-v2-exact".to_owned());
    dangling.matched_key = None;
    cases.push((dangling, "bootstrap_entry_key"));
    let mut blank_shadow = good.clone();
    blank_shadow.shadow_state = Some(String::new());
    cases.push((blank_shadow, "bootstrap_shadow_state"));
    for (candidate, needle) in &cases {
        expect_bootstrap_error(&pin, &provenance, &acquire_argv(&staged), candidate, needle)?;
    }
    match velnor_actions_workflow_renderer::steps::bootstrap_bridge_step(&good, "not-hex") {
        Ok(_) => return Err(fail("unapproved bridge digest accepted")),
        Err(err) => require(&err.to_string(), "bootstrap_sha256", "digest")?,
    }
    let foreign_argv = acquire_argv("/tmp/velnor-gc1-t43-elsewhere/binary");
    expect_bootstrap_error(
        &pin,
        &provenance,
        &foreign_argv,
        &good,
        "bootstrap_staged_mismatch",
    )?;
    expect_bootstrap_error(
        &pin,
        &provenance,
        &[format!("run {staged}")],
        &good,
        "bootstrap_acquire_shape",
    )?;
    Ok(())
}

pub(crate) fn check_bridge_self_validates() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let source = write_bridge_source(root.path())?;
    let run = |env: &[(&str, &str)]| {
        let owned: Vec<(String, String)> = env
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect();
        run_bridge_source(&source, &owned)
    };
    let outcome = run(&[
        (
            "VELNOR_BOOTSTRAP_PAYLOAD_DIR",
            "/tmp/velnor-gc1-t43-absent-payload",
        ),
        ("VELNOR_BOOTSTRAP_MEMBER", "controller/binary"),
        ("VELNOR_BOOTSTRAP_EXPECTED_SHA256", "not-hex"),
        ("VELNOR_BOOTSTRAP_STAGED_PATH", "/tmp/velnor-gc1-t43-staged"),
        ("VELNOR_BOOTSTRAP_MAX_BYTES", "67108864"),
    ])?;
    if outcome.code != 1 || !outcome.stderr.contains("expected_sha256") {
        return Err(fail("bridge left its digest self-check"));
    }
    let outcome = run(&[
        (
            "VELNOR_BOOTSTRAP_PAYLOAD_DIR",
            "/tmp/velnor-gc1-t43-absent-payload",
        ),
        ("VELNOR_BOOTSTRAP_MEMBER", "../escape"),
        ("VELNOR_BOOTSTRAP_EXPECTED_SHA256", &"0".repeat(64)),
        ("VELNOR_BOOTSTRAP_STAGED_PATH", "/tmp/velnor-gc1-t43-staged"),
        ("VELNOR_BOOTSTRAP_MAX_BYTES", "67108864"),
    ])?;
    if outcome.code != 1 || !outcome.stderr.contains("reject:member") {
        return Err(fail("bridge left its traversal self-check"));
    }
    let outcome = run(&[("VELNOR_BOOTSTRAP_MEMBER", "controller/binary")])?;
    if outcome.code != 1 {
        return Err(fail("bridge accepted its missing inputs"));
    }
    Ok(())
}

#[test]
fn gc1_t43_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_descriptor()?;
    check_rejects_pin_provenance_split()?;
    check_rejects_bad_bridge()?;
    check_bridge_self_validates()?;
    Ok(())
}
