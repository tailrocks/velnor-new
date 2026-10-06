//! T24 gate 4 provider-cache election and per-root output binding.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Job, StepKind};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use crate::impl_common::TestResult;
use crate::impl_tofu_t24_gates::tofu_perf_fixtures_t24::tofu_repo;

type Shell<'a> = (&'a Vec<String>, &'a BTreeMap<String, String>);

fn names(job: &Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

fn shell_of<'a>(job: &'a Job, name: &str) -> Result<Shell<'a>, Box<dyn std::error::Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .ok_or(format!("missing step {name}"))?;
    match &step.kind {
        StepKind::Shell { run, env } => Ok((run, env)),
        _ => Err(format!("{name} must be a shell step").into()),
    }
}

fn action_with<'a>(
    job: &'a Job,
    name: &str,
) -> Result<&'a BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .ok_or(format!("missing step {name}"))?;
    match &step.kind {
        StepKind::Action { with, .. } => Ok(with),
        _ => Err(format!("{name} must be an action step").into()),
    }
}

/// At most one init per root and one elected save over that restore's outputs.
#[test]
fn gate4_init_once_fmt_once_no_duplicate_uploads() -> TestResult {
    use velnor_actions_contract_workflow::workflow::ir::CACHE_SAVE_CONDITION;
    use velnor_actions_contract_workflow::workflow::step_identity::{
        TOFU_PROVIDERS_KEY_OUTPUT_EXPR, TOFU_PROVIDERS_PATH_OUTPUT_EXPR,
    };

    let dir = tofu_repo(3)?;
    let jobs = finalized_jobs(&prepare(dir.path())?)?;
    let catalog = ToolCatalog::pinned();
    let rust = catalog.tool_spec(PinnedTool::Rust);
    let opentofu = catalog.tool_spec(PinnedTool::Opentofu);
    let tofu: Vec<_> = jobs
        .iter()
        .filter(|(id, _)| id.starts_with("tofu-"))
        .collect();
    assert_eq!(tofu.len(), 3, "one job per root");
    let mut fmt_tasks = Vec::new();
    let mut saved_keys = Vec::new();
    for (id, job) in tofu {
        let steps = names(job);
        assert_eq!(
            steps
                .iter()
                .filter(|name| **name == "Init for validate")
                .count(),
            1,
            "{id} inits once"
        );
        let (_, fmt_env) = shell_of(job, "Format")?;
        let task = fmt_env.get("VELNOR_TASK_ID").ok_or("fmt task id")?.clone();
        assert!(task.starts_with("stack/tofu/") && task.ends_with("/fmt/default"));
        fmt_tasks.push(task);
        let (run, _) = shell_of(job, "Prepare pinned tools")?;
        assert!(run.contains(&opentofu));
        assert!(!run.contains(&rust));

        let saves: Vec<_> = job
            .steps
            .iter()
            .filter(|step| step.name == "Save Tofu providers")
            .collect();
        assert!(saves.len() <= 1, "{id} saves at most once");
        if let Some(save) = saves.first() {
            let restore = action_with(job, "Restore Tofu providers")?;
            let StepKind::Action { with, .. } = &save.kind else {
                return Err("provider saver must be an action".into());
            };
            assert_eq!(
                save.condition.as_deref(),
                Some(CACHE_SAVE_CONDITION),
                "{id} saves under the push-only gate"
            );
            assert_eq!(
                with.get("key").map(String::as_str),
                Some(TOFU_PROVIDERS_KEY_OUTPUT_EXPR)
            );
            assert_eq!(
                with.get("path").map(String::as_str),
                Some(TOFU_PROVIDERS_PATH_OUTPUT_EXPR)
            );
            saved_keys.push(restore.get("cache-key").ok_or("restore key")?.clone());
        }
    }
    fmt_tasks.sort();
    fmt_tasks.dedup();
    assert_eq!(fmt_tasks.len(), 3, "each scope formats once");
    assert!(!saved_keys.is_empty(), "provider uploads exist");
    let save_count = saved_keys.len();
    saved_keys.sort();
    saved_keys.dedup();
    assert_eq!(
        saved_keys.len(),
        save_count,
        "no duplicate provider archive upload"
    );
    Ok(())
}
