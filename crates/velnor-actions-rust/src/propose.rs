use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{
    CachePolicy, ContractError, IdentityInputs, ProposedTask, ResourceClass, ResourceDemand,
    component_id_for_unit, project_root_for_unit_path,
};

use crate::cargo_env::cargo_payload_env;
use crate::detect::manifest_for_key;
use crate::profile::{CompileDriver, TestRunner};
use crate::tasks::{TaskGroup, TaskKind, cargo_payload_with_profile};

impl TaskKind {
    pub fn parse(value: &str) -> Result<Self, ContractError> {
        match value {
            "fmt" => Ok(Self::Fmt),
            "clippy" => Ok(Self::Clippy),
            "test" => Ok(Self::Test),
            "nextest" => Ok(Self::Nextest),
            "doctest" => Ok(Self::Doctest),
            "doc" => Ok(Self::Doc),
            "build" => Ok(Self::Build),
            _ => Err(ContractError::identity(
                "task_kind",
                format!("unknown_kind:{value}"),
            )),
        }
    }
}

pub fn propose_task(group: &TaskGroup) -> Result<ProposedTask, ContractError> {
    let manifest = manifest_for_key(&group.manifest_key);
    let project_root = project_root_for_unit_path(&manifest);
    let environment: BTreeMap<String, String> = cargo_payload_env(group.kind)
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();
    Ok(ProposedTask {
        task_id: group.task_id.clone(),
        stack_id: crate::STACK_ID.to_owned(),
        component_id: component_id_for_unit(&group.package_id, &manifest),
        task_kind: group.kind.as_str().to_owned(),
        configuration: group.configuration.clone(),
        depends_on: group.depends_on.clone(),
        gated_by: group.gated_by.clone(),
        reads: vec![manifest.clone()],
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: resource_class_for_kind(group.kind),
            cpu_milli: None,
            memory_mb: None,
            needs_network: group.uses_network,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: true,
            allow_task_reuse: !group.undeclared_reads,
        },
        identity: IdentityInputs {
            unit_id: group.package_id.clone(),
            unit_key: group.manifest_key.clone(),
            unit_path: manifest,
            project_root,
            target: group.target.clone(),
            features: group.features.clone(),
            flags: {
                let mut f = group.target_flags.clone();
                if group.all_features {
                    f.push("--all-features".to_owned());
                }
                f.sort();
                f
            },
            compile_driver: group.compile_driver.as_str().to_owned(),
            test_runner: group.test_runner.as_str().to_owned(),
            environment,
            declared_inputs: group.declared_inputs.clone(),
            undeclared_reads: group.undeclared_reads,
        },
        payload: cargo_payload_with_profile(group)?,
        display_name: group.package_name.clone(),
        uses_clock: group.uses_clock,
        uses_random: group.uses_random,
        no_targets: group.no_test_targets,
        runner_profile: group.nextest_profile.as_str().to_owned(),
    })
}

#[must_use]
pub fn resource_class_for_kind(kind: TaskKind) -> ResourceClass {
    match kind {
        TaskKind::Clippy | TaskKind::Build => ResourceClass::Compiler,
        TaskKind::Test | TaskKind::Nextest | TaskKind::Doctest => ResourceClass::Test,
        TaskKind::Doc | TaskKind::Fmt => ResourceClass::Lightweight,
    }
}

#[must_use]
pub fn task_kind_rank(kind: &str) -> u32 {
    match kind {
        "fmt" => 0,
        "clippy" => 1,
        "build" => 2,
        "test" | "nextest" => 3,
        "doctest" => 4,
        "doc" => 5,
        _ => u32::MAX,
    }
}

#[must_use]
pub fn is_clippy_kind(kind: &str) -> bool {
    kind == TaskKind::Clippy.as_str()
}

#[must_use]
pub fn is_nextest_kind(kind: &str) -> bool {
    kind == TaskKind::Nextest.as_str()
}

#[must_use]
pub fn is_workspace_fmt_task(kind: &str, unit_id: &str, display_name: &str) -> bool {
    kind == TaskKind::Fmt.as_str() && unit_id.is_empty() && display_name.is_empty()
}

#[must_use]
pub fn step_base_name<'a>(kind: &'a str, fmt_name: &'a str) -> &'a str {
    match kind {
        "fmt" => fmt_name,
        "clippy" => "Clippy",
        "build" => "Build test executables",
        "test" | "nextest" => "Unit and integration tests",
        "doctest" => "Doctests",
        "doc" => "Documentation",
        _ => kind,
    }
}

pub const KIND_DISPLAY_WORDS: [(&str, &str); 7] = [
    ("clippy", "Clippy"),
    ("build", "build"),
    ("test", "run tests"),
    ("nextest", "run tests"),
    ("doctest", "doctests"),
    ("doc", "doc build"),
    ("fmt", "format check"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolNeeds {
    pub mbx: bool,
    pub nextest: bool,
}

#[must_use]
pub fn tool_needs(compile_driver: &str, test_runner: &str) -> ToolNeeds {
    ToolNeeds {
        mbx: compile_driver == CompileDriver::Mbx.as_str(),
        nextest: test_runner == TestRunner::CargoNextest.as_str(),
    }
}

#[must_use]
pub fn runs_before_parallel_checks(kind: &str) -> bool {
    TaskKind::parse(kind) == Ok(TaskKind::Build)
}

#[must_use]
pub fn concurrent_target_lane(kind: &str) -> Option<&'static str> {
    match TaskKind::parse(kind) {
        Ok(TaskKind::Clippy) => Some("velnor-clippy"),
        Ok(TaskKind::Doc) => Some("velnor-doc"),
        _ => None,
    }
}

#[must_use]
pub fn payload_env_for_kind(kind: &str) -> Vec<(OsString, OsString)> {
    match TaskKind::parse(kind) {
        Ok(parsed) => cargo_payload_env(parsed),
        Err(_) => Vec::new(),
    }
}
