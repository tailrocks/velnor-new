//! Per-package task-group derivation with clippy-gates-tests edges.

use velnor_actions_contract::{ContractError, manifest_key_for_cargo_manifest, task_id_for_stack};

use velnor_actions_rust_core::{
    metadata::PackageRecord,
    profile::{CompileDriver, NextestProfile, RustExecutionProfile, TestRunner},
};

pub use crate::argv::{
    EntryMetadata, cargo_payload_argv, cargo_payload_with_profile, entry_metadata, evidence_id,
    parse_rerun_changed, require_nextest_for_shards, shard_task_id, shards_allowed,
};
pub use crate::task_identity::{DigestSlot, ExtensionInputs, RustTaskIdentityExtension, SlotState};

/// Rust task kinds derived per package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Formatting check.
    Fmt,
    /// Clippy lint gate.
    Clippy,
    /// Generated legacy Cargo-test request; superseded by Nextest.
    Test,
    /// Unit and integration tests via Nextest.
    Nextest,
    /// Documentation-test coverage gap; never executable in generated CI.
    Doctest,
    /// Documentation build.
    Doc,
    /// Test-executable or archive build.
    Build,
}

impl TaskKind {
    /// Stable task-id kind segment.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fmt => "fmt",
            Self::Clippy => "clippy",
            Self::Test => "test",
            Self::Nextest => "nextest",
            Self::Doctest => "doctest",
            Self::Doc => "doc",
            Self::Build => "build",
        }
    }
}

/// One derived Rust task group.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "task-group cache/reuse signal flags"
)]
pub struct TaskGroup {
    /// Stable task id.
    pub task_id: String,
    /// Cargo package id (empty for workspace-level formatting).
    pub package_id: String,
    /// Package name (display only).
    pub package_name: String,
    /// Manifest key used in the task id.
    pub manifest_key: String,
    /// Task kind.
    pub kind: TaskKind,
    /// Configuration name.
    pub configuration: String,
    /// Sorted enabled features.
    pub features: Vec<String>,
    /// Execution target (`host` or triple).
    pub target: String,
    /// Quality gates that must pass first (task ids).
    pub gated_by: Vec<String>,
    /// Data producers that must finish first (task ids).
    pub depends_on: Vec<String>,
    /// Existing metadata-derived non-doc target flags (`Nextest` only).
    pub target_flags: Vec<String>,
    /// No applicable test target exists; emit no test command.
    pub no_test_targets: bool,
    /// Package named by Clippy (`clippy` kind only).
    pub package_arg: Option<String>,
    /// Selected compile driver.
    pub compile_driver: CompileDriver,
    /// Selected test runner.
    pub test_runner: TestRunner,
    /// Resolved Nextest profile.
    pub nextest_profile: NextestProfile,
    /// Whether nextest should run ignored tests.
    pub run_ignored: Option<String>,
    /// Declared non-Rust task inputs (sorted, deduped).
    pub declared_inputs: Vec<String>,
    /// Build script may read undeclared inputs (conservative at derive).
    pub undeclared_reads: bool,
    /// Task needs network access (Rust payloads run offline: false).
    pub uses_network: bool,
    /// Task reads the wall clock (Rust payloads: false).
    pub uses_clock: bool,
    /// Task needs randomness (Rust payloads: false).
    pub uses_random: bool,
}

impl TaskGroup {
    /// Attach declared non-Rust inputs (sorted, deduped).
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for empty, absolute, or traversing paths.
    pub fn with_declared_inputs(mut self, paths: &[String]) -> Result<Self, ContractError> {
        for path in paths {
            let bad =
                path.is_empty() || path.starts_with('/') || path.split('/').any(|s| s == "..");
            if bad {
                return Err(ContractError::identity(
                    "declared_inputs",
                    format!("bad_input:{path}"),
                ));
            }
        }
        let mut sorted = paths.to_vec();
        sorted.sort();
        sorted.dedup();
        self.declared_inputs = sorted;
        Ok(self)
    }
}

