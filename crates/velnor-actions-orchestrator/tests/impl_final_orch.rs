//! Final orchestrator requirements: tool checks, schemas, paths.

use std::collections::BTreeMap;
use std::fs;

use crate::impl_common::{TestResult, config_with_branch, err_of, make_repo, plan_for};
use velnor_actions_orchestrator::{
    CONFLICTING_TOOL_VALUES, DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS, ExternalDataFreshness, ToolParse,
    UNSUPPORTED_TOOL_VALUE, check_tool_inputs, coverage_schema_known, critical_path_for_groups,
    external_data_kind, may_skip_external_data, prepare, render_critical_path,
    reuse_eligible_for_schema, tool_conflicts,
};

/// Config with default plus all-features Rust configurations.
const TWO_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
    [[stacks.rust.configurations]]\nname = \"default\"\nfeatures = [\"default\"]\n\
    target = \"host\"\n[[stacks.rust.configurations]]\nname = \"all-features\"\n\
    features = [\"all\"]\ntarget = \"host\"\n";

#[test]
fn tool_inputs_report_presence_parse_values_digests() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.85.0\"\n",
    )?;
    fs::write(root.join("mise.toml"), "[tools]\nrust = \"1.85.0\"\n")?;
    fs::write(root.join("mise.lock"), "{}\n")?;
    let prep = prepare(root)?;
    assert_eq!(prep.discovery.tool_checks.len(), 3);
    let mut digests = Vec::new();
    for check in &prep.discovery.tool_checks {
        assert!(check.present, "{}", check.path);
        assert_eq!(check.parse, ToolParse::Valid, "{}", check.path);
        let digest = check.digest.as_deref().ok_or("missing digest")?;
        assert!(digest.starts_with("b3-"), "{digest}");
        digests.push(digest.to_owned());
    }
    digests.sort();
    digests.dedup();
    assert_eq!(digests.len(), 3, "digests differ per file");
    let values: BTreeMap<(&str, &str), &str> = prep
        .discovery
        .tool_checks
        .iter()
        .flat_map(|check| {
            check
                .values
                .iter()
                .map(|(key, value)| ((check.path.as_str(), key.as_str()), value.as_str()))
        })
        .collect();
    assert_eq!(
        values.get(&("rust-toolchain.toml", "channel")),
        Some(&"1.85.0")
    );
    assert_eq!(values.get(&("mise.toml", "tools.rust")), Some(&"1.85.0"));
    assert!(tool_conflicts(&prep.discovery.tool_checks).is_empty());
    let text = plan_for(&prep)?;
    assert!(text.contains("mise.toml is read-only input"), "{text}");
    assert_eq!(check_tool_inputs(root), prep.discovery.tool_checks);
    Ok(())
}

#[test]
fn malformed_mise_wrapper_fails_closed_before_toolcheck() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("mise.toml"), "[tools\nrust = \n")?;
    let err = err_of(prepare(root).map(|_| ()), "malformed wrapper")?;
    assert!(
        err.to_string().contains("wrapper_invalid"),
        "diagnostic: {err}"
    );
    let mise = check_tool_inputs(root)
        .into_iter()
        .find(|check| check.path == "mise.toml")
        .ok_or("missing mise check")?;
    assert!(matches!(mise.parse, ToolParse::Invalid { .. }));
    assert!(mise.digest.is_some());
    Ok(())
}

#[test]
fn conflicting_tool_values_yield_findings() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.84.0\"\n",
    )?;
    fs::write(root.join("mise.toml"), "[tools]\nrust = \"1.85.0\"\n")?;
    let before = (
        fs::read(root.join("rust-toolchain.toml"))?,
        fs::read(root.join("mise.toml"))?,
    );
    let prep = prepare(root)?;
    let findings = tool_conflicts(&prep.discovery.tool_checks);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, CONFLICTING_TOOL_VALUES);
    findings[0].validate()?;
    assert!(
        prep.discovery
            .recommendations
            .iter()
            .any(|line| line.contains(CONFLICTING_TOOL_VALUES)),
        "{:?}",
        prep.discovery.recommendations
    );
    assert_eq!(fs::read(root.join("rust-toolchain.toml"))?, before.0);
    assert_eq!(fs::read(root.join("mise.toml"))?, before.1);
    Ok(())
}

#[test]
fn unsupported_tool_values_yield_findings() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(
        root.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"frobnicator\"\n",
    )?;
    let prep = prepare(root)?;
    let findings = tool_conflicts(&prep.discovery.tool_checks);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, UNSUPPORTED_TOOL_VALUE);
    findings[0].validate()?;
    assert!(
        prep.discovery
            .recommendations
            .iter()
            .any(|line| line.contains(UNSUPPORTED_TOOL_VALUE)),
        "{:?}",
        prep.discovery.recommendations
    );
    Ok(())
}

