// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn reject_payload(
    setup: &MiseSetup,
    label: &str,
    runs_on: &str,
    target: &str,
    specs: &[&str],
    toolchain: Option<&str>,
    components: &[&str],
) -> Result<(), Box<dyn Error>> {
    if payload(setup, runs_on, target, specs, toolchain, components).is_ok() {
        return Err(fail(&format!("invalid host tool set accepted: {label}")));
    }
    Ok(())
}

pub(crate) fn check_rejects_bad_lanes_and_targets(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    reject_payload(
        setup,
        "empty lane",
        "",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "spaced lane",
        "ubuntu 24.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "bad target",
        "ubuntu-26.04",
        "not-a-target",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "empty specs",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &[],
        None,
        &[],
    )?;
    Ok(())
}

pub(crate) fn check_rejects_bad_specs(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    reject_payload(
        setup,
        "spec without version",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["gh"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "conflicting specs",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "gh@2.103.3"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "rust without toolchain",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        None,
        &[],
    )?;
    reject_payload(
        setup,
        "toolchain without rust",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        Some("1.99.1"),
        &[],
    )?;
    reject_payload(
        setup,
        "components without rust",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &["clippy"],
    )?;
    reject_payload(
        setup,
        "unknown component",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        Some("1.99.1"),
        &["cargo"],
    )?;
    Ok(())
}

pub(crate) fn check_rejects_decorated_rust_specs(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    let rows = [
        (
            "decorated rust without toolchain",
            "rust[profile=complete]@1.99.1",
        ),
        (
            "decorated rust with extra components",
            "rust[profile=minimal,components=rust-docs]@1.99.1",
        ),
        (
            "backend-qualified rust",
            "core:rust[profile=complete]@1.99.1",
        ),
        ("backend-qualified plain rust", "core:rust@1.99.1"),
    ];
    for (label, spec) in rows {
        reject_payload(
            setup,
            label,
            "ubuntu-26.04",
            "x86_64-unknown-linux-gnu",
            &[spec],
            None,
            &[],
        )?;
    }
    Ok(())
}

pub(crate) fn check_rejects_bad_setup_and_identity(
    setup: &MiseSetup,
) -> Result<(), Box<dyn Error>> {
    let mut bad_setup = mise_setup();
    "not-hex".clone_into(&mut bad_setup.sha256);
    reject_payload(
        &bad_setup,
        "bad mise sha",
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2"],
        None,
        &[],
    )?;
    let owned = base_payload(setup)?;
    if owned.key_for_runtime_identity("short").is_ok()
        || owned.key_for_runtime_identity(&"zz".repeat(32)).is_ok()
    {
        return Err(fail("invalid runtime identity accepted"));
    }
    let unsupported = payload(
        setup,
        "ubuntu-20.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )?;
    if unsupported.runtime_identity_supported() {
        return Err(fail("unqualified lane kept runtime identity"));
    }
    if unsupported.runtime_prelude_step().is_ok() {
        return Err(fail("unqualified lane emitted a prelude"));
    }
    Ok(())
}

#[test]
fn gc1_t13_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let setup = mise_setup();
    check_rejects_bad_lanes_and_targets(&setup)?;
    check_rejects_bad_specs(&setup)?;
    check_rejects_decorated_rust_specs(&setup)?;
    check_rejects_bad_setup_and_identity(&setup)?;
    Ok(())
}
