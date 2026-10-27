// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn check_native_unknown_never_zero() -> Result<(), Box<dyn Error>> {
    let mut ledger = native_ledger()?;
    ledger.record_unobserved_absolute("/usr/bin/cc")?;
    if ledger.measured_native_compilations().is_some() {
        return Err(fail("unobserved native calls measured as zero"));
    }
    let mut session = valid_session()?;
    let native = session.native_evidence(
        None,
        None,
        None,
        None,
        "one absolute native call bypassed the adapters",
    )?;
    session.provide_native(native)?;
    let compiler = session.compiler_evidence(None, None, None, None, None, "detail")?;
    session.provide_compiler(compiler)?;
    let origin = session.origin_evidence(None, None, "detail")?;
    session.provide_origin(origin)?;
    let cache = session.cache_evidence(None, "detail")?;
    session.provide_cache(cache)?;
    let report = session.finish(ChildOutcome::Success)?;
    if report.native_compilations.value.is_some() {
        return Err(fail("unknown native work passed as a value"));
    }
    if report.native_compilations
        != unknowns_with_code(&report, ObservationGapCode::UnknownNativeCoverage)?
    {
        return Err(fail("native unknown lost its gap"));
    }
    Ok(())
}

fn unknowns_with_code(
    report: &ObservationReport,
    code: ObservationGapCode,
) -> Result<MeasuredU64, Box<dyn Error>> {
    for gap in &report.gaps {
        if gap.code == code {
            return Ok(MeasuredU64 {
                value: None,
                unknown_reason: Some(gap.clone()),
            });
        }
    }
    Err(fail("expected observation gap missing"))
}

fn check_unknown_dimensions_stay_nullable() -> Result<(), Box<dyn Error>> {
    let mut session = valid_session()?;
    let compiler = session.compiler_evidence(
        None,
        None,
        None,
        None,
        None,
        "rust compiler invocations unattributed on this execution path",
    )?;
    session.provide_compiler(compiler)?;
    let native = session.native_evidence(
        None,
        None,
        None,
        None,
        "native compiler calls unobserved on this execution path",
    )?;
    session.provide_native(native)?;
    let origin = session.origin_evidence(
        None,
        None,
        "origin fetch bytes unobserved on this execution path",
    )?;
    session.provide_origin(origin)?;
    let cache = session.cache_evidence(
        None,
        "cache-transfer bytes unattributed on this execution path",
    )?;
    session.provide_cache(cache)?;
    let report = session.finish(ChildOutcome::Failure)?;
    for measured in [
        &report.dependency_compilations,
        &report.application_compilations,
        &report.compiler_queries,
        &report.compiler_failed_calls,
        &report.compiler_interrupted_calls,
        &report.native_compilations,
        &report.native_queries,
        &report.native_failed_calls,
        &report.native_interrupted_calls,
        &report.origin_acquisitions,
        &report.origin_bytes,
        &report.cache_bytes,
    ] {
        if measured.value.is_some() {
            return Err(fail("unobserved dimension passed as a value"));
        }
        measured
            .validate()
            .map_err(|error| fail(&error.to_string()))?;
        if measured.unknown_reason.is_none() {
            return Err(fail("unknown dimension lost its reason"));
        }
    }
    if report.gaps.len() != 4 {
        return Err(fail("unknown dimensions collapsed into shared gaps"));
    }
    Ok(())
}

fn check_rejects_unmeasurable_values() -> Result<(), Box<dyn Error>> {
    let gap = ObservationGapV1 {
        code: ObservationGapCode::IncompleteAttribution,
        detail: "detail".to_owned(),
        affected_recipe_ids: vec!["internal/lint/dev".to_owned()],
        evidence_ref: "session".to_owned(),
    };
    if (MeasuredU64 {
        value: Some(1),
        unknown_reason: Some(gap),
    })
    .validate()
    .is_ok()
    {
        return Err(fail("measured value with an unknown reason accepted"));
    }
    if (MeasuredU64 {
        value: None,
        unknown_reason: None,
    })
    .validate()
    .is_ok()
    {
        return Err(fail("absent value without a reason accepted"));
    }
    let mut session = valid_session()?;
    let compiler = session.compiler_evidence(None, None, None, None, None, "detail")?;
    session.provide_compiler(compiler)?;
    let native = session.native_evidence(None, None, None, None, "detail")?;
    session.provide_native(native)?;
    let origin = session.origin_evidence(None, None, "detail")?;
    session.provide_origin(origin)?;
    let cache = session.cache_evidence(None, "detail")?;
    session.provide_cache(cache)?;
    let report = session.finish(ChildOutcome::Success)?;
    let bytes = report
        .to_canonical_json()
        .map_err(|error| fail(&error.to_string()))?;
    let text = String::from_utf8(bytes).map_err(|error| fail(&error.to_string()))?;
    let tampered = text.replacen("\"schema\":1", "\"schema\":2", 1);
    if tampered == text {
        return Err(fail("canonical report hid its schema"));
    }
    let parsed: ObservationReport =
        serde_json::from_slice(tampered.as_bytes()).map_err(|error| fail(&error.to_string()))?;
    if parsed.validate().is_ok() {
        return Err(fail("unsupported report schema accepted"));
    }
    Ok(())
}

#[test]
fn gc1_t19_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_bindings()?;
    let session = valid_session()?;
    check_rejects_partial_dimensions(&session)?;
    check_rejects_foreign_and_duplicate_evidence()?;
    check_native_unknown_never_zero()?;
    check_unknown_dimensions_stay_nullable()?;
    check_rejects_unmeasurable_values()?;
    Ok(())
}
