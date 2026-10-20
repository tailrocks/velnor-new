//! Per-crate feature resolution and test-less crate omission cases.
//!
//! Two-crate workspace fixture: `app` (lib with `process`/`pty`
//! features) plus `fuzz` (featureless except `extra`, one bin with
//! `test = false`, mirroring the termpane consumer preview).

use std::error::Error;
use std::fs;

use tempfile::TempDir;
use velnor_actions_contract::workflow::TaskExecutionManifestV1;
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{
    TestResult, err_of, fixture_manifest_json, git, plan_for, write_nextest_task,
};

/// Config with one `full` configuration requesting `process` and `pty`.
const FULL_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[[stacks.rust.configurations]]\nname = \"full\"\nfeatures = [\"process\", \"pty\"]\ntarget = \"host\"\n";
const ALL_FEATURES_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[[stacks.rust.configurations]]\nname = \"all\"\nall_features = true\nfeatures = []\ntarget = \"host\"\n";

/// Build the two-crate workspace repo (uncommitted) under `config`.
fn feature_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), config)?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\", \"crates/fuzz\"]\n",
    )?;
    fs::create_dir_all(root.join("crates/app/src"))?;
    fs::write(
        root.join("crates/app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[features]\ndefault = []\nprocess = []\npty = []\n",
    )?;
    fs::write(root.join("crates/app/src/lib.rs"), "pub fn f() {}\n")?;
    fs::create_dir_all(root.join("crates/fuzz/src"))?;
    fs::write(
        root.join("crates/fuzz/Cargo.toml"),
        "[package]\nname = \"fuzz\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[[bin]]\nname = \"fuzz\"\npath = \"src/main.rs\"\ntest = false\n[features]\nextra = []\n",
    )?;
    fs::write(root.join("crates/fuzz/src/main.rs"), "fn main() {}\n")?;
    write_nextest_task(root)?;
    Ok(dir)
}

#[path = "impl_orch_features/all_features.rs"]
mod all_features;

/// Sorted features of every `kind` task for `package`.
fn features_of(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
    package: &str,
    kind: velnor_actions_rust::TaskKind,
) -> Vec<Vec<String>> {
    let mut found: Vec<Vec<String>> = prep
        .discovery
        .proposals
        .iter()
        .filter(|task| task.display_name == package && task.task_kind == kind.as_str())
        .map(|task| task.identity.features.clone())
        .collect();
    found.sort();
    found
}

#[path = "impl_orch_features_assertions.rs"]
mod assertions;
use assertions::{
    assert_fuzz_task_records, assert_generated_nextest_commands, has_argv_pair, task_manifest,
    task_record,
};

#[test]
fn config_features_intersect_per_crate() -> TestResult {
    let repo = feature_repo(FULL_CONFIG)?;
    let prep = prepare(repo.path())?;
    assert_eq!(
        features_of(&prep, "app", velnor_actions_rust::TaskKind::Clippy),
        vec![vec!["process".to_owned(), "pty".to_owned()]],
        "featured crate applies the full request"
    );
    assert_eq!(
        features_of(&prep, "fuzz", velnor_actions_rust::TaskKind::Clippy),
        vec![vec!["default".to_owned()]],
        "featureless crate falls back to its defaults"
    );
    assert_eq!(prep.discovery.feature_fallbacks.len(), 1);
    let fallback = &prep.discovery.feature_fallbacks[0];
    assert_eq!(fallback.package_name, "fuzz");
    assert_eq!(fallback.configuration, "full");
    assert_eq!(
        fallback.requested,
        vec!["process".to_owned(), "pty".to_owned()]
    );
    assert_eq!(fallback.applied, vec!["default".to_owned()]);
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains(
            "Features: fuzz [full] declares none of [process,pty]; using default features"
        ),
        "fallback recorded:\n{plan}"
    );
    let tree = render_staged_tree(&prep)?;
    let manifest = task_manifest(&tree)?;
    let yaml = tree
        .get(".github/workflows/ci.yml")
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    let app_clippy = task_record(
        &prep,
        &manifest,
        "app",
        velnor_actions_rust::TaskKind::Clippy,
    )?;
    assert!(
        has_argv_pair(app_clippy, "--package", "app")
            && has_argv_pair(app_clippy, "--features", "process,pty")
            && app_clippy
                .argv
                .iter()
                .any(|arg| arg == "--no-default-features"),
        "app task record carries the requested feature set: {:?}",
        app_clippy.argv
    );
    assert!(
        yaml.contains(&app_clippy.execution_digest),
        "workflow binds the app Clippy record"
    );
    let fuzz_clippy = task_record(
        &prep,
        &manifest,
        "fuzz",
        velnor_actions_rust::TaskKind::Clippy,
    )?;
    assert!(has_argv_pair(fuzz_clippy, "--package", "fuzz"));
    assert!(
        yaml.contains(&fuzz_clippy.execution_digest),
        "workflow binds the fuzz Clippy record"
    );
    assert_fuzz_task_records(&prep, &manifest, yaml)?;
    Ok(())
}

