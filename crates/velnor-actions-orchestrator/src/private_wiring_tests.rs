use std::collections::BTreeMap;
use std::fs;

use velnor_actions_contract::{Job, JobTimeout, Step, StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;

use super::*;
use crate::config::load_config;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

const BASE: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"main\"\n";
const TABLE: &str = "[private_sources]\nowner = \"ChainArgos\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\napp_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\n";
const CARGO_LOCK: &str = "[[package]]\nname = \"shared\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/processor-shared#0123456789abcdef0123456789abcdef01234567\"\n[[package]]\nname = \"domain\"\nversion = \"1\"\nsource = \"git+https://github.com/ChainArgos/eth-domain#0123456789abcdef0123456789abcdef01234567\"\n";

fn workspace(config: &str, cargo_lock: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("dir");
    fs::create_dir_all(dir.path().join(".velnor")).expect("velnor");
    fs::write(dir.path().join(".velnor/config.toml"), config).expect("config");
    fs::write(dir.path().join("Cargo.lock"), cargo_lock).expect("lock");
    dir
}

fn wiring(dir: &tempfile::TempDir) -> Result<Option<PrivateWiring>, OrchestratorError> {
    let config = load_config(dir.path())?;
    PrivateWiring::load(dir.path(), &config, &ToolCatalog::pinned())
}

fn shell(name: &str, role: Option<StepRole>) -> Step {
    Step {
        parallel: false,
        name: name.to_owned(),
        id: None,
        role,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        services: Vec::new(),
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps,
        cache_mode: None,
    }
}

fn names(job: &Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

#[test]
fn no_table_means_no_wiring() {
    let dir = workspace(BASE, CARGO_LOCK);
    assert!(wiring(&dir).expect("load").is_none());
}

#[test]
fn only_consumer_workflows_can_use_private_sources() {
    let dir = workspace(
        &format!("{BASE}{TABLE}").replace(
            "name = \"CI\"",
            "name = \"CI\"\npolicy = \"velnor-repository-v1\"",
        ),
        CARGO_LOCK,
    );
    let error = wiring(&dir).expect_err("rejected");
    assert!(
        error.to_string().contains("requires_consumer_policy"),
        "{error}"
    );
}

#[test]
fn declared_sources_need_a_matching_lock() {
    let declared = format!(
        "{BASE}{TABLE}[[private_sources.git_trees]]\nid = \"h\"\nrepository = \"ChainArgos/java-monorepo\"\ntag = \"t\"\n"
    );
    let dir = workspace(&declared, CARGO_LOCK);
    assert!(
        wiring(&dir)
            .expect_err("missing")
            .to_string()
            .contains("deps_lock_missing")
    );
    fs::write(
        dir.path().join(".velnor/deps.lock"),
        format!(
            "schema = 1\n[[git_trees]]\nid = \"h\"\nrepository = \"ChainArgos/java-monorepo\"\ntag = \"other\"\ncommit = \"{COMMIT}\"\n"
        ),
    )
    .expect("lock");
    assert!(
        wiring(&dir)
            .expect_err("drift")
            .to_string()
            .contains("deps_lock_drift:h")
    );
}

#[test]
fn the_allowlist_is_sorted_and_scoped_to_the_roots() {
    let dir = workspace(&format!("{BASE}{TABLE}"), CARGO_LOCK);
    let wiring = wiring(&dir).expect("load").expect("some");
    let steps = wiring.steps_for_roots(&[String::new()]).expect("steps");
    assert_eq!(steps.len(), 3);
    let StepKind::Action { with, .. } = &steps[0].kind else {
        panic!("mint is an action");
    };
    assert_eq!(with["repositories"], "eth-domain,processor-shared");
    assert!(
        wiring
            .steps_for_roots(&["no-lock-here".to_owned()])
            .expect("steps")
            .is_empty(),
        "a root without a lock has no closure"
    );
    assert_eq!(wiring.steps_for_roots(&[]).expect("steps").len(), 0);
}

#[test]
fn the_block_lands_before_cargo_fetch_and_pulls_the_helper_up() {
    let dir = workspace(&format!("{BASE}{TABLE}"), CARGO_LOCK);
    let wiring = wiring(&dir).expect("load").expect("some");
    let mut plan = job(vec![
        shell("Checkout", None),
        shell("Fetch Cargo sources", Some(StepRole::CargoSourcesFetch)),
        shell("Save sources", None),
        shell("Acquire Velnor", Some(StepRole::AcquireVelnor)),
        shell("Plan", None),
    ]);
    wiring
        .insert_into_job(&mut plan, &[String::new()])
        .expect("insert");
    assert_eq!(
        names(&plan),
        [
            "Checkout",
            "Acquire Velnor",
            "Mint read-only token for private sources",
            "Fetch private sources",
            "Revoke private-source token",
            "Fetch Cargo sources",
            "Save sources",
            "Plan",
        ]
    );
    let mut staged_early = job(vec![
        shell("Acquire Velnor", Some(StepRole::AcquireVelnor)),
        shell("Fetch Cargo sources", Some(StepRole::CargoSourcesFetch)),
    ]);
    wiring
        .insert_into_job(&mut staged_early, &[String::new()])
        .expect("insert");
    assert_eq!(names(&staged_early)[0], "Acquire Velnor");
    assert_eq!(
        names(&staged_early)
            .iter()
            .filter(|n| **n == "Acquire Velnor")
            .count(),
        1
    );
    let mut empty_closure = job(vec![shell(
        "Fetch Cargo sources",
        Some(StepRole::CargoSourcesFetch),
    )]);
    wiring
        .insert_into_job(&mut empty_closure, &["elsewhere".to_owned()])
        .expect("noop");
    assert_eq!(empty_closure.steps.len(), 1);
    let mut bun_only = job(vec![shell("Plan", None)]);
    wiring
        .insert_into_job(&mut bun_only, &[])
        .expect("a job without Cargo roots takes no block");
    assert_eq!(bun_only.steps.len(), 1);
    let mut no_fetch = job(vec![shell("Plan", None)]);
    assert!(
        wiring
            .insert_into_job(&mut no_fetch, &[String::new()])
            .is_err()
    );
}

#[path = "private_wiring_workflow_tests.rs"]
mod workflow;
