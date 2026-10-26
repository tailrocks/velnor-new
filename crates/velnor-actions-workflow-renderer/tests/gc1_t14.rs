// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_mise::cache_sources::{ByteCounter, SourceByteLedger};
use velnor_actions_mise::cache_transport::{
    SourceTransfer, SourceTransferDirection, SourceTransferOutcome, apply_source_transfer,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

const INSTALLS_VERIFY_SCRIPT: &str = include_str!("../src/cache_steps_tools_installs_verify.sh");
const RESTORE_COMPOSITION: &str = include_str!("../src/cache_steps_tools_restore.rs");
const RUST_TOOLCHAIN_TEMPLATE: &str =
    include_str!("../../velnor-actions-orchestrator/src/rust_toolchain_script.rs");

fn ordered(first: &str, second: &str, label: &str, script: &str) -> Result<(), Box<dyn Error>> {
    let before = script
        .find(first)
        .ok_or_else(|| fail(&format!("{label} marker missing: {first}")))?;
    let after = script
        .find(second)
        .ok_or_else(|| fail(&format!("{label} marker missing: {second}")))?;
    if before < after {
        Ok(())
    } else {
        Err(fail(&format!(
            "{label} order drifted: {first} not before {second}"
        )))
    }
}

fn rejects_on_line(script: &str, reason: &str, exit: &str) -> Result<(), Box<dyn Error>> {
    let line = script
        .lines()
        .find(|line| line.contains(reason))
        .ok_or_else(|| fail(&format!("rejection path missing: {reason}")))?;
    if line.contains(exit) {
        Ok(())
    } else {
        Err(fail(&format!("rejection escapes without {exit}: {reason}")))
    }
}

fn check_reuse_precedes_origin_fetch() -> Result<(), Box<dyn Error>> {
    ordered(
        "reusing verified",
        "download failed",
        "mise archive reuse",
        INSTALLS_VERIFY_SCRIPT,
    )?;
    ordered(
        "archive does not match the mise.lock checksum",
        r#"cp "$archive" "$stored""#,
        "mise archive store",
        INSTALLS_VERIFY_SCRIPT,
    )?;
    ordered(
        r#"cp \"$mstored\" \"$d/channel.toml\""#,
        r#"get \"$d/channel.toml\""#,
        "rust manifest reuse",
        RUST_TOOLCHAIN_TEMPLATE,
    )?;
    ordered(
        r#"cp \"$stored\" \"$d/c.tar.xz\""#,
        r#"get \"$d/c.tar.xz\""#,
        "rust tarball reuse",
        RUST_TOOLCHAIN_TEMPLATE,
    )?;
    Ok(())
}

fn check_trust_anchors_retained() -> Result<(), Box<dyn Error>> {
    for marker in [
        "sha256:*) sum=${sum#sha256:}",
        "case \"$url\" in https://*)",
        "curl --proto '=https'",
        "VELNOR_TOOL_PINS",
        "\"$work/pins\"",
    ] {
        if !INSTALLS_VERIFY_SCRIPT.contains(marker) {
            return Err(fail(&format!("mise trust anchor lost: {marker}")));
        }
    }
    if INSTALLS_VERIFY_SCRIPT.contains("VELNOR_TOOL_CHECKSUM") {
        return Err(fail("mise trusts a candidate-supplied checksum"));
    }
    for marker in [
        "https://static.rust-lang.org/dist/*.tar.xz",
        "rust_component_url_untrusted",
        "rust_channel_manifest_digest_mismatch",
        "rust_channel_manifest_not_catalogued",
        "rust_component_tarball_digest_mismatch",
    ] {
        if !RUST_TOOLCHAIN_TEMPLATE.contains(marker) {
            return Err(fail(&format!("rust trust anchor lost: {marker}")));
        }
    }
    Ok(())
}

fn check_fetch_only_absent_or_invalid() -> Result<(), Box<dyn Error>> {
    for marker in ["[ ! -L \"$stored\" ]", "rm -f \"$stored\""] {
        if !INSTALLS_VERIFY_SCRIPT.contains(marker) {
            return Err(fail(&format!("mise refetch guard lost: {marker}")));
        }
    }
    for marker in [
        r#"[ ! -L \"$mstored\" ]"#,
        r#"rm -f \"$mstored\""#,
        r#"rm -f \"$stored\""#,
    ] {
        if !RUST_TOOLCHAIN_TEMPLATE.contains(marker) {
            return Err(fail(&format!("rust refetch guard lost: {marker}")));
        }
    }
    Ok(())
}

fn check_measurements_recorded_separately() -> Result<(), Box<dyn Error>> {
    for marker in [
        "origin-bytes=%s",
        "origin-fetches=%s",
        "reused-archives=%s",
        "verified-files=%s",
    ] {
        if !INSTALLS_VERIFY_SCRIPT.contains(marker) {
            return Err(fail(&format!("mise measurement field lost: {marker}")));
        }
    }
    if !INSTALLS_VERIFY_SCRIPT.contains("[ -n \"$origin_bytes_unknown\" ] ||") {
        return Err(fail("unknown origin bytes not left nullable"));
    }
    for marker in [
        "steps.verify.outputs.origin-bytes",
        "steps.verify.outputs.origin-fetches",
        "steps.verify.outputs.reused-archives",
        "steps.verify.outputs.verified-files",
    ] {
        if !RESTORE_COMPOSITION.contains(marker) {
            return Err(fail(&format!("restore output wiring lost: {marker}")));
        }
    }
    if !RESTORE_COMPOSITION.contains("(\"id\".to_owned(), Yaml::str(\"verify\"))") {
        return Err(fail("verify step lost its id"));
    }
    for marker in [
        "origin-fetches=$ofetches",
        "reused-archives=$reused_archives",
        "verified-components=$vcomps",
    ] {
        if !RUST_TOOLCHAIN_TEMPLATE.contains(marker) {
            return Err(fail(&format!("rust measurement field lost: {marker}")));
        }
    }
    if !RUST_TOOLCHAIN_TEMPLATE.contains(r#"[ -n \"$obytes_unknown\" ] ||"#) {
        return Err(fail("unknown rust origin bytes not left nullable"));
    }
    let mut ledger = SourceByteLedger::new(
        ByteCounter::unknown("origin_acquisition_not_needed"),
        ByteCounter::measured(0),
    );
    ledger.validate()?;
    ledger.record_cache(46)?;
    if ledger.cache_bytes.value != Some(46) || ledger.origin_bytes.value.is_some() {
        return Err(fail("origin and cache bytes conflated"));
    }
    if ledger.record_origin(1).is_ok() {
        return Err(fail("unknown origin bytes admitted a write"));
    }
    let mut measured = SourceByteLedger::new(ByteCounter::measured(10), ByteCounter::measured(854));
    let origin = SourceTransfer {
        direction: SourceTransferDirection::OriginDownload,
        object_id: "tool-archive".to_owned(),
        transferred_bytes: 5,
        outcome: SourceTransferOutcome::Completed,
    };
    apply_source_transfer(&mut measured, &origin)?;
    if measured.origin_bytes.value != Some(15) || measured.cache_bytes.value != Some(854) {
        return Err(fail("origin download touched cache bytes"));
    }
    let failed = SourceTransfer {
        direction: SourceTransferDirection::OriginDownload,
        object_id: "tool-archive".to_owned(),
        transferred_bytes: 7,
        outcome: SourceTransferOutcome::Failed,
    };
    apply_source_transfer(&mut measured, &failed)?;
    if measured.origin_bytes.value != Some(15) || measured.cache_bytes.value != Some(854) {
        return Err(fail("failed transfer recorded bytes"));
    }
    Ok(())
}

#[test]
fn gc1_t14_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_reuse_precedes_origin_fetch()?;
    check_trust_anchors_retained()?;
    check_fetch_only_absent_or_invalid()?;
    check_measurements_recorded_separately()?;
    Ok(())
}