#[test]
fn all_features_uses_each_packages_complete_feature_set() -> TestResult {
    let repo = feature_repo(ALL_FEATURES_CONFIG)?;
    let prep = prepare(repo.path())?;
    assert_eq!(prep.discovery.feature_fallbacks.len(), 0);
    assert_eq!(
        features_of(&prep, "app", velnor_actions_rust::TaskKind::Clippy),
        vec![vec![
            "default".to_owned(),
            "process".to_owned(),
            "pty".to_owned()
        ]]
    );
    assert_eq!(
        features_of(&prep, "fuzz", velnor_actions_rust::TaskKind::Clippy),
        vec![vec!["extra".to_owned()]]
    );
    let tree = render_staged_tree(&prep)?;
    let manifest = task_manifest(&tree)?;
    assert_all_features_commands(&prep, &manifest)?;
    let extended_repo = feature_repo(ALL_FEATURES_CONFIG)?;
    add_app_feature(&extended_repo)?;
    let extended = prepare(extended_repo.path())?;
    assert_eq!(
        features_of(&extended, "app", velnor_actions_rust::TaskKind::Clippy),
        vec![vec![
            "default".to_owned(),
            "extra".to_owned(),
            "process".to_owned(),
            "pty".to_owned()
        ]]
    );
    let extended_clippy = clippy_proposal(&extended, "app")?;
    let base_clippy = clippy_proposal(&prep, "app")?;
    assert_ne!(
        base_clippy.identity.features,
        extended_clippy.identity.features
    );
    Ok(())
}

fn clippy_proposal<'a>(
    prep: &'a velnor_actions_orchestrator::GenerationPreparation,
    package: &str,
) -> Result<&'a velnor_actions_contract::ProposedTask, Box<dyn Error>> {
    prep.discovery
        .proposals
        .iter()
        .find(|task| task.display_name == package && task.task_kind == "clippy")
        .ok_or_else(|| std::io::Error::other(format!("missing {package} Clippy proposal")).into())
}

fn add_app_feature(repo: &TempDir) -> Result<(), Box<dyn Error>> {
    let manifest_path = repo.path().join("crates/app/Cargo.toml");
    let original = fs::read_to_string(&manifest_path)?;
    fs::write(
        &manifest_path,
        original.replace("pty = []\n", "pty = []\nextra = []\n"),
    )?;
    Ok(())
}