/// Inputs for per-package derivation.
#[derive(Debug, Clone)]
pub struct DeriveInputs<'a> {
    /// Package inventory.
    pub package: &'a PackageRecord,
    /// Workspace profile.
    pub profile: &'a RustExecutionProfile,
    /// Configuration name.
    pub configuration: &'a str,
    /// Enabled features.
    pub features: &'a [String],
    /// Execution target.
    pub target: &'a str,
    /// Whether the package has explicit formatting configuration.
    pub explicit_fmt: bool,
}

/// Shared group fields resolved once per derivation.
struct GroupBase<'a> {
    key: &'a str,
    configuration: &'a str,
    features: &'a [String],
    target: &'a str,
    driver: CompileDriver,
    runner: TestRunner,
    nextest_profile: NextestProfile,
    run_ignored: Option<String>,
    package: &'a PackageRecord,
}

/// Derive per-package task groups; Nextest owns unit/integration execution.
///
/// Formatting is per package only with explicit configuration; otherwise the
/// plan-job Format step covers formatting. Cargo's test runner is forbidden in
/// generated Actions, so every executable test profile normalizes to Nextest.
///
/// # Errors
///
/// Returns [`ContractError`] when a task id cannot be derived.
pub fn derive_task_groups(inputs: &DeriveInputs<'_>) -> Result<Vec<TaskGroup>, ContractError> {
    let manifest_key = manifest_key_for_cargo_manifest(&inputs.package.manifest)?;
    let mut features = inputs.features.to_vec();
    features.sort();
    let base = GroupBase {
        key: &manifest_key,
        configuration: inputs.configuration,
        features: &features,
        target: inputs.target,
        driver: inputs.profile.compile_driver,
        runner: TestRunner::CargoNextest,
        nextest_profile: inputs.profile.nextest_profile,
        run_ignored: inputs.profile.run_ignored.clone(),
        package: inputs.package,
    };
    let clippy_id = task_id(&base, TaskKind::Clippy)?;
    let mut groups = vec![clippy_group(&base, &clippy_id)];
    let build_id = task_id(&base, TaskKind::Build)?;
    groups.push(plain_group(
        &base,
        TaskKind::Build,
        &build_id,
        std::slice::from_ref(&clippy_id),
        &[],
    ));
    let test_kind = TaskKind::Nextest;
    let test_id = task_id(&base, test_kind)?;
    groups.push(test_group(
        &base,
        test_kind,
        &test_id,
        &clippy_id,
        Some(build_id.as_str()),
    ));
    let doctest_id = task_id(&base, TaskKind::Doctest)?;
    groups.push(doctest_group(&base, &doctest_id, &clippy_id));
    let doc_id = task_id(&base, TaskKind::Doc)?;
    let doc_gates = [clippy_id.clone(), doctest_id.clone()];
    groups.push(plain_group(&base, TaskKind::Doc, &doc_id, &doc_gates, &[]));
    if inputs.explicit_fmt {
        let fmt_id = task_id(&base, TaskKind::Fmt)?;
        groups.push(plain_group(&base, TaskKind::Fmt, &fmt_id, &[], &[]));
    }
    Ok(groups)
}

/// Derive the single plan-level formatting group for a workspace root.
///
/// # Errors
///
/// Returns [`ContractError`] when the workspace manifest or task id is invalid.
pub fn derive_workspace_fmt(
    workspace_manifest: &str,
    profile: &RustExecutionProfile,
    configuration: &str,
    target: &str,
) -> Result<TaskGroup, ContractError> {
    let key = manifest_key_for_cargo_manifest(workspace_manifest)?;
    let task_id = task_id_for_stack(
        crate::STACK_ID,
        &key,
        TaskKind::Fmt.as_str(),
        configuration,
        None,
    )?;
    Ok(TaskGroup {
        task_id,
        package_id: String::new(),
        package_name: String::new(),
        manifest_key: key,
        kind: TaskKind::Fmt,
        configuration: configuration.to_owned(),
        features: Vec::new(),
        target: target.to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: profile.compile_driver,
        test_runner: profile.test_runner,
        nextest_profile: profile.nextest_profile,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    })
}