fn check_rejects_bad_archives_and_pins() -> Result<(), Box<dyn Error>> {
    for reason in [
        "no mise.lock or catalog pin",
        "mise.lock checksum is not SHA-256",
        "mise.lock URL is not HTTPS",
        "archive does not match the mise.lock checksum",
        "a file differs from the pinned archive",
        "leaves the install",
        "special file",
    ] {
        rejects_on_line(INSTALLS_VERIFY_SCRIPT, reason, "return 1")?;
    }
    for reason in [
        "rust_component_name_invalid",
        "rust_component_url_untrusted",
        "rust_channel_manifest_digest_mismatch",
        "rust_component_tarball_digest_mismatch",
        "rust_toolchain_bytes_mismatch",
        "{NOT_CATALOGUED_CODE}",
    ] {
        rejects_on_line(RUST_TOOLCHAIN_TEMPLATE, reason, "fail ")?;
    }
    if !RUST_TOOLCHAIN_TEMPLATE
        .contains("NOT_CATALOGUED_CODE: &str = \"rust_channel_manifest_not_catalogued\"")
    {
        return Err(fail("uncatalogued rust version lost its code"));
    }
    if !RESTORE_COMPOSITION.contains("bad_tool_pin") {
        return Err(fail("restore composition lost pin rejection"));
    }
    Ok(())
}

fn check_rejects_unmeasured_and_conflated_bytes() -> Result<(), Box<dyn Error>> {
    let mut unknown = ByteCounter::unknown("origin_acquisition_not_needed");
    if unknown.record(10).is_ok() {
        return Err(fail("unknown counter admitted a write"));
    }
    if unknown.value.is_some() {
        return Err(fail("unknown origin bytes zero-filled"));
    }
    let conflated = ByteCounter {
        value: Some(1),
        unknown_reason: Some("gap"),
    };
    if conflated.validate().is_ok() {
        return Err(fail("measured and unknown conflated"));
    }
    let absent = ByteCounter {
        value: None,
        unknown_reason: None,
    };
    if absent.validate().is_ok() {
        return Err(fail("absent counter admitted"));
    }
    let mut ledger = SourceByteLedger::new(ByteCounter::measured(0), ByteCounter::measured(0));
    let anonymous = SourceTransfer {
        direction: SourceTransferDirection::CacheTransfer,
        object_id: String::new(),
        transferred_bytes: 1,
        outcome: SourceTransferOutcome::Completed,
    };
    if apply_source_transfer(&mut ledger, &anonymous).is_ok() {
        return Err(fail("anonymous transfer applied"));
    }
    if ledger.cache_bytes.value != Some(0) {
        return Err(fail("rejected transfer mutated ledger"));
    }
    Ok(())
}

#[test]
fn gc1_t14_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_archives_and_pins()?;
    check_rejects_unmeasured_and_conflated_bytes()?;
    Ok(())
}