fn assert_all_features_commands(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
    manifest: &TaskExecutionManifestV1,
) -> TestResult {
    let app_clippy = task_record(prep, manifest, "app", velnor_actions_rust::TaskKind::Clippy)?;
    assert!(app_clippy.argv.iter().any(|arg| arg == "--all-features"));
    assert!(
        !app_clippy
            .argv
            .iter()
            .any(|arg| arg == "--no-default-features" || arg == "--features")
    );
    for kind in [
        velnor_actions_rust::TaskKind::Build,
        velnor_actions_rust::TaskKind::Nextest,
        velnor_actions_rust::TaskKind::Doctest,
    ] {
        let record = task_record(prep, manifest, "app", kind)?;
        assert!(
            record.argv.iter().any(|arg| arg == "--all-features"),
            "{kind:?} argv: {:?}",
            record.argv
        );
    }
    let proposal = clippy_proposal(prep, "app")?;
    assert!(
        proposal
            .identity
            .flags
            .iter()
            .any(|flag| flag == "--all-features")
    );
    Ok(())
}

#[test]
fn testless_crate_omits_test_runners() -> TestResult {
    let repo = feature_repo(FULL_CONFIG)?;
    let prep = prepare(repo.path())?;
    let fuzz_tests: Vec<&velnor_actions_contract::ProposedTask> = prep
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.display_name == "fuzz"
                && matches!(task.task_kind.as_str(), "nextest" | "test" | "doctest")
        })
        .collect();
    assert!(!fuzz_tests.is_empty(), "fuzz test tasks derived");
    assert!(
        fuzz_tests.iter().all(|task| task.no_targets),
        "every fuzz test task ineligible"
    );
    let app_nextest: Vec<&velnor_actions_contract::ProposedTask> = prep
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.display_name == "app"
                && task.task_kind == velnor_actions_rust::TaskKind::Nextest.as_str()
        })
        .collect();
    assert_eq!(app_nextest.len(), 1, "app keeps its nextest task");
    assert!(!app_nextest[0].no_targets);
    let plan = plan_for(&prep)?;
    assert!(
        plan.contains("Ineligible: stack/rust/crates/fuzz/nextest/full has no test targets"),
        "fuzz ineligibility recorded:\n{plan}"
    );
    let critical = plan
        .lines()
        .find(|line| line.contains("Critical path:"))
        .ok_or_else(|| std::io::Error::other("missing critical path"))?;
    for task in &prep.discovery.proposals {
        if task.no_targets {
            assert!(
                !critical.contains(&task.task_id),
                "ineligible {} excluded from critical path:{critical}",
                task.task_id
            );
        }
    }
    let tree = render_staged_tree(&prep)?;
    let manifest = task_manifest(&tree)?;
    let yaml = tree
        .get(".github/workflows/ci.yml")
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    assert_generated_nextest_commands(&prep, &manifest, yaml)?;
    Ok(())
}

#[test]
fn partial_feature_subset_applies_intersection() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[[stacks.rust.configurations]]\nname = \"mixed\"\nfeatures = [\"extra\", \"process\"]\ntarget = \"host\"\n";
    let repo = feature_repo(config)?;
    let prep = prepare(repo.path())?;
    assert_eq!(
        features_of(&prep, "app", velnor_actions_rust::TaskKind::Clippy),
        vec![vec!["process".to_owned()]],
        "app applies its declared subset"
    );
    assert_eq!(
        features_of(&prep, "fuzz", velnor_actions_rust::TaskKind::Clippy),
        vec![vec!["extra".to_owned()]],
        "fuzz applies its declared subset"
    );
    assert_eq!(prep.discovery.feature_fallbacks.len(), 2);
    let plan = plan_for(&prep)?;
    for line in [
        "Features: app [mixed] requested [extra,process]; applied [process]",
        "Features: fuzz [mixed] requested [extra,process]; applied [extra]",
    ] {
        assert!(plan.contains(line), "partial narrowing recorded:\n{plan}");
    }
    Ok(())
}

#[test]
fn unknown_config_feature_fails_closed_naming_crate() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[[stacks.rust.configurations]]\nname = \"full\"\nfeatures = [\"typo_feat\"]\ntarget = \"host\"\n";
    let repo = feature_repo(config)?;
    let err = err_of(prepare(repo.path()), "typo feature")?;
    assert!(
        err.to_string()
            .contains("unknown_feature:full:app:typo_feat"),
        "names crate and feature: {err}"
    );
    Ok(())
}
