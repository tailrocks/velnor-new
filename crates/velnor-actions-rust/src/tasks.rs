// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{ContractError, manifest_key_for_cargo_manifest, task_id_for_stack};

use crate::{
    metadata::PackageRecord,
    profile::{CompileDriver, NextestProfile, RustExecutionProfile, TestRunner},
};

pub use crate::argv::{
    EntryMetadata, cargo_payload_argv, cargo_payload_with_profile, entry_metadata, evidence_id,
    parse_rerun_changed, require_nextest_for_shards, shard_task_id, shards_allowed,
};
pub use crate::task_identity::{DigestSlot, ExtensionInputs, RustTaskIdentityExtension, SlotState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Fmt,
    Clippy,
    Test,
    Nextest,
    Doctest,
    Doc,
    Build,
}

impl TaskKind {
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

#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "task-group cache/reuse signal flags"
)]
pub struct TaskGroup {
    pub task_id: String,
    pub package_id: String,
    pub package_name: String,
    pub manifest_key: String,
    pub kind: TaskKind,
    pub configuration: String,
    pub features: Vec<String>,
    pub all_features: bool,
    pub target: String,
    pub gated_by: Vec<String>,
    pub depends_on: Vec<String>,
    pub target_flags: Vec<String>,
    pub no_test_targets: bool,
    pub package_arg: Option<String>,
    pub compile_driver: CompileDriver,
    pub test_runner: TestRunner,
    pub nextest_profile: NextestProfile,
    pub allow_empty_test_suite: bool,
    pub run_ignored: Option<String>,
    pub declared_inputs: Vec<String>,
    pub undeclared_reads: bool,
    pub uses_network: bool,
    pub uses_clock: bool,
    pub uses_random: bool,
}

impl TaskGroup {
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

#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "derive inputs mirror the task-group flag set"
)]
pub struct DeriveInputs<'a> {
    pub package: &'a PackageRecord,
    pub profile: &'a RustExecutionProfile,
    pub configuration: &'a str,
    pub features: &'a [String],
    pub all_features: bool,
    pub clippy_all_features: bool,
    pub allow_empty_test_suite: bool,
    pub target: &'a str,
    pub explicit_fmt: bool,
}

struct GroupBase<'a> {
    key: &'a str,
    configuration: &'a str,
    features: &'a [String],
    all_features: bool,
    clippy_all_features: bool,
    allow_empty_test_suite: bool,
    target: &'a str,
    driver: CompileDriver,
    runner: TestRunner,
    nextest_profile: NextestProfile,
    run_ignored: Option<String>,
    package: &'a PackageRecord,
}

pub fn derive_task_groups(inputs: &DeriveInputs<'_>) -> Result<Vec<TaskGroup>, ContractError> {
    let manifest_key = manifest_key_for_cargo_manifest(&inputs.package.manifest)?;
    let mut features = inputs.features.to_vec();
    features.sort();
    let base = GroupBase {
        key: &manifest_key,
        configuration: inputs.configuration,
        features: &features,
        all_features: inputs.all_features,
        clippy_all_features: inputs.clippy_all_features,
        allow_empty_test_suite: inputs.allow_empty_test_suite,
        target: inputs.target,
        driver: inputs.profile.compile_driver,
        runner: inputs.profile.test_runner,
        nextest_profile: inputs.profile.nextest_profile,
        run_ignored: inputs.profile.run_ignored.clone(),
        package: inputs.package,
    };
    let clippy_id = task_id(&base, TaskKind::Clippy)?;
    let mut groups = vec![clippy_group(&base, &clippy_id)];
    let (build_id, test_kind) = if inputs.profile.test_runner == TestRunner::CargoNextest {
        let id = task_id(&base, TaskKind::Build)?;
        groups.push(plain_group(&base, TaskKind::Build, &id));
        (Some(id), TaskKind::Nextest)
    } else {
        (None, TaskKind::Test)
    };
    let test_id = task_id(&base, test_kind)?;
    groups.push(test_group(&base, test_kind, &test_id, build_id.as_deref()));
    let doctest_id = task_id(&base, TaskKind::Doctest)?;
    groups.push(doctest_group(&base, &doctest_id));
    let doc_id = task_id(&base, TaskKind::Doc)?;
    groups.push(plain_group(&base, TaskKind::Doc, &doc_id));
    if inputs.explicit_fmt {
        let fmt_id = task_id(&base, TaskKind::Fmt)?;
        groups.push(plain_group(&base, TaskKind::Fmt, &fmt_id));
    }
    Ok(groups)
}

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
        all_features: false,
        target: target.to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: profile.compile_driver,
        test_runner: profile.test_runner,
        nextest_profile: profile.nextest_profile,
        allow_empty_test_suite: false,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    })
}

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

fn task_id(base: &GroupBase<'_>, kind: TaskKind) -> Result<String, ContractError> {
    task_id_for_stack(
        crate::STACK_ID,
        base.key,
        kind.as_str(),
        base.configuration,
        None,
    )
}

fn plain_group(base: &GroupBase<'_>, kind: TaskKind, id: &str) -> TaskGroup {
    TaskGroup {
        task_id: id.to_owned(),
        package_id: base.package.id.clone(),
        package_name: base.package.name.clone(),
        manifest_key: base.key.to_owned(),
        kind,
        configuration: base.configuration.to_owned(),
        features: base.features.to_vec(),
        all_features: base.all_features,
        target: base.target.to_owned(),
        gated_by: Vec::new(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: base.driver,
        test_runner: base.runner,
        nextest_profile: base.nextest_profile,
        allow_empty_test_suite: base.allow_empty_test_suite,
        run_ignored: None,
        declared_inputs: Vec::new(),
        undeclared_reads: base.package.has_build_script,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
    }
}

fn clippy_group(base: &GroupBase<'_>, id: &str) -> TaskGroup {
    let mut group = plain_group(base, TaskKind::Clippy, id);
    group.all_features |= base.clippy_all_features;
    group.package_arg = Some(base.package.name.clone());
    group
}

fn test_group(base: &GroupBase<'_>, kind: TaskKind, id: &str, build_id: Option<&str>) -> TaskGroup {
    let mut group = plain_group(base, kind, id);
    if kind == TaskKind::Test {
        group.target_flags = target_flags(base.package);
    }
    if kind == TaskKind::Nextest {
        group.run_ignored = base.run_ignored.clone().or_else(|| {
            (base.package.name.contains("conformance") || base.package.name.contains("visual"))
                .then(|| "all".to_owned())
        });
    }
    group.no_test_targets = !has_test_targets(base.package);
    group.depends_on = build_id.map(str::to_owned).into_iter().collect();
    group
}

fn doctest_group(base: &GroupBase<'_>, id: &str) -> TaskGroup {
    let mut group = plain_group(base, TaskKind::Doctest, id);
    group.no_test_targets = !base.package.targets.iter().any(|target| target.doctest);
    group
}

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

fn has_test_targets(package: &PackageRecord) -> bool {
    package.targets.iter().any(|target| {
        target.test
            && matches!(
                target.kind.as_str(),
                "lib" | "bin" | "test" | "example" | "bench"
            )
    })
}
