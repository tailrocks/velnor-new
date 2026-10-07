use super::*;
use velnor_actions_orchestrator_core::clippy_groups::ClippyMemoryPlan;
use velnor_actions_rust::{TaskGroup, TaskKind};
use velnor_actions_rust_core::{CompileDriver, NextestProfile, TestRunner};
/// Discovery shell carrying only task proposals.
pub(super) fn discovery(groups: Vec<ProposedTask>) -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: groups,
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}
/// Runnable fixture proposal for one package/kind pair.
pub(super) fn group(package: &str, kind: TaskKind, gated_by: &[&str]) -> ProposedTask {
    let key = if package == "demo" { "root" } else { package };
    let group = TaskGroup {
        task_id: format!("stack/rust/{key}/{}/default", kind.as_str()),
        package_id: format!("{package} 0.1.0"),
        package_name: package.to_owned(),
        manifest_key: key.to_owned(),
        kind,
        configuration: "default".to_owned(),
        features: Vec::new(),
        target: "host".to_owned(),
        gated_by: gated_by.iter().map(ToString::to_string).collect(),
        depends_on: Vec::new(),
        target_flags: Vec::new(),
        no_test_targets: false,
        package_arg: None,
        compile_driver: CompileDriver::Cargo,
        test_runner: TestRunner::CargoTest,
        nextest_profile: NextestProfile::Default,
        declared_inputs: Vec::new(),
        undeclared_reads: false,
        uses_network: false,
        uses_clock: false,
        uses_random: false,
        run_ignored: None,
    };
    let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}
/// Step names of one built job.
pub(super) fn names(job: &Job) -> Vec<&str> {
    job.steps.iter().map(|step| step.name.as_str()).collect()
}

mod crate_jobs_display_tests;
mod crate_jobs_tests;
mod crate_jobs_tofu_cache_tests;
mod crate_jobs_tofu_tests;
mod crate_jobs_upload_tests;
mod mbx_tests;
