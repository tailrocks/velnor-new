// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_contract::{StepId, StepRole};
use velnor_actions_workflow_renderer::cache_p08::{ToolsCacheInputs, ToolsCachePayload};
use velnor_actions_workflow_renderer::setup::{MISE_BINARY_SHA256_LINUX_X64, MiseSetup};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn mise_setup() -> MiseSetup {
    MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: velnor_actions_mise::MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    }
}

fn payload(
    setup: &MiseSetup,
    runs_on: &str,
    target: &str,
    specs: &[&str],
    toolchain: Option<&str>,
    components: &[&str],
) -> Result<ToolsCachePayload, Box<dyn Error>> {
    let tool_specs: Vec<String> = specs.iter().map(|spec| (*spec).to_owned()).collect();
    let rustup_components: Vec<String> = components
        .iter()
        .map(|component| (*component).to_owned())
        .collect();
    Ok(ToolsCachePayload::new(ToolsCacheInputs {
        runs_on,
        target,
        mise_setup: setup,
        tool_specs: &tool_specs,
        rustup_toolchain: toolchain,
        rustup_components: &rustup_components,
    })?)
}

fn base_payload(setup: &MiseSetup) -> Result<ToolsCachePayload, Box<dyn Error>> {
    payload(
        setup,
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )
}

fn check_union_and_determinism(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    let base = base_payload(setup)?;
    let same = base_payload(setup)?;
    if base.static_digest() != same.static_digest()
        || base.identity_input() != same.identity_input()
    {
        return Err(fail("host tool set digest drifted"));
    }
    let shuffled = payload(
        setup,
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["opentofu@1.13.2", "gh@2.103.2", "gh@2.103.2"],
        None,
        &[],
    )?;
    if shuffled.static_digest() != base.static_digest()
        || shuffled.identity_input() != base.identity_input()
    {
        return Err(fail("host tool set union kept order or duplicates"));
    }
    if !base.runtime_identity_supported() {
        return Err(fail("supported host lane lost runtime identity"));
    }
    if base.key_expression() != "mise-tools-v2-${{steps.v2.outputs.identity}}" {
        return Err(fail("host tool set key left the shared expression"));
    }
    let key = base.key_for_runtime_identity(&"ab".repeat(32))?;
    if key != format!("mise-tools-v2-{}", "ab".repeat(32)) {
        return Err(fail("host tool set key rejected its runtime identity"));
    }
    if base.paths().is_empty() || !base.paths().contains(&"~/.local/share/mise".to_owned()) {
        return Err(fail("host tool set dropped an approved path"));
    }
    Ok(())
}

fn check_host_distinctions_survive(setup: &MiseSetup) -> Result<(), Box<dyn Error>> {
    let base = base_payload(setup)?;
    let other_lane = payload(
        setup,
        "scale-set:velnor+my-set",
        "x86_64-unknown-linux-gnu",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )?;
    if other_lane.static_digest() == base.static_digest() {
        return Err(fail("host distinction collapsed across lanes"));
    }
    let other_target = payload(
        setup,
        "ubuntu-26.04",
        "aarch64-apple-darwin",
        &["gh@2.103.2", "opentofu@1.13.2"],
        None,
        &[],
    )?;
    if other_target.static_digest() == base.static_digest() {
        return Err(fail("host distinction collapsed across targets"));
    }
    if other_target.runtime_identity_supported() {
        return Err(fail("unsupported target kept runtime identity"));
    }
    let clippy = payload(
        setup,
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        Some("1.99.1"),
        &["clippy"],
    )?;
    let full = payload(
        setup,
        "ubuntu-26.04",
        "x86_64-unknown-linux-gnu",
        &["rust@1.99.1"],
        Some("1.99.1"),
        &["clippy", "rustfmt"],
    )?;
    if clippy.static_digest() == full.static_digest() {
        return Err(fail("component distinction collapsed"));
    }
    clippy.restore_step()?;
    full.restore_step()?;
    let restore = base.restore_step()?;
    if restore.role != Some(StepRole::ToolsCacheRestore) || restore.id != Some(StepId::ToolsRestore)
    {
        return Err(fail("restore step lost its tools identity"));
    }
    let prelude = base.runtime_prelude_step()?;
    if prelude.role != Some(StepRole::ToolsCacheIdentity)
        || prelude.id != Some(StepId::ToolsCacheIdentity)
    {
        return Err(fail("prelude step lost its tools identity"));
    }
    Ok(())
}

#[test]
fn gc1_t13_preserves_contract() -> Result<(), Box<dyn Error>> {
    let setup = mise_setup();
    setup.validate()?;
    check_union_and_determinism(&setup)?;
    check_host_distinctions_survive(&setup)?;
    Ok(())
}

#[test]
fn tools_identity_separates_rust_install_profiles() -> Result<(), Box<dyn Error>> {
    let setup = mise_setup();
    let digest = |spec: &str| -> Result<String, Box<dyn Error>> {
        Ok(payload(
            &setup,
            "ubuntu-26.04",
            "x86_64-unknown-linux-gnu",
            &[spec],
            Some("1.99.1"),
            &["clippy", "rustfmt"],
        )?
        .static_digest()
        .to_owned())
    };
    let minimal = digest("rust[profile=minimal]@1.99.1")?;
    let unset = digest("rust@1.99.1")?;
    if minimal == unset {
        return Err(fail("install profile did not reach the tools identity"));
    }
    if minimal != digest("rust[profile=minimal]@1.99.1")? {
        return Err(fail("identical inputs gave different identities"));
    }
    Ok(())
}

const PRE_X44_RUST_STATIC_DIGEST: &str =
    "26cf85558bce297155044b6da2e856e4c97870f7fa4ede071b119b4128d96d81";

#[test]
fn tools_identity_never_matches_a_pre_x44_rust_cache_entry() -> Result<(), Box<dyn Error>> {
    let setup = mise_setup();
    let current = || {
        payload(
            &setup,
            "ubuntu-26.04",
            "x86_64-unknown-linux-gnu",
            &["rust[profile=minimal]@1.99.1"],
            Some("1.99.1"),
            &["clippy", "rustfmt"],
        )
    };
    let digest = current()?.static_digest().to_owned();
    if digest == PRE_X44_RUST_STATIC_DIGEST {
        return Err(fail("a pre-X44 cache entry keeps the current identity"));
    }
    if current()?
        .identity_input()
        .contains(PRE_X44_RUST_STATIC_DIGEST)
    {
        return Err(fail("the identity input still carries the pre-X44 digest"));
    }
    if digest != current()?.static_digest() {
        return Err(fail("identical inputs gave different identities"));
    }
    Ok(())
}

#[path = "gc1_t13_reject.rs"]
mod gc1_t13_reject;
