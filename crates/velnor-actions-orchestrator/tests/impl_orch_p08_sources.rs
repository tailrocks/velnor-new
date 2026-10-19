//! P08 Cargo-source ownership and writer-election assertions.

use velnor_actions_contract::StepKind;

use crate::impl_common::TestResult;
use crate::impl_orch_p08::{job_names, preparation_for, yaml_for};

#[test]
fn c3_sources_subset_at_owned_home_single_writer() -> TestResult {
    for mbx in [false, true] {
        assert_source_yaml(mbx)?;
        assert_source_ir(mbx)?;
    }
    Ok(())
}

fn assert_source_yaml(mbx: bool) -> TestResult {
    let yaml = yaml_for(mbx)?;
    for path in [
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/git/db",
        "velnor-v1-sources-",
        "hashFiles('Cargo.lock')",
    ] {
        assert!(yaml.contains(path), "missing {path} (mbx={mbx})");
    }
    for excluded in ["credentials.toml", "registry/src", "Swatinem/rust-cache"] {
        assert!(
            !yaml.contains(excluded),
            "unexpected {excluded} (mbx={mbx})"
        );
    }
    Ok(())
}

fn assert_source_ir(mbx: bool) -> TestResult {
    let prep = preparation_for(mbx)?;
    let expected = [
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/git/db",
    ];
    for id in [
        "plan",
        "rust-demo-lint",
        "rust-demo-tests",
        "rust-demo-docs",
    ] {
        let job = prep
            .workflow
            .ir
            .jobs
            .get(id)
            .ok_or_else(|| std::io::Error::other(format!("missing {id}")))?;
        for step in job.steps.iter().filter(|step| {
            matches!(
                step.name.as_str(),
                "Restore Cargo sources" | "Save Cargo sources"
            )
        }) {
            let StepKind::Action { with, .. } = &step.kind else {
                return Err(format!("{} is not an action", step.name).into());
            };
            let paths = with
                .get("path")
                .ok_or_else(|| std::io::Error::other("source cache path"))?
                .lines()
                .collect::<Vec<_>>();
            assert_eq!(paths, expected, "source ownership in {id}");
        }
    }
    assert_elected_writer(mbx)
}

fn assert_elected_writer(mbx: bool) -> TestResult {
    let plan = job_names("plan", mbx)?;
    assert!(plan.contains(&"Save Cargo sources".to_owned()), "{plan:?}");
    for phase in ["lint", "tests", "docs"] {
        let readers = job_names(&format!("rust-demo-{phase}"), mbx)?;
        assert!(
            readers.contains(&"Restore Cargo sources".to_owned()),
            "{readers:?}"
        );
        assert!(
            !readers.contains(&"Save Cargo sources".to_owned()),
            "readers never save: {readers:?}"
        );
    }
    Ok(())
}

#[test]
fn c10_only_plan_saves_producer_successful_deltas() -> TestResult {
    use velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION;

    let plan = job_names("plan", true)?;
    let fetch = plan
        .iter()
        .position(|name| name == "Fetch Cargo sources")
        .ok_or("plan fetch")?;
    let save = plan
        .iter()
        .position(|name| name == "Save Cargo sources")
        .ok_or("plan save")?;
    assert!(fetch < save, "writer saves after fetch: {plan:?}");
    let prep = preparation_for(false)?;
    let job = prep
        .workflow
        .ir
        .jobs
        .get("plan")
        .ok_or_else(|| std::io::Error::other("missing plan"))?;
    let step = job
        .steps
        .iter()
        .find(|step| step.name == "Save Cargo sources")
        .ok_or("plan source save")?;
    assert_eq!(step.condition.as_deref(), Some(CACHE_SAVE_CONDITION));
    assert_elected_writer(false)
}

/// Each fixed Rust phase restores both caches before fetching and its own tasks.
pub(super) fn assert_restore_before_obligations(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
) -> TestResult {
    for phase in ["lint", "tests", "docs"] {
        let id = format!("rust-demo-{phase}");
        let job = prep.workflow.ir.jobs.get(&id).ok_or("missing phase")?;
        let names: Vec<_> = job.steps.iter().map(|step| step.name.as_str()).collect();
        let at = |name: &str| names.iter().position(|seen| *seen == name);
        let (Some(restore), Some(mbx), Some(fetch)) = (
            at("Restore Cargo sources"),
            at("Restore MBX objects"),
            at("Fetch Cargo sources"),
        ) else {
            return Err(format!("{id} cache order missing: {names:?}").into());
        };
        assert!(
            restore < mbx && mbx < fetch,
            "{id}: restore<mbx<fetch: {names:?}"
        );
        let tasks: Vec<_> = job
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| matches!(step.kind, StepKind::TaskExecution { .. }))
            .collect();
        assert!(!tasks.is_empty(), "{id} owns obligations");
        assert!(
            tasks.iter().all(|(at, _)| fetch < *at),
            "{id}: fetch precedes every obligation"
        );
        if phase == "lint" {
            assert!(names.contains(&"Clippy"), "{id} retains Clippy");
        }
    }
    Ok(())
}
