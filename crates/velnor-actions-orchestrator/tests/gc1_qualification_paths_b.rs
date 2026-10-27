// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn paths_family_invocation(
    event_id: &str,
    package_id: &str,
    role: &str,
    tool_kind: &str,
    doc_phase: Option<&str>,
    operation: &str,
) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: package_id.to_owned(),
        dependency_role: role.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: tool_kind.to_owned(),
        executable: format!("/tools/rust/prefix/bin/{tool_kind}"),
        argv: vec![
            tool_kind.to_owned(),
            "--crate-name".to_owned(),
            "family".to_owned(),
            "--edition=2021".to_owned(),
            "src/lib.rs".to_owned(),
        ],
        wrapper_chain: Vec::new(),
        doc_phase: doc_phase.map(str::to_owned),
        operation_kind: operation.to_owned(),
        cwd: "/repo".to_owned(),
    }
}

fn paths_family_success() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

fn require_full_family_count(dependency_compilations: u64) -> Result<(), Box<dyn Error>> {
    if dependency_compilations != 3 {
        return Err(invalid("command family ledger is incomplete"));
    }
    Ok(())
}

/// C19: rustdoc validates only with a supported documentation phase while
/// rustc and clippy-driver reject any phase; every actual family validates
/// through the same observation boundary.
#[expect(
    clippy::too_many_lines,
    reason = "family validation keeps every tool-kind boundary together"
)]
pub(crate) fn check_family_invocation_validation() -> Result<(), Box<dyn Error>> {
    for phase in DOC_PHASES {
        paths_family_invocation(
            "evt-doc-ok",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            Some(phase),
            OPERATION_COMPILATION,
        )
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    }
    must_fail(
        &paths_family_invocation(
            "evt-doc-bare",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            None,
            OPERATION_COMPILATION,
        )
        .validate(),
        "rustdoc without a documentation phase admitted",
    )?;
    must_fail(
        &paths_family_invocation(
            "evt-doc-bogus",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            Some("draft"),
            OPERATION_COMPILATION,
        )
        .validate(),
        "rustdoc with an unknown phase admitted",
    )?;
    must_fail(
        &paths_family_invocation(
            "evt-rustc-phase",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTC,
            Some(DOC_PHASE_COMPILATION),
            OPERATION_COMPILATION,
        )
        .validate(),
        "rustc with a documentation phase admitted",
    )?;
    must_fail(
        &paths_family_invocation(
            "evt-clippy-phase",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_CLIPPY_DRIVER,
            Some(DOC_PHASE_PROBE),
            OPERATION_COMPILATION,
        )
        .validate(),
        "clippy-driver with a documentation phase admitted",
    )?;
    paths_family_invocation(
        "evt-rustc-ok",
        LIB_ID,
        ROLE_DEPENDENCY,
        TOOL_KIND_RUSTC,
        None,
        OPERATION_COMPILATION,
    )
    .validate()
    .map_err(|error| invalid(&error.to_string()))?;
    paths_family_invocation(
        "evt-clippy-ok",
        LIB_ID,
        ROLE_DEPENDENCY,
        TOOL_KIND_CLIPPY_DRIVER,
        None,
        OPERATION_COMPILATION,
    )
    .validate()
    .map_err(|error| invalid(&error.to_string()))?;
    Ok(())
}

/// C19: a complete family ledger attributes rustc, clippy-driver, and rustdoc
/// dependency compilations plus the doctest descendant; a rustc-only ledger
/// never passes as the full family, and a rustdoc version query never counts
/// as documentation compilation.
#[expect(
    clippy::too_many_lines,
    reason = "family ledger keeps full, partial, and query ledgers together"
)]
pub(crate) fn check_family_ledger_completeness() -> Result<(), Box<dyn Error>> {
    let mut full = CompilerLedger::new();
    for (event_id, tool_kind, doc_phase, role) in [
        ("evt-fam-rustc", TOOL_KIND_RUSTC, None, ROLE_DEPENDENCY),
        (
            "evt-fam-clippy",
            TOOL_KIND_CLIPPY_DRIVER,
            None,
            ROLE_DEPENDENCY,
        ),
        (
            "evt-fam-rustdoc",
            TOOL_KIND_RUSTDOC,
            Some(DOC_PHASE_COMPILATION),
            ROLE_DEPENDENCY,
        ),
        (
            "evt-fam-doctest",
            TOOL_KIND_RUSTDOC,
            Some(DOC_PHASE_DOCTEST_DESCENDANT),
            ROLE_APPLICATION,
        ),
    ] {
        full.record(CompilerEvent {
            invocation: paths_family_invocation(
                event_id,
                LIB_ID,
                role,
                tool_kind,
                doc_phase,
                OPERATION_COMPILATION,
            ),
            completion: None,
        })?;
        full.complete(event_id, paths_family_success())?;
    }
    require_nonzero(
        "full family dependency compilations",
        full.dependency_compilations(),
    )?;
    require_nonzero(
        "full family application compilations",
        full.application_compilations(),
    )?;
    require_zero("full family queries", full.queries())?;
    if full.failed() != 0 {
        return Err(invalid("full family recorded failures"));
    }
    if full.dependency_compilations() != 3 {
        return Err(invalid("full family lost a command family"));
    }
    let mut partial = CompilerLedger::new();
    partial.record(CompilerEvent {
        invocation: paths_family_invocation(
            "evt-partial-rustc",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTC,
            None,
            OPERATION_COMPILATION,
        ),
        completion: None,
    })?;
    partial.complete("evt-partial-rustc", paths_family_success())?;
    if partial.dependency_compilations() == full.dependency_compilations() {
        return Err(invalid("rustc-only ledger passed as the full family"));
    }
    require_full_family_count(full.dependency_compilations())?;
    if partial.dependency_compilations() != 1 {
        return Err(invalid("rustc-only ledger lost its single compilation"));
    }
    must_fail(
        &require_full_family_count(partial.dependency_compilations()),
        "partial family manufactured full coverage",
    )?;
    let mut query = CompilerLedger::new();
    query.record(CompilerEvent {
        invocation: paths_family_invocation(
            "evt-doc-query",
            LIB_ID,
            ROLE_DEPENDENCY,
            TOOL_KIND_RUSTDOC,
            Some(DOC_PHASE_PROBE),
            OPERATION_COMPILER_QUERY,
        ),
        completion: None,
    })?;
    query.complete("evt-doc-query", paths_family_success())?;
    require_zero(
        "rustdoc query compilations",
        query.dependency_compilations(),
    )?;
    must_fail(
        &require_nonzero(
            "rustdoc query cold control",
            query.dependency_compilations(),
        ),
        "rustdoc version query satisfied the documentation control",
    )?;
    Ok(())
}

