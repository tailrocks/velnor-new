// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn paths_native_invocation(home: &str) -> NativeInvocation {
    NativeInvocation {
        event_id: "evt-cc-home-1".to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: LIB_ID.to_owned(),
        dependency_role: ROLE_DEPENDENCY.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: TOOL_KIND_CC.to_owned(),
        executable: "/tools/native/cc".to_owned(),
        argv: vec![
            "cc".to_owned(),
            "-O2".to_owned(),
            "-c".to_owned(),
            "src/native.c".to_owned(),
            "-o".to_owned(),
            "out/native.o".to_owned(),
        ],
        adapter_chain: vec!["/tools/native/cc".to_owned()],
        parent_event_id: None,
        operation_kind: OPERATION_COMPILATION.to_owned(),
        cwd: home.to_owned(),
    }
}

/// C18: native commands attribute compilations to the observed adapter; an
/// absolute executable outside the adapter chain cannot resolve to a native
/// tool kind, and its unobserved calls block the warm zero.
pub(crate) fn check_native_command_attribution() -> Result<(), Box<dyn Error>> {
    if !is_observed_native_executable("/tools/native/cc", "/tools/native/cc", "/tools/native/cxx") {
        return Err(invalid("observed adapter executable misclassified"));
    }
    if is_observed_native_executable("/usr/bin/cc", "/tools/native/cc", "/tools/native/cxx") {
        return Err(invalid("absolute system compiler claimed as observed"));
    }
    if resolve_native_tool_kind("/tools/native/cc", "/tools/native/cc", "/tools/native/cxx")?
        != TOOL_KIND_CC
    {
        return Err(invalid("adapter executable lost its tool kind"));
    }
    must_fail(
        &resolve_native_tool_kind("/usr/bin/cc", "/tools/native/cc", "/tools/native/cxx")
            .map(|_| ()),
        "unobserved absolute executable resolved to a tool kind",
    )?;
    must_fail(
        &resolve_native_tool_kind(
            "/tools/native/cc",
            "/tools/native/same",
            "/tools/native/same",
        )
        .map(|_| ()),
        "ambiguous adapter pair admitted",
    )?;
    must_fail(
        &select_native_adapter("rustc", "/tools/native/cc", "/tools/native/cxx").map(|_| ()),
        "unknown native tool kind selected an adapter",
    )?;
    if select_native_adapter(TOOL_KIND_CXX, "/tools/native/cc", "/tools/native/cxx")?
        != "/tools/native/cxx"
    {
        return Err(invalid("C++ adapter selection drifted"));
    }
    let cold = native_cold_ledger()?;
    require_nonzero("cold native compilations", cold.dependency_compilations())?;
    if cold.measured_native_compilations() != Some(1) {
        return Err(invalid("cold native work lost its measured compilation"));
    }
    let mut blind = native_cold_ledger()?;
    blind.record_unobserved_absolute("/usr/bin/cc")?;
    if blind.measured_native_compilations().is_some() {
        return Err(invalid("unobserved absolute calls measured as observed"));
    }
    must_fail(
        &require_measured_zero("blind native work", blind.measured_native_compilations()),
        "unobserved native work admitted as warm zero",
    )?;
    let warm = warm_native_ledger();
    require_measured_zero("warm native work", warm.measured_native_compilations())?;
    Ok(())
}

