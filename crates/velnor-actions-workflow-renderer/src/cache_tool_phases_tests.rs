//! Planning/full tool-cache phase boundaries and read-only consumer policy.

use std::collections::BTreeMap;

use super::*;
use velnor_actions_contract::{
    Job, JobTimeout, SourceBoundOperation, Step, StepKind, ToolCacheDomain,
};

const RUNS_ON: &str = "ubuntu-24.04";
const TARGET: &str = "x86_64-unknown-linux-gnu";
const PLANNING_SPECS: [&str; 4] = [
    "gh@2.102.0",
    "actionlint@1.7.12",
    "shellcheck@0.11.0",
    "zizmor@1.30.1",
];

fn setup() -> MiseSetup {
    crate::setup::fixture::mise_setup("2026.9.16", &"a".repeat(64))
}

fn fixture_records(setup: &MiseSetup) -> Vec<velnor_actions_contract::CompiledSourceHelper> {
    setup
        .bootstraps
        .values()
        .map(|bootstrap| bootstrap.helper.clone())
        .collect()
}

fn ensure(job_id: &str, job: &mut Job, setup: &MiseSetup, target: &str) -> Result<(), RenderError> {
    let records = fixture_records(setup);
    super::ensure(job_id, job, setup, target, &records)
}

fn install_step(name: &str, specs: &[&str]) -> Step {
    let mut argv = vec![
        "mise".to_owned(),
        "--no-config".to_owned(),
        "install".to_owned(),
    ];
    argv.extend(specs.iter().map(|spec| (*spec).to_owned()));
    crate::steps::ambient_shell_step(name, argv, BTreeMap::new()).expect("install step")
}

fn fixture() -> Job {
    let mut rust = install_step("Prepare Rust", &["rust@1.98.1"]);
    rust.condition = Some(crate::early_plan::NEEDS_CARGO_CONDITION.to_owned());
    Job {
        cache_mode: None,
        display_name: "Phased tools".to_owned(),
        runs_on: RUNS_ON.to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps: vec![
            install_step("Install planning tools", &PLANNING_SPECS),
            crate::early_plan::early_plan_step().expect("early plan"),
            rust,
            crate::steps::plan_step(),
        ],
    }
}

fn planning_only_fixture() -> Job {
    let mut job = fixture();
    job.steps.remove(2);
    job.display_name = "Planning-only tools".to_owned();
    job
}

fn rendered() -> Job {
    let mut job = fixture();
    ensure("rust-demo", &mut job, &setup(), TARGET).expect("phase setup");
    job
}

fn at_id(job: &Job, id: &str) -> usize {
    job.steps
        .iter()
        .position(|step| step.id.as_ref().is_some_and(|value| value.as_str() == id))
        .expect("step id")
}

fn at_name(job: &Job, name: &str) -> usize {
    job.steps
        .iter()
        .position(|step| step.name == name)
        .expect("step name")
}

fn named<'a>(job: &'a Job, name: &str) -> &'a Step {
    job.steps
        .iter()
        .find(|step| step.name == name)
        .expect("named step")
}

fn action_with(step: &Step) -> &BTreeMap<String, String> {
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("action step")
    };
    with
}

fn shell_env(step: &Step) -> &BTreeMap<String, String> {
    let StepKind::Shell { env, .. } = &step.kind else {
        panic!("shell step")
    };
    env
}

fn bootstrap<'a>(job: &'a Job) -> &'a Step {
    job.steps
        .iter()
        .find(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == SourceBoundOperation::MiseBootstrap)
        })
        .expect("compiled Mise bootstrap")
}

fn full_bootstrap<'a>(job: &'a Job) -> &'a Step {
    job.steps
        .iter()
        .find(|step| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
                if invocation.descriptor().operation() == SourceBoundOperation::MiseBootstrap
                    && step.condition.as_deref()
                        == Some(crate::early_plan::NEEDS_CARGO_CONDITION))
        })
        .expect("guarded full Mise bootstrap")
}

fn preflight<'a>(job: &'a Job, domain: &str) -> &'a Step {
    job.steps
        .iter()
        .find(|step| {
            step.name == "Verify restored Mise binary"
                && shell_env(step)
                    .get("VELNOR_MISE_CACHE_DOMAIN")
                    .map(String::as_str)
                    == Some(domain)
        })
        .expect("domain preflight")
}

