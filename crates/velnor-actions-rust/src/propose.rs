//! Rust proposal contract: task groups into neutral proposals.
//!
//! [`propose_task`] converts one derived [`TaskGroup`] wholesale into a
//! [`ProposedTask`]: field copies plus precomputed adapter facts (payload,
//! environment, component, project root), never recomputed downstream.
//! The kind/tool helpers below back the orchestrator's closed per-stack
//! dispatch: spellings stay here, decisions stay neutral out there.

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
    /// Parse a kind token; unknown tokens fail closed.
    ///
    /// Backs proposal dispatch: validated proposals always parse, so
    /// dispatch sites fail closed on drift instead of defaulting.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] for any token outside the seven kinds.
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

/// Convert one task group into its neutral proposal.
///
/// Copies adapter-known fields verbatim (ids, edges, features, flags,
/// drivers as spellings) and precomputes the payload, environment,
/// component, and project root the pipeline needs. List order is
/// preserved, never re-sorted: consumers sort their own copies.
///
/// # Errors
///
/// Returns [`ContractError`] when the fixed payload rejects a
/// leading-dash value.
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

/// Resource class for one task kind.
#[must_use]
pub fn resource_class_for_kind(kind: TaskKind) -> ResourceClass {
    match kind {
        TaskKind::Clippy | TaskKind::Build => ResourceClass::Compiler,
        TaskKind::Test | TaskKind::Nextest | TaskKind::Doctest => ResourceClass::Test,
        TaskKind::Doc | TaskKind::Fmt => ResourceClass::Lightweight,
    }
}

/// In-crate obligation order rank: Format, Clippy, build, tests, doctests, docs.
///
/// Unknown kinds sort last; validated proposals never carry them.
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

/// Whether `kind` is the Clippy lint gate.
#[must_use]
pub fn is_clippy_kind(kind: &str) -> bool {
    kind == TaskKind::Clippy.as_str()
}

/// Whether `kind` is the Nextest test runner.
#[must_use]
pub fn is_nextest_kind(kind: &str) -> bool {
    kind == TaskKind::Nextest.as_str()
}

/// Whether this is a package-less workspace Format task.
#[must_use]
pub fn is_workspace_fmt_task(kind: &str, unit_id: &str, display_name: &str) -> bool {
    kind == TaskKind::Fmt.as_str() && unit_id.is_empty() && display_name.is_empty()
}

/// Human step base name for one kind; shards suffix it downstream.
///
/// `fmt_name` carries the renderer's shared Format name so this table
/// never duplicates it; unknown kinds echo (validated proposals never
/// carry them).
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

/// Kind words in fixed plan order: `(kind spelling, display word)`.
pub const KIND_DISPLAY_WORDS: [(&str, &str); 7] = [
    ("clippy", "Clippy"),
    ("build", "build"),
    ("test", "run tests"),
    ("nextest", "run tests"),
    ("doctest", "doctests"),
    ("doc", "doc build"),
    ("fmt", "format check"),
];

/// Supplementary pinned tools one task needs beyond the base toolchain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolNeeds {
    /// Task compiles through MBX.
    pub mbx: bool,
    /// Task runs tests through Nextest.
    pub nextest: bool,
}

/// Supplementary tool needs from driver/runner spellings.
#[must_use]
pub fn tool_needs(compile_driver: &str, test_runner: &str) -> ToolNeeds {
    ToolNeeds {
        mbx: compile_driver == CompileDriver::Mbx.as_str(),
        nextest: test_runner == TestRunner::CargoNextest.as_str(),
    }
}