/// C18: restored bytes land in the exact private home the child commands use;
/// identical home content authenticates unchanged while any byte or mode drift
/// is detected and never scheduled for restore.
pub(crate) fn check_private_home_restore() -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("private-home");
    fs::create_dir_all(&home)?;
    let declared = ProjectionEntry {
        path: "cargo-home/bin/tool".to_owned(),
        kind: EntryKind::File,
        mode: 0o755,
        content_sha256: sha256_hex(b"home-tool-bytes"),
        link_target: None,
        mtime: Some(RealMtime {
            seconds: 946_684_800,
            nanoseconds: 0,
        }),
    };
    declared.validate()?;
    if authenticate_entry(&home, &declared)? != CheckoutState::Missing {
        return Err(invalid("empty private home misclassified its state"));
    }
    let full = home.join("cargo-home/bin/tool");
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full, b"home-tool-bytes")?;
    fs::set_permissions(&full, fs::Permissions::from_mode(0o755))?;
    if authenticate_entry(&home, &declared)? != CheckoutState::Unchanged {
        return Err(invalid("restored home bytes rejected in the exact home"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Unchanged)
        != RestoreDecision::Restore
    {
        return Err(invalid("unchanged home bytes not scheduled for restore"));
    }
    fs::write(&full, b"other bytes")?;
    if authenticate_entry(&home, &declared)? != CheckoutState::Changed {
        return Err(invalid("home content drift missed"));
    }
    if decide(Some(&declared), Some(&declared), CheckoutState::Changed)
        != RestoreDecision::KeepCurrent
    {
        return Err(invalid("changed home bytes scheduled for overwrite"));
    }
    let invocation = paths_native_invocation(
        home.to_str()
            .ok_or_else(|| invalid("private home is not representable"))?,
    );
    invocation
        .validate()
        .map_err(|error| invalid(&error.to_string()))?;
    let mut ledger = NativeLedger::new();
    ledger.record(NativeEvent {
        invocation,
        completion: None,
    })?;
    ledger.complete(
        "evt-cc-home-1",
        NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    require_nonzero("home native compilations", ledger.dependency_compilations())?;
    Ok(())
}

/// C19: the command inventory covers exactly the rustc, clippy-driver, and
/// rustdoc families; unknown tool kinds never classify to a family adapter.
pub(crate) fn check_tool_kind_coverage() -> Result<(), Box<dyn Error>> {
    if TOOL_KINDS != [TOOL_KIND_RUSTC, TOOL_KIND_CLIPPY_DRIVER, TOOL_KIND_RUSTDOC] {
        return Err(invalid("command family inventory drifted"));
    }
    for kind in TOOL_KINDS {
        if !is_supported_tool_kind(kind) {
            return Err(invalid("listed command family rejected"));
        }
    }
    must_fail(
        &is_supported_tool_kind("turbo")
            .then_some(())
            .ok_or_else(|| invalid("unknown tool kind admitted")),
        "unknown tool kind admitted",
    )?;
    if classify_tool_kind("/tools/rust/prefix/bin/rustc")? != TOOL_KIND_RUSTC {
        return Err(invalid("rustc executable lost its family"));
    }
    if classify_tool_kind("clippy-driver")? != TOOL_KIND_CLIPPY_DRIVER {
        return Err(invalid("clippy-driver executable lost its family"));
    }
    if classify_tool_kind("/tools/rust/prefix/bin/rustdoc")? != TOOL_KIND_RUSTDOC {
        return Err(invalid("rustdoc executable lost its family"));
    }
    must_fail(
        &classify_tool_kind("/usr/bin/turbo").map(|_| ()),
        "unknown executable classified to a command family",
    )?;
    Ok(())
}

/// C19: documentation routing keeps the four supported phases; the suggestion
/// maps doctest descendants, probes, and plain rustdoc compilations, and an
/// unsupported phase never validates.
pub(crate) fn check_doc_phase_routing() -> Result<(), Box<dyn Error>> {
    if DOC_PHASES
        != [
            DOC_PHASE_COMPILATION,
            DOC_PHASE_FINALIZATION,
            DOC_PHASE_PROBE,
            DOC_PHASE_DOCTEST_DESCENDANT,
        ]
    {
        return Err(invalid("documentation phase inventory drifted"));
    }
    for phase in DOC_PHASES {
        if !is_supported_doc_phase(phase) {
            return Err(invalid("listed documentation phase rejected"));
        }
    }
    must_fail(
        &is_supported_doc_phase("draft")
            .then_some(())
            .ok_or_else(|| invalid("unknown doc phase admitted")),
        "unknown documentation phase admitted",
    )?;
    let doctest_argv = vec![
        "rustdoc".to_owned(),
        "--test".to_owned(),
        "src/lib.rs".to_owned(),
    ];
    if suggest_doc_phase(&doctest_argv) != DOC_PHASE_DOCTEST_DESCENDANT {
        return Err(invalid("doctest descendant lost its phase"));
    }
    let probe_argv = vec![
        "rustdoc".to_owned(),
        "--check".to_owned(),
        "src/lib.rs".to_owned(),
    ];
    if suggest_doc_phase(&probe_argv) != DOC_PHASE_PROBE {
        return Err(invalid("documentation probe lost its phase"));
    }
    let plain_argv = vec!["rustdoc".to_owned(), "src/lib.rs".to_owned()];
    if suggest_doc_phase(&plain_argv) != DOC_PHASE_COMPILATION {
        return Err(invalid("plain rustdoc run lost its compilation phase"));
    }
    Ok(())
}
