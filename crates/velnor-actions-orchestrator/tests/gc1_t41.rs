// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[expect(
    clippy::duplicate_mod,
    reason = "T41 pins the shared observation lifecycle alongside its owning task tests"
)]
#[path = "../src/compiler_observation.rs"]
mod compiler_observation;

use std::error::Error;
use std::io;

use self::compiler_observation::{
    ChildOutcome, ObservationReport, ObservationSession, new_session_id,
};
use velnor_actions_contract::config::{
    CheckExecutor, CheckPlatform, CheckRunner, NativeHomebrewCheck, NativeHomebrewOperation,
};
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_contract::{digest_b3, validate_task_id};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn hosted_macos() -> CheckRunner {
    CheckRunner {
        label: "macos-15".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::Hosted,
        container: None,
    }
}

fn hosted_linux() -> CheckRunner {
    CheckRunner {
        label: "ubuntu-24.04".to_owned(),
        platform: CheckPlatform::LinuxX64,
        executor: CheckExecutor::Hosted,
        container: None,
    }
}

fn ephemeral_macos() -> CheckRunner {
    CheckRunner {
        label: "custom-mac-runner".to_owned(),
        platform: CheckPlatform::MacosArm64,
        executor: CheckExecutor::EphemeralSelfHosted,
        container: None,
    }
}

fn native_check(
    id: &str,
    operation: NativeHomebrewOperation,
    channel: Option<PackageReleaseChannel>,
    runner: CheckRunner,
    timeout_minutes: u32,
) -> NativeHomebrewCheck {
    NativeHomebrewCheck {
        id: id.to_owned(),
        operation,
        formula_channel: channel,
        runner,
        timeout_minutes,
    }
}

fn operation_vocabulary() -> Result<(), Box<dyn Error>> {
    let observed = [
        (NativeHomebrewOperation::TapTree.as_str(), "tap_tree"),
        (
            NativeHomebrewOperation::FormulaContract.as_str(),
            "formula_contract",
        ),
        (NativeHomebrewOperation::Install.as_str(), "install"),
        (
            NativeHomebrewOperation::AdoptionContract.as_str(),
            "adoption_contract",
        ),
    ];
    for (actual, expected) in observed {
        if actual != expected {
            return Err(fail("native operation vocabulary drifted"));
        }
    }
    Ok(())
}

fn native_task_identity() -> Result<(), Box<dyn Error>> {
    validate_task_id("internal/native-check/brew-install")?;
    Ok(())
}

fn install_path() -> Result<(), Box<dyn Error>> {
    let file = ".velnor/config.toml";
    native_check(
        "brew-install",
        NativeHomebrewOperation::Install,
        Some(PackageReleaseChannel::Stable),
        hosted_macos(),
        10,
    )
    .validate(file, "native_checks.stable")?;
    native_check(
        "brew-install",
        NativeHomebrewOperation::Install,
        Some(PackageReleaseChannel::Preview),
        hosted_macos(),
        30,
    )
    .validate(file, "native_checks.preview")?;
    native_check(
        "brew-formula",
        NativeHomebrewOperation::FormulaContract,
        Some(PackageReleaseChannel::Stable),
        hosted_macos(),
        5,
    )
    .validate(file, "native_checks.formula")?;
    native_check(
        "brew-tree",
        NativeHomebrewOperation::TapTree,
        None,
        hosted_linux(),
        5,
    )
    .validate(file, "native_checks.tree")?;
    native_check(
        "brew-adoption",
        NativeHomebrewOperation::AdoptionContract,
        None,
        hosted_linux(),
        1,
    )
    .validate(file, "native_checks.adoption")?;
    Ok(())
}

fn reuse_evidence(session: &mut ObservationSession) -> Result<(), Box<dyn Error>> {
    let compiler = session.compiler_evidence(
        Some(0),
        Some(0),
        Some(2),
        Some(0),
        Some(0),
        "gc1-t41 compiler",
    )?;
    session.provide_compiler(compiler)?;
    let native = session.native_evidence(Some(0), Some(1), Some(0), Some(0), "gc1-t41 native")?;
    session.provide_native(native)?;
    let origin = session.origin_evidence(Some(4), Some(12345), "gc1-t41 origin")?;
    session.provide_origin(origin)?;
    let cache = session.cache_evidence(Some(6789), "gc1-t41 cache")?;
    session.provide_cache(cache)?;
    Ok(())
}

