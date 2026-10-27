// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::io;

use velnor_actions_contract::canonical::{Digest, digest_b3};
use velnor_actions_contract::workflow::cache_identity::{Gc1Family, GenerationId};
use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerInvocation, CompilerLedger, OPERATION_COMPILATION,
    OPERATION_COMPILER_QUERY, ROLE_APPLICATION, ROLE_DEPENDENCY, TOOL_KIND_RUSTC,
};
use velnor_actions_mise::native_compiler_observer::{
    NativeCompletion, NativeEvent, NativeInvocation, NativeLedger, TOOL_KIND_CC,
};
use velnor_actions_rust::identity::DigestSlot;
use velnor_actions_rust::identity::{
    DependencyCoverage, DependencyCoverageInputs, dependency_closure_ids, dependency_coverage,
};
use velnor_actions_rust::metadata::{PackageRecord, VelnorV1TaskOwner, WorkspaceRecord};
use velnor_actions_rust::resolved_graph::{ResolvedGraph, ResolvedPathPackage};

pub(crate) const APP_ID: &str = "workspace-app";
pub(crate) const LIB_ID: &str = "workspace-lib";
pub(crate) const SERDE_ID: &str = "registry-serde";
pub(crate) const LINUX_TARGET: &str = "x86_64-unknown-linux-gnu";
pub(crate) const ARM_TARGET: &str = "aarch64-unknown-linux-gnu";

pub(crate) fn invalid(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

pub(crate) fn fixture_graph() -> ResolvedGraph {
    ResolvedGraph {
        edges: vec![
            (APP_ID.to_owned(), LIB_ID.to_owned()),
            (APP_ID.to_owned(), SERDE_ID.to_owned()),
            (LIB_ID.to_owned(), SERDE_ID.to_owned()),
        ],
        path_packages: vec![
            ResolvedPathPackage {
                id: APP_ID.to_owned(),
                dir: "crates/app".to_owned(),
            },
            ResolvedPathPackage {
                id: LIB_ID.to_owned(),
                dir: "crates/lib".to_owned(),
            },
        ],
    }
}

fn fixture_package(id: &str, name: &str, version: &str) -> PackageRecord {
    PackageRecord {
        id: id.to_owned(),
        name: name.to_owned(),
        version: version.to_owned(),
        manifest: format!("crates/{name}/Cargo.toml"),
        external: false,
        in_workspace: true,
        targets: Vec::new(),
        features: vec!["serde".to_owned()],
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::Project,
    }
}

pub(crate) fn fixture_workspaces(app_version: &str, lib_version: &str) -> Vec<WorkspaceRecord> {
    vec![WorkspaceRecord {
        workspace_root: String::new(),
        members: vec![APP_ID.to_owned(), LIB_ID.to_owned()],
        packages: vec![
            fixture_package(APP_ID, "app", app_version),
            fixture_package(LIB_ID, "lib", lib_version),
        ],
        edges: Vec::new(),
        skipped_edges: Vec::new(),
        has_external_dependencies: true,
    }]
}

pub(crate) fn coverage_variant(
    app_version: &str,
    lib_version: &str,
    extra_feature: Option<&str>,
    target: &str,
    extra_native: Option<&str>,
    lock_fill: char,
) -> Result<DependencyCoverage, Box<dyn Error>> {
    let graph = fixture_graph();
    let workspaces = fixture_workspaces(app_version, lib_version);
    let lock = DigestSlot::Known(format!("b3-{}", lock_fill.to_string().repeat(64)));
    let mut features = vec!["serde".to_owned()];
    if let Some(feature) = extra_feature {
        features.push(feature.to_owned());
    }
    let targets = vec![target.to_owned()];
    let mut native_inputs = vec!["openssl-src".to_owned()];
    if let Some(native) = extra_native {
        native_inputs.push(native.to_owned());
    }
    Ok(dependency_coverage(&DependencyCoverageInputs {
        package_id: APP_ID,
        graph: &graph,
        workspaces: &workspaces,
        features: &features,
        all_features: false,
        targets: &targets,
        lock_digest: &lock,
        native_inputs: &native_inputs,
    })?)
}

pub(crate) fn base_coverage() -> Result<DependencyCoverage, Box<dyn Error>> {
    coverage_variant("0.1.0", "0.2.0", None, LINUX_TARGET, None, 'f')
}

pub(crate) fn require_valid_digests(coverage: &DependencyCoverage) -> Result<(), Box<dyn Error>> {
    for digest in [
        &coverage.dependency_digest,
        &coverage.workspace_digest,
        &coverage.native_digest,
        &coverage.coverage_digest,
    ] {
        Digest::parse(digest)?;
    }
    Ok(())
}

pub(crate) fn require_same_coverage(
    first: &DependencyCoverage,
    second: &DependencyCoverage,
) -> Result<(), Box<dyn Error>> {
    if first != second {
        return Err(invalid("identical inputs produced different coverage"));
    }
    Ok(())
}

pub(crate) fn require_changed_coverage(
    before: &DependencyCoverage,
    after: &DependencyCoverage,
) -> Result<(), Box<dyn Error>> {
    if before.coverage_digest == after.coverage_digest {
        return Err(invalid("changed inputs kept the old coverage digest"));
    }
    Ok(())
}

pub(crate) fn app_closure() -> Result<BTreeSet<String>, Box<dyn Error>> {
    Ok(dependency_closure_ids(&fixture_graph(), APP_ID)?)
}

fn rust_argv() -> Vec<String> {
    [
        "rustc",
        "--crate-name",
        "mylib",
        "--edition=2021",
        "src/lib.rs",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

fn rust_invocation(
    event_id: &str,
    package_id: &str,
    role: &str,
    operation: &str,
) -> CompilerInvocation {
    CompilerInvocation {
        event_id: event_id.to_owned(),
        recipe_id: "internal/lint/dev".to_owned(),
        package_id: package_id.to_owned(),
        dependency_role: role.to_owned(),
        lane_id: "build".to_owned(),
        tool_kind: TOOL_KIND_RUSTC.to_owned(),
        executable: "/tools/rust/prefix/bin/rustc".to_owned(),
        argv: rust_argv(),
        wrapper_chain: Vec::new(),
        operation_kind: operation.to_owned(),
        doc_phase: None,
        cwd: "/repo".to_owned(),
    }
}

fn rust_success() -> CompilerCompletion {
    CompilerCompletion {
        exit_code: Some(0),
        signal: None,
        interrupted: false,
    }
}

pub(crate) fn cold_ledger() -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    for (event_id, package_id, role) in [
        ("evt-lib-1", LIB_ID, ROLE_DEPENDENCY),
        ("evt-serde-1", SERDE_ID, ROLE_DEPENDENCY),
        ("evt-app-1", APP_ID, ROLE_APPLICATION),
    ] {
        ledger.record(CompilerEvent {
            invocation: rust_invocation(event_id, package_id, role, OPERATION_COMPILATION),
            completion: None,
        })?;
        ledger.complete(event_id, rust_success())?;
    }
    Ok(ledger)
}

pub(crate) fn changed_closure_ledger() -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation: rust_invocation("evt-lib-2", LIB_ID, ROLE_DEPENDENCY, OPERATION_COMPILATION),
        completion: None,
    })?;
    ledger.complete("evt-lib-2", rust_success())?;
    Ok(ledger)
}