fn save_count(job: &Job) -> usize {
    job.steps
        .iter()
        .filter(|step| {
            matches!(&step.kind, StepKind::Action { uses, .. }
                if uses.starts_with("actions/cache/save@"))
        })
        .count()
}

#[test]
fn cold_phase_layout_bootstraps_planning_and_guards_full_fallback() {
    let job = rendered();
    let early = at_id(&job, crate::early_plan::EARLY_PLAN_STEP_ID);
    let planning_restore = at_id(&job, PLANNING_RESTORE_ID);
    let planning_bootstrap_at = at_name(&job, crate::setup::SETUP_MISE_NAME);
    let full_restore = at_id(&job, crate::cache_steps::TOOLS_RESTORE_ID);
    let full_preflight = at_name(&job, "Verify restored Mise binary");
    let full_bootstrap_step = full_bootstrap(&job);
    let full_bootstrap_at = job
        .steps
        .iter()
        .position(|step| step == full_bootstrap_step)
        .expect("full bootstrap index");
    let full_prepare = at_name(&job, "Prepare Rust");

    assert_eq!(
        job.steps
            .iter()
            .filter(|step| {
                matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
            if invocation.descriptor().operation() == SourceBoundOperation::MiseBootstrap)
            })
            .count(),
        2
    );
    assert!(planning_restore < planning_bootstrap_at);
    assert!(planning_bootstrap_at < early);
    assert!(early < full_restore && full_restore < full_preflight);
    assert!(full_preflight < full_bootstrap_at && full_bootstrap_at < full_prepare);
    assert_eq!(
        action_with(named(&job, PLANNING_RESTORE_NAME))
            .get("path")
            .map(String::as_str),
        Some(PLANNING_ROOT)
    );
    let StepKind::SourceBoundHelper { env, .. } = &bootstrap(&job).kind else {
        panic!("compiled bootstrap")
    };
    assert_eq!(
        env.get("MISE_DATA_DIR").map(String::as_str),
        Some(PLANNING_ROOT)
    );
    assert_eq!(
        env.get("VELNOR_MISE_VERSION").map(String::as_str),
        Some("2026.9.16")
    );
    assert_eq!(bootstrap(&job).condition.as_deref(), None);
    assert_eq!(
        full_bootstrap(&job).condition.as_deref(),
        Some(crate::early_plan::NEEDS_CARGO_CONDITION)
    );
    let StepKind::SourceBoundHelper { env, .. } = &full_bootstrap_step.kind else {
        panic!("guarded full bootstrap")
    };
    assert_eq!(
        env.get("MISE_DATA_DIR").map(String::as_str),
        Some(ToolCacheDomain::Full.root())
    );
    assert_eq!(
        job.steps
            .iter()
            .filter(|step| step.name == "Verify restored Mise binary")
            .count(),
        1,
        "full preflight is the only standalone preflight"
    );
    assert_eq!(
        shell_env(named(&job, "Install planning tools"))
            .get("MISE_DATA_DIR")
            .map(String::as_str),
        Some(PLANNING_ROOT)
    );
    assert_eq!(
        shell_env(named(&job, "Prepare Rust"))
            .get("MISE_DATA_DIR")
            .map(String::as_str),
        Some(ToolCacheDomain::Full.root())
    );

    let full = named(&job, crate::cache_steps::TOOLS_RESTORE_NAME);
    assert_eq!(
        full.condition.as_deref(),
        Some(crate::early_plan::NEEDS_CARGO_CONDITION)
    );
    assert_ne!(full.condition.as_deref(), Some("false"));
    assert_eq!(
        preflight(&job, "tools").condition.as_deref(),
        Some(crate::early_plan::NEEDS_CARGO_CONDITION)
    );
}

#[test]
fn read_only_consumers_share_restores_without_writers() {
    let jobs = BTreeMap::from([
        ("rust-a".to_owned(), rendered()),
        ("rust-b".to_owned(), rendered()),
    ]);
    crate::cache_p08::validate_tool_consumers(&jobs, &setup(), &[]).expect("read-only consumers");
    for job in jobs.values() {
        assert_eq!(save_count(job), 0);
        assert_eq!(
            preflight(job, "tools").condition.as_deref(),
            Some(crate::early_plan::NEEDS_CARGO_CONDITION)
        );
    }
}