/// C20: a changed helper rebuilds because its full identity digest moves,
/// while an application-only change keeps coverage and reuses the unchanged
/// eligible dependencies.
pub(crate) fn check_changed_helper_rebuilds() -> Result<(), Box<dyn Error>> {
    let base = base_coverage()?;
    require_valid_digests(&base)?;
    let helper_bump = coverage_variant("0.1.0", "0.3.0", None, LINUX_TARGET, None, 'f')?;
    require_valid_digests(&helper_bump)?;
    require_changed_coverage(&base, &helper_bump)?;
    if base.coverage_digest == helper_bump.coverage_digest {
        return Err(invalid("changed helper kept the old coverage digest"));
    }
    if base.workspace_digest == helper_bump.workspace_digest {
        return Err(invalid("changed helper kept the old workspace digest"));
    }
    let app_only = coverage_variant("0.1.1", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_same_coverage(&base, &app_only)?;
    for member in eligible_ids() {
        if member != LIB_ID && member != SERDE_ID {
            return Err(invalid("eligible dependency set drifted"));
        }
    }
    if !eligible_ids().contains(&LIB_ID.to_owned()) {
        return Err(invalid("changed helper left the eligible set"));
    }
    Ok(())
}

/// C20: identical inputs reproduce identical coverage, so unchanged eligible
/// dependencies are reused instead of recompiled.
pub(crate) fn check_unchanged_inputs_reuse_eligible() -> Result<(), Box<dyn Error>> {
    let first = base_coverage()?;
    let second = coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, None, 'f')?;
    require_same_coverage(&first, &second)?;
    require_valid_digests(&second)?;
    Ok(())
}

fn paths_intent_draft() -> IntentDraft {
    IntentDraft {
        repository_id: 1,
        default_ref: "refs/heads/main".to_owned(),
        observed_head_sha: format!("{:040x}", 0xa5a5),
        configuration_digest: digest_b3_typed(b"gc1-qualification-paths-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
        derived_at: "2026-10-26T00:00:00Z".to_owned(),
        derivation_evidence_digest: digest_b3_typed(b"gc1-qualification-paths-evidence"),
    }
}

fn paths_intent_observation() -> IntentObservation {
    IntentObservation {
        observed_head_sha: format!("{:040x}", 0xa5a5),
        configuration_digest: digest_b3_typed(b"gc1-qualification-paths-config"),
        active_group_ids: vec!["group-a".to_owned()],
        active_coverage_ids: vec!["coverage-a".to_owned()],
    }
}

/// C20: only an unchanged current intent authorizes mutation; changed coverage
/// and a missing observation both refuse, so a signing or publication
/// authority never mutates cached state it did not verify.
pub(crate) fn check_authority_needs_unchanged_intent() -> Result<(), Box<dyn Error>> {
    let intent =
        derive_intent(paths_intent_draft()).map_err(|error| invalid(&error.to_string()))?;
    let unchanged = recheck_intent(&intent, Some(&paths_intent_observation()))
        .map_err(|error| invalid(&error.to_string()))?;
    if unchanged != IntentRecheck::CoverageUnchanged {
        return Err(invalid("unchanged intent lost its authorization"));
    }
    if !intent_authorizes_mutation(unchanged) {
        return Err(invalid("unchanged intent refused mutation"));
    }
    let mut changed_observation = paths_intent_observation();
    changed_observation.active_coverage_ids = vec!["coverage-b".to_owned()];
    let changed = recheck_intent(&intent, Some(&changed_observation))
        .map_err(|error| invalid(&error.to_string()))?;
    if changed != IntentRecheck::CoverageChanged {
        return Err(invalid("changed coverage kept its authorization"));
    }
    must_fail(
        &intent_authorizes_mutation(changed)
            .then_some(())
            .ok_or_else(|| invalid("changed coverage authorized mutation")),
        "changed coverage authorized mutation",
    )?;
    let blocked = recheck_intent(&intent, None).map_err(|error| invalid(&error.to_string()))?;
    if blocked != IntentRecheck::MutationBlocked {
        return Err(invalid("missing observation kept its authorization"));
    }
    must_fail(
        &intent_authorizes_mutation(blocked)
            .then_some(())
            .ok_or_else(|| invalid("missing observation authorized mutation")),
        "missing observation authorized mutation",
    )?;
    Ok(())
}