fn reuse_report(context: &str, outcome: ChildOutcome) -> Result<ObservationReport, Box<dyn Error>> {
    let mut session =
        ObservationSession::begin("install", "brew-install", &new_session_id(), context)?;
    reuse_evidence(&mut session)?;
    Ok(session.finish(outcome)?)
}

fn assert_measured_reuse(report: &ObservationReport, context: &str) -> Result<(), Box<dyn Error>> {
    if report.schema != 1 || report.child_failed {
        return Err(fail("reuse report misreported its outcome"));
    }
    if report.execution_context_digest != context {
        return Err(fail("reuse report lost its context"));
    }
    let expected: [(&str, u64); 7] = [
        ("dependency", 0),
        ("application", 0),
        ("queries", 2),
        ("native", 0),
        ("origin_acquisitions", 4),
        ("origin_bytes", 12345),
        ("cache_bytes", 6789),
    ];
    let actual = [
        report.dependency_compilations.value,
        report.application_compilations.value,
        report.compiler_queries.value,
        report.native_compilations.value,
        report.origin_acquisitions.value,
        report.origin_bytes.value,
        report.cache_bytes.value,
    ];
    for ((name, want), got) in expected.iter().zip(actual) {
        if got != Some(*want) {
            return Err(fail(&format!("reuse report lost measured {name}")));
        }
    }
    if !report.gaps.is_empty() {
        return Err(fail("measured reuse report kept unknown gaps"));
    }
    let bytes = report.to_canonical_json()?;
    let parsed: serde_json::Value = serde_json::from_slice(&bytes)?;
    if parsed.get("schema").and_then(serde_json::Value::as_u64) != Some(1)
        || parsed
            .pointer("/dependency_compilations/value")
            .and_then(serde_json::Value::as_u64)
            != Some(0)
    {
        return Err(fail("reuse report lost measured JSON"));
    }
    Ok(())
}

fn measured_reuse_report() -> Result<(), Box<dyn Error>> {
    let context = digest_b3(b"gc1-t41-measured-reuse");
    assert_measured_reuse(&reuse_report(&context, ChildOutcome::Success)?, &context)?;
    if !reuse_report(&context, ChildOutcome::Failure)?.child_failed {
        return Err(fail("reuse report dropped its failure"));
    }
    Ok(())
}

fn install_rejects() -> Result<(), Box<dyn Error>> {
    let file = ".velnor/config.toml";
    if native_check(
        "brew-install",
        NativeHomebrewOperation::Install,
        Some(PackageReleaseChannel::Stable),
        hosted_linux(),
        10,
    )
    .validate(file, "native_checks.linux")
    .is_ok()
    {
        return Err(fail("install accepted a non-macOS host"));
    }
    if native_check(
        "brew-install",
        NativeHomebrewOperation::Install,
        None,
        hosted_macos(),
        10,
    )
    .validate(file, "native_checks.channel")
    .is_ok()
    {
        return Err(fail("install accepted a missing release channel"));
    }
    if native_check(
        "brew-install",
        NativeHomebrewOperation::Install,
        Some(PackageReleaseChannel::Stable),
        ephemeral_macos(),
        10,
    )
    .validate(file, "native_checks.executor")
    .is_ok()
    {
        return Err(fail("install accepted a non-hosted runner"));
    }
    for timeout in [0, 31, 360] {
        if native_check(
            "brew-install",
            NativeHomebrewOperation::Install,
            Some(PackageReleaseChannel::Stable),
            hosted_macos(),
            timeout,
        )
        .validate(file, "native_checks.timeout")
        .is_ok()
        {
            return Err(fail("install accepted an out-of-window timeout"));
        }
    }
    Ok(())
}

fn consumption_id_bounds(file: &str) -> Result<(), Box<dyn Error>> {
    for id in ["", "has space", &"a".repeat(129)] {
        if native_check(
            id,
            NativeHomebrewOperation::TapTree,
            None,
            hosted_linux(),
            5,
        )
        .validate(file, "native_checks.id")
        .is_ok()
        {
            return Err(fail("consumption accepted a bad check id"));
        }
    }
    if native_check(
        &"a".repeat(128),
        NativeHomebrewOperation::TapTree,
        None,
        hosted_linux(),
        5,
    )
    .validate(file, "native_checks.id")
    .is_err()
    {
        return Err(fail("consumption rejected a max-length check id"));
    }
    Ok(())
}

#[path = "gc1_t41_a.rs"]
mod gc1_t41_a;