#[test]
fn phase_render_is_idempotent_and_keeps_consumer_read_only() {
    let mut job = fixture();
    ensure("rust-demo", &mut job, &setup(), TARGET).expect("first phase setup");
    let first = job.steps.clone();
    ensure("rust-demo", &mut job, &setup(), TARGET).expect("repeat phase setup");
    assert_eq!(job.steps, first);
    assert_eq!(save_count(&job), 0);
    let jobs = BTreeMap::from([("rust-demo".to_owned(), job)]);
    crate::cache_p08::validate_tool_consumers(&jobs, &setup(), &[]).expect("readonly validation");
}

#[test]
fn empty_full_suffix_uses_the_readonly_mise_bootstrap_fallback() {
    let mut job = planning_only_fixture();
    ensure("planning-only", &mut job, &setup(), TARGET).expect("planning-only setup");
    let first = job.steps.clone();
    ensure("planning-only", &mut job, &setup(), TARGET).expect("repeat setup");
    assert_eq!(job.steps, first);
    assert_eq!(save_count(&job), 0);

    let planning_specs = PLANNING_SPECS
        .iter()
        .map(|spec| (*spec).to_owned())
        .collect::<Vec<_>>();
    let planning_base =
        crate::cache_p08::mise_cache_key_for_tools(TARGET, &setup().version, &planning_specs)
            .expect("planning key")
            .replacen("mise-v3-", "mise-v3-planning-", 1);
    let expected_planning = format!("{planning_base}-${{{{env.VELNOR_CACHE_IMAGE}}}}");
    let expected_full = format!(
        "{}-${{{{env.VELNOR_CACHE_IMAGE}}}}",
        crate::cache_p08::mise_cache_key_for_tools(
            TARGET,
            &setup().version,
            &["mise@bootstrap".to_owned()],
        )
        .expect("full bootstrap fallback key")
    );
    let expected_planning_snapshot = format!("{expected_planning}-snapshot-");
    let expected_full_snapshot = format!("{expected_full}-snapshot-");
    assert_eq!(
        action_with(named(&job, PLANNING_RESTORE_NAME)).get("restore-keys"),
        Some(&expected_planning_snapshot)
    );
    assert_eq!(
        action_with(named(&job, crate::cache_steps::TOOLS_RESTORE_NAME)).get("restore-keys"),
        Some(&expected_full_snapshot)
    );
    assert_eq!(
        named(&job, crate::cache_steps::TOOLS_RESTORE_NAME)
            .condition
            .as_deref(),
        Some(crate::early_plan::NEEDS_CARGO_CONDITION)
    );
    assert_eq!(
        full_bootstrap(&job).condition.as_deref(),
        Some(crate::early_plan::NEEDS_CARGO_CONDITION)
    );
}

#[test]
fn malformed_cache_preflight_and_platform_controls_reject() {
    let mut foreign = fixture();
    let transport = crate::cache_steps::cache_action_step(
        true,
        crate::cache_steps::TOOLS_RESTORE_USES,
        "tools",
        "foreign",
        &[],
        &[PLANNING_ROOT.to_owned()],
    )
    .expect("foreign transport");
    foreign.steps.insert(0, transport);
    assert!(ensure("foreign", &mut foreign, &setup(), TARGET).is_err());

    let mut bad_cache = rendered();
    let planning_restore_at = at_id(&bad_cache, PLANNING_RESTORE_ID);
    if let StepKind::Action { with, .. } = &mut bad_cache.steps[planning_restore_at].kind {
        with.insert("restore-keys".to_owned(), "foreign".to_owned());
    }
    assert!(ensure("bad-cache", &mut bad_cache, &setup(), TARGET).is_err());

    let mut bad_preflight = rendered();
    let preflight = bad_preflight
        .steps
        .iter_mut()
        .find(|step| step.name == "Verify restored Mise binary")
        .expect("preflight");
    if let StepKind::Shell { env, .. } = &mut preflight.kind {
        env.insert("VELNOR_MISE_SHA256".to_owned(), "b".repeat(64));
    }
    assert!(ensure("bad-preflight", &mut bad_preflight, &setup(), TARGET).is_err());

    let mut bad_platform = rendered();
    let platform = bad_platform
        .steps
        .iter_mut()
        .find(|step| step.name == "Resolve tool cache platform")
        .expect("platform");
    if let StepKind::Shell { env, .. } = &mut platform.kind {
        env.insert("MISE_DATA_DIR".to_owned(), "/tmp/foreign".to_owned());
    }
    assert!(ensure("bad-platform", &mut bad_platform, &setup(), TARGET).is_err());
}

#[path = "cache_tool_phases_negative_tests.rs"]
mod negative;
