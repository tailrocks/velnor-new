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
        "velnor-v2-sources-",
        "$GITHUB_WORKSPACE/Cargo.lock",
        "${{steps.cache-keys.outputs.sources_key}}",
        "${{needs.plan.outputs.sources_key}}",
    ] {
        assert!(yaml.contains(path), "missing {path} (mbx={mbx})");
    }
    for excluded in [
        "credentials.toml",
        "registry/src",
        "Swatinem/rust-cache",
        "velnor-v1-sources-",
    ] {
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
    for id in ["plan", "rust-demo"] {
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
    {
        let readers = job_names("rust-demo", mbx)?;
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
    use velnor_actions_contract::workflow::cache_family::SOURCES_SAVE_CONDITION;

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
    let produce = plan
        .iter()
        .position(|name| name == "Plan")
        .ok_or("plan producer")?;
    assert!(
        produce < save,
        "optional save must not block planning: {plan:?}"
    );
    let yaml = yaml_for(true)?;
    let publish = yaml.find("- name: Publish plan").ok_or("publish plan")?;
    let save_yaml = yaml
        .find("- name: Save Cargo sources")
        .ok_or("save sources")?;
    assert!(
        publish < save_yaml,
        "plan artifact precedes optional cache transport"
    );
    assert!(yaml.contains("velnor-optional-sources-save-budget.outputs.admitted"));
    assert!(yaml.contains("Record optional cache budget"));
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
    assert_eq!(step.condition.as_deref(), Some(SOURCES_SAVE_CONDITION));
    let fetch_step = job
        .steps
        .iter()
        .rfind(|step| step.role == Some(velnor_actions_contract::StepRole::CargoSourcesFetch))
        .ok_or("plan fetch step")?;
    assert_eq!(
        fetch_step.id,
        Some(velnor_actions_contract::StepId::SourcesFetch),
        "the save gates on the last fetch step's outcome"
    );
    assert_elected_writer(false)
}

/// The crate job restores both caches before fetching and its own tasks.
pub(super) fn assert_restore_before_obligations(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
) -> TestResult {
    {
        let id = "rust-demo";
        let job = prep.workflow.ir.jobs.get(id).ok_or("missing crate job")?;
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
        assert!(names.contains(&"Clippy"), "{id} retains Clippy");
    }
    Ok(())
}
