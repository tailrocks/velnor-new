// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn consumption_rejects() -> Result<(), Box<dyn Error>> {
    let file = ".velnor/config.toml";
    if native_check(
        "brew-tree",
        NativeHomebrewOperation::TapTree,
        Some(PackageReleaseChannel::Stable),
        hosted_linux(),
        5,
    )
    .validate(file, "native_checks.tree")
    .is_ok()
    {
        return Err(fail("consumption accepted a release channel"));
    }
    if native_check(
        "brew-adoption",
        NativeHomebrewOperation::AdoptionContract,
        Some(PackageReleaseChannel::Preview),
        hosted_linux(),
        5,
    )
    .validate(file, "native_checks.adoption")
    .is_ok()
    {
        return Err(fail("adoption accepted a release channel"));
    }
    if native_check(
        "brew-formula",
        NativeHomebrewOperation::FormulaContract,
        None,
        hosted_macos(),
        5,
    )
    .validate(file, "native_checks.formula")
    .is_ok()
    {
        return Err(fail("formula contract accepted a missing release channel"));
    }
    consumption_id_bounds(file)?;
    for timeout in [0, 6] {
        if native_check(
            "brew-tree",
            NativeHomebrewOperation::TapTree,
            None,
            hosted_linux(),
            timeout,
        )
        .validate(file, "native_checks.timeout")
        .is_ok()
        {
            return Err(fail("consumption accepted an out-of-window timeout"));
        }
    }
    if validate_task_id("internal/native-check/").is_ok()
        || validate_task_id("internal/native-check/a/b").is_ok()
        || validate_task_id("native-check/brew-install").is_ok()
    {
        return Err(fail("task identity accepted a malformed native id"));
    }
    Ok(())
}

fn observation_rejects() -> Result<(), Box<dyn Error>> {
    let context = digest_b3(b"gc1-t41-observation-root");
    if ObservationSession::begin("..", "brew-install", &new_session_id(), &context).is_ok() {
        return Err(fail("observation accepted a dotdot recipe"));
    }
    if ObservationSession::begin("a\0b", "brew-install", &new_session_id(), &context).is_ok() {
        return Err(fail("observation accepted a nul recipe"));
    }
    if ObservationSession::begin("install", "..", &new_session_id(), &context).is_ok() {
        return Err(fail("observation accepted a dotdot check"));
    }
    if ObservationSession::begin("install", "brew-install", "a/b", &context).is_ok() {
        return Err(fail("observation accepted a separated session"));
    }
    if ObservationSession::begin("install", "brew-install", "a\\b", &context).is_ok() {
        return Err(fail("observation accepted an escaped session"));
    }
    if ObservationSession::begin("install", "brew-install", &new_session_id(), "x\0y").is_ok() {
        return Err(fail("observation accepted a nul context"));
    }
    let session =
        ObservationSession::begin("install", "brew-install", &new_session_id(), &context)?;
    if session
        .compiler_evidence(Some(0), Some(0), Some(0), Some(0), Some(0), "")
        .is_ok()
    {
        return Err(fail("observation accepted an empty detail"));
    }
    if session
        .origin_evidence(Some(1), None, "gc1-t41 partial")
        .is_ok()
    {
        return Err(fail("observation accepted partial origin dimensions"));
    }
    if session.cache_evidence(Some(1), "").is_ok() {
        return Err(fail("observation accepted an empty cache detail"));
    }
    let mut incomplete =
        ObservationSession::begin("install", "brew-install", &new_session_id(), &context)?;
    let compiler = incomplete.compiler_evidence(
        Some(0),
        Some(0),
        Some(0),
        Some(0),
        Some(0),
        "gc1-t41 compiler",
    )?;
    incomplete.provide_compiler(compiler)?;
    let native =
        incomplete.native_evidence(Some(0), Some(0), Some(0), Some(0), "gc1-t41 native")?;
    incomplete.provide_native(native)?;
    let origin = incomplete.origin_evidence(Some(0), Some(0), "gc1-t41 origin")?;
    incomplete.provide_origin(origin)?;
    if incomplete.finish(ChildOutcome::Success).is_ok() {
        return Err(fail("observation finished without cache evidence"));
    }
    Ok(())
}

#[test]
fn gc1_t41_preserves_contract() -> Result<(), Box<dyn Error>> {
    operation_vocabulary()?;
    native_task_identity()?;
    install_path()?;
    measured_reuse_report()?;
    Ok(())
}

#[test]
fn gc1_t41_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    install_rejects()?;
    consumption_rejects()?;
    observation_rejects()?;
    Ok(())
}