pub(crate) fn query_only_ledger() -> Result<CompilerLedger, Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation: rust_invocation(
            "evt-query-1",
            LIB_ID,
            ROLE_DEPENDENCY,
            OPERATION_COMPILER_QUERY,
        ),
        completion: None,
    })?;
    ledger.complete("evt-query-1", rust_success())?;
    Ok(ledger)
}

fn native_argv() -> Vec<String> {
    ["cc", "-O2", "-c", "src/native.c", "-o", "out/native.o"]
        .into_iter()
        .map(str::to_owned)
        .collect()
}

pub(crate) fn native_cold_ledger() -> Result<NativeLedger, Box<dyn Error>> {
    let mut ledger = NativeLedger::new();
    ledger.record(NativeEvent {
        invocation: NativeInvocation {
            event_id: "evt-cc-1".to_owned(),
            recipe_id: "internal/lint/dev".to_owned(),
            package_id: LIB_ID.to_owned(),
            dependency_role: ROLE_DEPENDENCY.to_owned(),
            lane_id: "build".to_owned(),
            tool_kind: TOOL_KIND_CC.to_owned(),
            executable: "/tools/native/cc".to_owned(),
            argv: native_argv(),
            adapter_chain: vec!["/tools/native/cc".to_owned()],
            parent_event_id: None,
            operation_kind: OPERATION_COMPILATION.to_owned(),
            cwd: "/repo".to_owned(),
        },
        completion: None,
    })?;
    ledger.complete(
        "evt-cc-1",
        NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    Ok(ledger)
}

pub(crate) fn generation_for(coverage_seed: &[u8]) -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-qualification-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-qualification-compat"))?,
        coverage_digest: Digest::parse(&digest_b3(coverage_seed))?,
        repair_revision: 0,
    })
}

pub(crate) fn compat_variant_generation() -> Result<GenerationId, Box<dyn Error>> {
    Ok(GenerationId {
        family: Gc1Family::State,
        group_digest: Digest::parse(&digest_b3(b"gc1-qualification-group"))?,
        compatibility_digest: Digest::parse(&digest_b3(b"gc1-qualification-compat-variant"))?,
        coverage_digest: Digest::parse(&digest_b3(b"gc1-qualification-coverage"))?,
        repair_revision: 0,
    })
}

pub(crate) fn warm_native_ledger() -> NativeLedger {
    NativeLedger::new()
}

pub(crate) fn eligible_ids() -> Vec<String> {
    vec![LIB_ID.to_owned(), SERDE_ID.to_owned()]
}

pub(crate) fn require_nonzero(label: &str, count: u64) -> Result<(), Box<dyn Error>> {
    if count == 0 {
        return Err(invalid(&format!("{label} observed zero work")));
    }
    Ok(())
}

pub(crate) fn require_zero(label: &str, count: u64) -> Result<(), Box<dyn Error>> {
    if count != 0 {
        return Err(invalid(&format!("{label} repeated eliminated work")));
    }
    Ok(())
}

pub(crate) fn require_measured_zero(
    label: &str,
    observed: Option<u64>,
) -> Result<(), Box<dyn Error>> {
    match observed {
        Some(0) => Ok(()),
        Some(_) => Err(invalid(&format!("{label} repeated eliminated work"))),
        None => Err(invalid(&format!("{label} is unknown, not zero"))),
    }
}
