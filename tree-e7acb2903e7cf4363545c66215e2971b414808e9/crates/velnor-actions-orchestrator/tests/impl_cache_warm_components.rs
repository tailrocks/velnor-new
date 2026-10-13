//! Rust component installation stays tied to actual component consumers.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, StepKind, StepRole};
use velnor_actions_mise::PREPARE_RUST_COMPONENTS_STEP;
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{SETUP_MISE_NAME, render::WORKFLOW_PATH};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn rust_components_install_unconditionally_after_restore() -> TestResult {
    for mbx in [false, true] {
        assert_ir_component_order(mbx)?;
        assert_yaml_component_order(mbx)?;
    }
    Ok(())
}

fn assert_ir_component_order(mbx: bool) -> TestResult {
    let repo = super::make_workspace(&super::lock_for(&["a", "b"]), mbx)?;
    let jobs = finalized_jobs(&prepare(repo.path())?)?;
    let mut rust_jobs = 0;
    for (id, job) in &jobs {
        if id == "plan" || id.starts_with("rust-") {
            rust_jobs += usize::from(id.starts_with("rust-"));
            assert_job_components(id, job, mbx)?;
        }
    }
    assert_eq!(rust_jobs, 2, "both fixture crates are watched (mbx={mbx})");
    Ok(())
}

fn assert_job_components(id: &str, job: &Job, mbx: bool) -> TestResult {
    let component = job
        .steps
        .iter()
        .position(|step| step.name == PREPARE_RUST_COMPONENTS_STEP);
    let needs_components = id.starts_with("rust-")
        || job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::PlanFormat));
    assert_eq!(component.is_some(), needs_components, "{id} (mbx={mbx})");
    let Some(component) = component else {
        return Ok(());
    };
    let setup = job
        .steps
        .iter()
        .position(|step| step.name == SETUP_MISE_NAME)
        .ok_or_else(|| std::io::Error::other(format!("{id} misses setup (mbx={mbx})")))?;
    assert!(setup < component, "{id}: components follow restore/setup");
    let step = &job.steps[component];
    assert!(
        step.condition.is_none(),
        "{id}: component repair is unconditional"
    );
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(format!("{id} component repair is not a shell step").into());
    };
    for token in [
        "rustup",
        "component",
        "add",
        "--toolchain",
        "clippy",
        "rustfmt",
    ] {
        assert!(run.join(" ").contains(token), "{id} payload misses {token}");
    }
    Ok(())
}

fn assert_yaml_component_order(mbx: bool) -> TestResult {
    let repo = super::make_workspace(&super::lock_for(&["a", "b"]), mbx)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let yaml = tree
        .get(WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    let emitted = yaml_steps(yaml);
    for id in ["rust-a", "rust-b"] {
        let names = emitted
            .get(id)
            .ok_or_else(|| std::io::Error::other(format!("missing {id}")))?;
        assert_crate_yaml_order(names, id)?;
    }
    if let Some(plan) = emitted.get("plan") {
        assert_plan_yaml_order(plan, mbx)?;
    }
    Ok(())
}

fn assert_crate_yaml_order(names: &[String], id: &str) -> TestResult {
    let setup = step_index(names, SETUP_MISE_NAME, id)?;
    let components = step_index(names, PREPARE_RUST_COMPONENTS_STEP, id)?;
    let clippy = step_index(names, "Clippy", id)?;
    assert!(
        setup < components && components < clippy,
        "{id} emitted order"
    );
    Ok(())
}

fn assert_plan_yaml_order(names: &[String], mbx: bool) -> TestResult {
    let has_format = names.iter().any(|name| name == "Format");
    let components = names
        .iter()
        .any(|name| name == PREPARE_RUST_COMPONENTS_STEP);
    assert_eq!(components, has_format, "plan Format components (mbx={mbx})");
    if has_format {
        assert!(
            step_index(names, SETUP_MISE_NAME, "plan")?
                < step_index(names, PREPARE_RUST_COMPONENTS_STEP, "plan")?,
            "plan Format components follow restore/setup"
        );
    }
    Ok(())
}

fn step_index(names: &[String], target: &str, job: &str) -> Result<usize, std::io::Error> {
    names
        .iter()
        .position(|name| name == target)
        .ok_or_else(|| std::io::Error::other(format!("{job} misses {target}")))
}

fn yaml_steps(yaml: &str) -> BTreeMap<String, Vec<String>> {
    let mut jobs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current = None;
    for line in yaml.lines() {
        let header = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && rest.ends_with(':') && !rest.contains(' '));
        if let Some(id) = header {
            current = Some(id.trim_end_matches(':').to_owned());
        } else if let (Some(id), Some(name)) = (&current, line.trim().strip_prefix("- name: ")) {
            jobs.entry(id.clone()).or_default().push(name.to_owned());
        }
    }
    jobs
}