/// Derive the workspace formatting group only with explicit root config.
///
/// Returns `None` without explicit workspace formatting configuration:
/// per-package groups cover explicit packages and the plan-job Format
/// step covers the rest, so a workspace matrix obligation would double-run.
///
/// # Errors
///
/// Returns [`ContractError`] when the workspace manifest or task id is invalid.
pub fn derive_workspace_fmt_if_explicit(
    workspace_manifest: &str,
    profile: &RustExecutionProfile,
    configuration: &str,
    target: &str,
    explicit_fmt: bool,
) -> Result<Option<TaskGroup>, ContractError> {
    if !explicit_fmt {
        return Ok(None);
    }
    derive_workspace_fmt(workspace_manifest, profile, configuration, target).map(Some)
}

/// Derive one task id for `kind`.
fn task_id(base: &GroupBase<'_>, kind: TaskKind) -> Result<String, ContractError> {
    task_id_for_stack(
        crate::STACK_ID,
        base.key,
        kind.as_str(),
        base.configuration,
        None,
    )
}

/// Build a group with explicit gate and data edges.
fn plain_group(
    base: &GroupBase<'_>,
    kind: TaskKind,
    id: &str,
    gated_by: &[String],
    depends_on: &[String],
) -> TaskGroup {
    TaskGroup {
        task_id: id.to_owned(),
        package_id: base.package.id.clone(),
        package_name: base.package.name.clone(),
        manifest_key: base.key.to_owned(),
        kind,
        configuration: base.configuration.to_owned(),
        features: base.features.to_vec(),
        target: base.target.to_owned(),
        gated_by: gated_by.to_vec(),
        depends_on: depends_on.to_vec(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: base.driver,
        test_runner: base.runner,
        nextest_profile: base.nextest_profile,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: base.package.has_build_script,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

/// Build the Clippy gate naming exactly one package.
fn clippy_group(base: &GroupBase<'_>, id: &str) -> TaskGroup {
    let mut group = plain_group(base, TaskKind::Clippy, id, &[], &[]);
    group.package_arg = Some(base.package.name.clone());
    group
}

/// Build the test group with metadata-derived target flags.
fn test_group(
    base: &GroupBase<'_>,
    kind: TaskKind,
    id: &str,
    clippy_id: &str,
    build_id: Option<&str>,
) -> TaskGroup {
    let mut group = plain_group(base, kind, id, &[clippy_id.to_owned()], &[]);
    if kind == TaskKind::Nextest {
        let mut target_flags = target_flags(base.package);
        target_flags.sort();
        group.target_flags = target_flags;
        group.run_ignored = base.run_ignored.clone().or_else(|| {
            (base.package.name.contains("conformance") || base.package.name.contains("visual"))
                .then(|| "all".to_owned())
        });
    }
    group.no_test_targets = !has_test_targets(base.package);
    group.depends_on = build_id.map(str::to_owned).into_iter().collect();
    group
}

/// Build the non-executable doctest gap for either test profile.
///
/// Cargo's documented-test runner is forbidden in generated Actions.
/// Nextest has no supported doctest mode, so coverage remains explicit
/// `NOT_RUN` rather than silently claiming execution.
fn doctest_group(base: &GroupBase<'_>, id: &str, clippy_id: &str) -> TaskGroup {
    let mut group = plain_group(base, TaskKind::Doctest, id, &[clippy_id.to_owned()], &[]);
    group.no_test_targets = true;
    group
}

/// Test-bearing non-doc target flags; `test = false` targets get none.
fn target_flags(package: &PackageRecord) -> Vec<String> {
    [
        ("lib", "--lib"),
        ("bin", "--bins"),
        ("test", "--tests"),
        ("example", "--examples"),
        ("bench", "--benches"),
    ]
    .into_iter()
    .filter(|(k, _)| package.targets.iter().any(|t| t.kind == *k && t.test))
    .map(|(_, f)| (*f).to_owned())
    .collect()
}

/// Whether any target is exercised by `cargo test` (mirrors `test`).
fn has_test_targets(package: &PackageRecord) -> bool {
    package.targets.iter().any(|target| {
        target.test
            && matches!(
                target.kind.as_str(),
                "lib" | "bin" | "test" | "example" | "bench"
            )
    })
}