/// Fixed payload env for one kind spelling; empty unless `doc`.
#[must_use]
pub fn payload_env_for_kind(kind: &str) -> Vec<(OsString, OsString)> {
    match TaskKind::parse(kind) {
        Ok(parsed) => cargo_payload_env(parsed),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::NextestProfile;

    /// Minimal group exercising every converted field.
    fn group() -> TaskGroup {
        TaskGroup {
            task_id: "stack/rust/root/clippy/default".to_owned(),
            package_id: "path+file:///repo#demo@0.1.0".to_owned(),
            package_name: "demo".to_owned(),
            manifest_key: "root".to_owned(),
            kind: TaskKind::Clippy,
            configuration: "default".to_owned(),
            features: vec!["a".to_owned()],
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: Some("demo".to_owned()),
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoTest,
            nextest_profile: NextestProfile::Default,
            run_ignored: None,
            declared_inputs: Vec::new(),
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
        }
    }

    /// Conversion copies ids/edges and precomputes adapter facts.
    #[test]
    fn conversion_copies_and_precomputes() {
        let group = group();
        let task = propose_task(&group).expect("valid group proposes");
        assert_eq!(task.task_id, group.task_id);
        assert_eq!(task.stack_id, crate::STACK_ID);
        assert_eq!(task.component_id, "demo@0.1.0");
        assert_eq!(task.task_kind, "clippy");
        assert_eq!(task.reads, vec!["Cargo.toml".to_owned()]);
        assert_eq!(task.identity.unit_id, group.package_id);
        assert_eq!(task.identity.unit_key, "root");
        assert_eq!(task.identity.unit_path, "Cargo.toml");
        assert_eq!(task.identity.project_root, ".");
        assert_eq!(task.identity.compile_driver, "cargo");
        assert!(task.validate().is_ok());
    }

    /// The precomputed payload matches the direct payload bytes.
    #[test]
    fn payload_matches_direct_bytes() {
        let group = group();
        let task = propose_task(&group).expect("valid group proposes");
        assert_eq!(
            task.payload,
            cargo_payload_with_profile(&group).expect("direct payload")
        );
    }

    /// Payloads match for every kind with kind-specific inputs loaded.
    #[test]
    fn payload_matches_all_kinds() {
        let kinds = [
            TaskKind::Fmt,
            TaskKind::Clippy,
            TaskKind::Build,
            TaskKind::Test,
            TaskKind::Nextest,
            TaskKind::Doctest,
            TaskKind::Doc,
        ];
        for kind in kinds {
            let mut group = group();
            group.kind = kind;
            group.target_flags = vec!["--lib".to_owned()];
            group.no_test_targets = false;
            let task = propose_task(&group).expect("valid group proposes");
            assert_eq!(
                task.payload,
                cargo_payload_with_profile(&group).expect("direct payload"),
                "payload drift for kind {}",
                kind.as_str()
            );
            assert!(task.validate().is_ok(), "kind {} validates", kind.as_str());
            assert_eq!(
                task.identity.environment.is_empty(),
                kind != TaskKind::Doc,
                "env for kind {}",
                kind.as_str()
            );
        }
    }

    /// Conversion copies every field, including edge-case spellings.
    #[test]
    fn conversion_copies_every_field() {
        let mut group = group();
        group.manifest_key = "crates/a".to_owned();
        group.kind = TaskKind::Nextest;
        group.features = vec!["a".to_owned(), "b".to_owned()];
        group.target = "x86_64-unknown-linux-gnu".to_owned();
        group.gated_by = vec!["stack/rust/root/clippy/default".to_owned()];
        group.depends_on = vec!["stack/rust/root/build/default".to_owned()];
        group.target_flags = vec!["--tests".to_owned()];
        group.no_test_targets = true;
        group.compile_driver = CompileDriver::Mbx;
        group.test_runner = TestRunner::CargoNextest;
        group.declared_inputs = vec!["proto/a.proto".to_owned()];
        group.undeclared_reads = true;
        group.uses_network = true;
        group.uses_clock = true;
        group.uses_random = true;
        let task = propose_task(&group).expect("valid group proposes");
        assert_eq!(task.task_kind, "nextest");
        assert_eq!(task.configuration, group.configuration);
        assert_eq!(task.depends_on, group.depends_on);
        assert_eq!(task.gated_by, group.gated_by);
        assert_eq!(task.reads, vec!["crates/a/Cargo.toml".to_owned()]);
        assert!(task.writes.is_empty() && task.outputs.is_empty());
        assert_eq!(task.resource.class, ResourceClass::Test);
        assert!(task.resource.needs_network);
        assert!(task.cache_policy.allow_compilation_reuse);
        assert!(!task.cache_policy.allow_task_reuse);
        let identity = &task.identity;
        assert_eq!(identity.unit_id, group.package_id);
        assert_eq!(identity.unit_key, "crates/a");
        assert_eq!(identity.unit_path, "crates/a/Cargo.toml");
        assert_eq!(identity.project_root, "crates/a");
        assert_eq!(identity.target, group.target);
        assert_eq!(identity.features, group.features);
        assert_eq!(identity.flags, group.target_flags);
        assert_eq!(identity.compile_driver, "mbx");
        assert_eq!(identity.test_runner, "cargo_nextest");
        assert_eq!(identity.declared_inputs, group.declared_inputs);
        assert!(identity.undeclared_reads);
        assert!(identity.environment.is_empty());
        assert_eq!(task.display_name, group.package_name);
        assert!(task.uses_clock && task.uses_random && task.no_targets);
        assert_eq!(task.runner_profile, "default");
        assert_eq!(task.component_id, "demo@0.1.0");
        assert!(task.validate().is_ok());
    }

    /// Leading-dash values fail the proposal with the payload error.
    #[test]
    fn proposal_rejects_leading_dash_target() {
        let mut group = group();
        group.target = "-bad".to_owned();
        let err = propose_task(&group).expect_err("leading dash must fail");
        assert!(err.to_string().contains("leading_dash_target"), "{err}");
    }

    /// Dispatch helpers pin every kind spelling and rank.
    #[test]
    fn dispatch_helpers_pin_spellings() {
        assert_eq!(task_kind_rank("fmt"), 0);
        assert_eq!(task_kind_rank("nextest"), 3);
        assert_eq!(task_kind_rank("bogus"), u32::MAX);
        assert!(is_clippy_kind("clippy"));
        assert!(!is_clippy_kind("test"));
        assert!(is_nextest_kind("nextest"));
        assert!(is_workspace_fmt_task("fmt", "", ""));
        assert!(!is_workspace_fmt_task("fmt", "a", ""));
        assert_eq!(step_base_name("clippy", "Format"), "Clippy");
        assert_eq!(step_base_name("fmt", "Format"), "Format");
        assert_eq!(step_base_name("bogus", "Format"), "bogus");
        assert_eq!(KIND_DISPLAY_WORDS.len(), 7);
        let needs = tool_needs("mbx", "cargo_nextest");
        assert!(needs.mbx && needs.nextest);
        assert!(!tool_needs("cargo", "cargo_test").mbx);
        assert_eq!(payload_env_for_kind("doc").len(), 1);
        assert_eq!(
            payload_env_for_kind("test"),
            [] as [(std::ffi::OsString, std::ffi::OsString); 0]
        );
        assert_eq!(
            payload_env_for_kind("bogus"),
            [] as [(std::ffi::OsString, std::ffi::OsString); 0]
        );
    }
}
