// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use serde_json::Value;
use sha2::{Digest, Sha256};

use velnor_actions_mise::cache_sources::{ByteCounter, SourceByteLedger};
use velnor_actions_mise::cache_transport::{
    SourceTransfer, SourceTransferDirection, SourceTransferOutcome, apply_source_transfer,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

#[path = "gc1_t14_support.rs"]
mod support;

fn script(step: &Value) -> Result<&str, Box<dyn Error>> {
    if step["id"] != "verify" {
        return Err(fail("verify step lost its id"));
    }
    step["run"]
        .as_str()
        .ok_or_else(|| fail("verify script missing"))
}

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

fn check_reuse_precedes_origin_fetch(script: &str) -> Result<(), Box<dyn Error>> {
    ordered(
        "reusing verified",
        "download failed",
        "mise archive reuse",
        script,
    )?;
    ordered(
        "archive does not match the mise.lock checksum",
        r#"cp "$archive" "$stored""#,
        "mise archive store",
        script,
    )?;
    Ok(())
}

fn check_trust_anchors_retained(script: &str) -> Result<(), Box<dyn Error>> {
    for marker in [
        "sha256:*) sum=${sum#sha256:}",
        "case \"$url\" in https://*)",
        "curl --proto '=https'",
        "VELNOR_TOOL_PINS",
        "\"$work/pins\"",
    ] {
        if !script.contains(marker) {
            return Err(fail(&format!("mise trust anchor lost: {marker}")));
        }
    }
    if script.contains("VELNOR_TOOL_CHECKSUM") {
        return Err(fail("mise trusts a candidate-supplied checksum"));
    }
    Ok(())
}

fn check_fetch_only_absent_or_invalid(script: &str) -> Result<(), Box<dyn Error>> {
    for marker in ["[ ! -L \"$stored\" ]", "rm -f \"$stored\""] {
        if !script.contains(marker) {
            return Err(fail(&format!("mise refetch guard lost: {marker}")));
        }
    }
    Ok(())
}

fn check_measurements_recorded_separately(
    action: &Value,
    script: &str,
) -> Result<(), Box<dyn Error>> {
    for marker in [
        "origin-bytes=%s",
        "origin-fetches=%s",
        "reused-archives=%s",
        "verified-files=%s",
    ] {
        if !script.contains(marker) {
            return Err(fail(&format!("mise measurement field lost: {marker}")));
        }
    }
    if !script.contains("[ -n \"$origin_bytes_unknown\" ] ||") {
        return Err(fail("unknown origin bytes not left nullable"));
    }
    let outputs = support::outputs()?;
    if action["id"] != "verify" {
        return Err(fail("verify step lost its id"));
    }
    for name in [
        "origin-bytes",
        "origin-fetches",
        "reused-archives",
        "verified-files",
    ] {
        if outputs[name]["value"] != format!("${{{{ steps.verify.outputs.{name} }}}}") {
            return Err(fail(&format!("restore output wiring lost: {name}")));
        }
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
    let action = support::step(&[], false)?;
    let script = script(&action)?;
    check_reuse_precedes_origin_fetch(script)?;
    check_trust_anchors_retained(script)?;
    check_fetch_only_absent_or_invalid(script)?;
    check_measurements_recorded_separately(&action, script)?;
    Ok(())
}

fn check_rejects_bad_archives_and_pins(script: &str) -> Result<(), Box<dyn Error>> {
    for reason in [
        "mise.lock checksum is not SHA-256",
        "mise.lock URL is not HTTPS",
        "archive does not match the mise.lock checksum",
        "a file differs from the pinned archive",
        "leaves the install",
        "special file",
    ] {
        rejects_on_line(script, reason, "return 1")?;
    }
    for pin in [
        "bad pin".to_owned(),
        format!(
            "tool 1.0.0 linux-x64 http://invalid/artifact.bin {}",
            "0".repeat(64)
        ),
        "tool 1.0.0 linux-x64 https://invalid/artifact.bin short".to_owned(),
    ] {
        support::reject_pin(&pin)?;
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

fn check_install_qualification() -> Result<(), Box<dyn Error>> {
    let trusted = b"trusted tool bytes\n";
    let sum: String = Sha256::digest(trusted)
        .iter()
        .flat_map(|byte| {
            [
                char::from(b"0123456789abcdef"[usize::from(byte >> 4)]),
                char::from(b"0123456789abcdef"[usize::from(byte & 15)]),
            ]
        })
        .collect();
    for (qualified, tampered) in [(false, false), (true, false), (true, true)] {
        let root = tempfile::tempdir()?;
        let installs = root.path().join(".local/share/mise/installs");
        let install = installs.join("tool/1.0.0");
        let alias = installs.join("tool/latest");
        std::fs::create_dir_all(&install)?;
        std::fs::write(
            install.join("tool"),
            if tampered {
                b"tampered\n".as_slice()
            } else {
                trusted
            },
        )?;
        std::os::unix::fs::symlink("1.0.0", &alias)?;
        let archives = installs.join(".velnor-verified-archives");
        std::fs::create_dir_all(&archives)?;
        std::fs::write(archives.join(&sum), trusted)?;
        let pins = if qualified {
            vec![format!(
                "tool 1.0.0 linux-x64 https://invalid/artifact.bin {sum}"
            )]
        } else {
            Vec::new()
        };
        let action = support::step(&pins, true)?;
        let output_path = root.path().join("outputs");
        let output = support::run(&action, root.path(), &output_path)?;
        if output.status.success() == (qualified && tampered) {
            return Err(fail(&format!(
                "qualification outcome wrong: qualified={qualified} tampered={tampered}: {}",
                String::from_utf8_lossy(&output.stdout)
            )));
        }
        let retained = qualified && !tampered;
        if install.exists() != retained || alias.symlink_metadata().is_ok() != retained {
            return Err(fail(
                "unverified install or alias survived, or verified install was discarded",
            ));
        }
        if retained && std::fs::read(install.join("tool"))? != trusted {
            return Err(fail("verified install changed"));
        }
        let reused = u8::from(qualified);
        let verified = u8::from(retained);
        let metrics = std::fs::read_to_string(output_path)?;
        for expected in [
            "origin-bytes=0".to_owned(),
            "origin-fetches=0".to_owned(),
            format!("reused-archives={reused}"),
            format!("verified-files={verified}"),
        ] {
            if !metrics.lines().any(|line| line == expected) {
                return Err(fail(&format!(
                    "qualification metrics lost {expected}: {metrics}"
                )));
            }
        }
    }
    Ok(())
}

#[test]
fn gc1_t14_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let action = support::step(&[], false)?;
    check_rejects_bad_archives_and_pins(script(&action)?)?;
    check_install_qualification()?;
    check_rejects_unmeasured_and_conflated_bytes()?;
    Ok(())
}