#[test]
fn unknown_extension_schema_disables_reuse_and_coverage() -> TestResult {
    assert!(coverage_schema_known("stack/rust/root/clippy/default"));
    assert!(!coverage_schema_known("stack/unknown/root/test/default"));
    assert!(reuse_eligible_for_schema("rust-task-identity-v1"));
    assert!(!reuse_eligible_for_schema("rust-task-v2"));
    let known = velnor_actions_contract::StackExtension {
        schema: "rust-task-identity-v1".to_owned(),
        data: serde_json::json!({}),
    };
    assert!(velnor_actions_contract::cachekey::stack_extension_id(&known).is_ok());
    let unknown = velnor_actions_contract::StackExtension {
        schema: "rust-task-v2".to_owned(),
        data: serde_json::json!({}),
    };
    let err = velnor_actions_contract::cachekey::stack_extension_id(&unknown)
        .err()
        .ok_or("unknown schema must be rejected")?;
    assert!(err.to_string().contains("unknown_schema"), "{err}");
    Ok(())
}

#[test]
fn plan_reports_structural_critical_path() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let text = plan_for(&prep)?;
    let line = text
        .lines()
        .find(|line| line.contains("Critical path:"))
        .ok_or("missing critical path line")?;
    assert!(line.contains("clippy"), "{line}");
    let chain: Vec<&str> = line
        .split_once("Critical path:")
        .ok_or("malformed line")?
        .1
        .split('(')
        .next()
        .ok_or("malformed line")?
        .split(" -> ")
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    assert!(chain.len() >= 2, "{line}");
    for pair in chain.windows(2) {
        let next = prep
            .discovery
            .proposals
            .iter()
            .find(|task| task.task_id == pair[1])
            .ok_or("chain task missing")?;
        assert!(
            next.gated_by.iter().any(|dep| dep == pair[0])
                || next.depends_on.iter().any(|dep| dep == pair[0]),
            "{} does not follow {}",
            pair[1],
            pair[0]
        );
    }
    Ok(())
}

#[test]
fn critical_path_reports_task_durations() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let mut durations = BTreeMap::new();
    for task in &prep.discovery.proposals {
        let weight = if task.task_id.contains("/test/") {
            500
        } else if task.task_id.contains("/clippy/") {
            100
        } else {
            10
        };
        durations.insert(task.task_id.clone(), weight);
    }
    let path = critical_path_for_groups(&prep.discovery.proposals, &durations);
    assert!(path.path.len() >= 2);
    assert!(path.path.last().is_some_and(|id| id.contains("/test/")));
    assert_eq!(path.total_duration_ms, 600);
    let lines = render_critical_path(&path, &durations);
    assert!(lines[0].contains("total 600 ms"), "{lines:?}");
    assert!(
        lines.iter().any(|line| line.contains("500 ms")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("100 ms")),
        "{lines:?}"
    );
    Ok(())
}

#[test]
fn external_data_skip_needs_declared_fresh_identity() -> TestResult {
    let fresh = ExternalDataFreshness {
        source: "advisory-db".to_owned(),
        identity: velnor_actions_contract::digest_b3(b"snapshot"),
        age_secs: 60,
    };
    fresh.validate()?;
    assert!(may_skip_external_data(
        true,
        Some(&fresh),
        DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS
    ));
    assert!(!may_skip_external_data(
        false,
        Some(&fresh),
        DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS
    ));
    assert!(!may_skip_external_data(
        true,
        None,
        DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS
    ));
    let stale = ExternalDataFreshness {
        age_secs: DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS + 1,
        ..fresh
    };
    assert!(!may_skip_external_data(
        true,
        Some(&stale),
        DEFAULT_EXTERNAL_DATA_MAX_AGE_SECS
    ));
    assert_eq!(
        external_data_kind("stack/rust/root/advisory/default"),
        Some("advisory")
    );
    assert_eq!(external_data_kind("stack/rust/root/clippy/default"), None);
    Ok(())
}

#[test]
fn clippy_configs_schedule_in_separate_groups() -> TestResult {
    let repo = make_repo(TWO_CONFIG)?;
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[features]\nall = []\n",
    )?;
    let prep = prepare(repo.path())?;
    let memory = &prep.discovery.clippy_memory;
    assert_eq!(memory.groups.len(), 2);
    assert_eq!(memory.barriers, 1);
    let mut clippy: Vec<&str> = prep
        .discovery
        .proposals
        .iter()
        .filter(|task| task.task_id.contains("/clippy/"))
        .map(|task| task.task_id.as_str())
        .collect();
    clippy.sort_unstable();
    assert_eq!(clippy.len(), 2);
    let mut scheduled: Vec<&str> = memory
        .groups
        .iter()
        .flatten()
        .filter(|id| id.contains("/clippy/"))
        .map(String::as_str)
        .collect();
    scheduled.sort_unstable();
    assert_eq!(scheduled, clippy);
    let mut union: Vec<&str> = memory.groups.iter().flatten().map(String::as_str).collect();
    union.sort_unstable();
    let mut all: Vec<&str> = prep
        .discovery
        .proposals
        .iter()
        .map(|task| task.task_id.as_str())
        .collect();
    all.sort_unstable();
    assert_eq!(union, all, "no check removed");
    assert!(
        plan_for(&prep)?.contains("Clippy memory groups: 2; barriers: 1"),
        "schedule reported"
    );
    Ok(())
}

#[test]
fn single_clippy_config_needs_no_barrier() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let memory = &prep.discovery.clippy_memory;
    assert_eq!(memory.groups.len(), 1);
    assert_eq!(memory.barriers, 0);
    assert!(
        plan_for(&prep)?.contains("Clippy memory groups: 1; barriers: 0"),
        "schedule reported"
    );
    Ok(())
}
