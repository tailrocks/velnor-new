// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

macro_rules! must_fail {
    ($result:expr, $case:expr $(,)?) => {
        if $result.is_ok() {
            return Err(fail($case));
        }
    };
}

#[path = "../src/crate_jobs_stage.rs"]
mod crate_jobs_stage;

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::io;

use velnor_actions_contract::cachekey::ToolchainInputs;
use velnor_actions_contract::{
    CacheMode, CachePolicy, IdentityInputs, Job, JobTimeout, ProposedTask, ResourceClass,
    ResourceDemand, Step, StepId, StepKind, StepRole,
};

use self::crate_jobs_stage::{
    BRANCHES_PER_GROUP, BranchJobs, CombinedBranch, STAGED_TOFU_JOB_CONDITION,
    TRUSTED_WRITER_EVENT_GATE, UNTRUSTED_READER_EVENT_GATE, WRITER_RECHECK_STEP_NAME,
    branch_job_count, combined_branch_for, is_mbx, is_nextest, is_opentofu, is_recoverable_probe,
    is_rust, needs, reader_branch_condition, stage_tofu_root_jobs, validate_combined_branch,
    writer_branch_condition, writer_branch_needs, writer_recheck_step,
};

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn fixture_toolchain() -> ToolchainInputs {
    ToolchainInputs {
        tools: vec!["rust".to_owned()],
        components: Vec::new(),
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo".to_owned(),
    }
}

fn fixture_task(stack: &str, driver: &str, runner: &str) -> ProposedTask {
    ProposedTask {
        task_id: "internal/build/dev".to_owned(),
        stack_id: stack.to_owned(),
        component_id: "demo".to_owned(),
        task_kind: "build".to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: "demo".to_owned(),
            unit_key: "demo".to_owned(),
            unit_path: "crates/demo/Cargo.toml".to_owned(),
            project_root: String::new(),
            target: String::new(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: driver.to_owned(),
            test_runner: runner.to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: vec![OsString::from("mbx")],
        display_name: "demo".to_owned(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}

fn shell_step(name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["echo probe".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

fn restore_step(name: &str, role: StepRole) -> Step {
    Step {
        name: name.to_owned(),
        id: Some(StepId::SourcesRestore),
        role: Some(role),
        condition: None,
        parallel: false,
        kind: StepKind::Action {
            uses: "actions/cache/restore@v4".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn execution_step(name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::TaskExecution {
            argv: vec!["mbx".to_owned(), "build".to_owned(), "--locked".to_owned()],
            env: BTreeMap::new(),
            task_id: "internal/build/dev".to_owned(),
            task_digest: "digest".to_owned(),
            toolchain_inputs: fixture_toolchain(),
            matrix_id: "matrix".to_owned(),
            matrix_key: "key".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            matrix_max_parallel: None,
        },
    }
}

fn fixture_job(
    cache_mode: Option<CacheMode>,
    condition: Option<String>,
    steps: Vec<Step>,
) -> Result<Job, Box<dyn Error>> {
    Ok(Job {
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::new(10).map_err(|_| fail("bad fixture timeout"))?,
        needs: vec!["plan".to_owned()],
        condition,
        permissions: None,
        cache_mode,
        environment: None,
        services: Vec::new(),
        steps,
    })
}

fn fixture_base_steps() -> Vec<Step> {
    vec![
        restore_step("Restore Cargo sources", StepRole::CargoSourcesRestore),
        shell_step("Prepare tools"),
        execution_step("Run build lane"),
    ]
}

fn fixture_branches() -> Result<(CombinedBranch, Job, Job), Box<dyn Error>> {
    let branch = combined_branch_for("rust-demo")?;
    let gate = "!contains(needs.plan.outputs.not_affected_jobs, ',rust-demo,')";
    let reader = fixture_job(
        Some(CacheMode::explicit_read()),
        Some(reader_branch_condition(Some(gate))?),
        fixture_base_steps(),
    )?;
    let mut writer_steps = Vec::new();
    if let Some(recheck) = writer_recheck_step(&fixture_base_steps()) {
        writer_steps.push(recheck);
    }
    writer_steps.extend(fixture_base_steps());
    let writer = Job {
        needs: writer_branch_needs(&["plan".to_owned()], &branch.reader)?,
        condition: Some(writer_branch_condition(Some(gate))?),
        cache_mode: Some(CacheMode::push_write_expression()),
        ..fixture_job(None, None, writer_steps)?
    };
    Ok((branch, reader, writer))
}

fn check_group_classification() -> Result<(), Box<dyn Error>> {
    let cargo = fixture_task("rust", "cargo", "cargo_test");
    let mbx = fixture_task("rust", "mbx", "cargo_nextest");
    let tofu = fixture_task("tofu", "cargo", "cargo_test");
    if !is_rust(&cargo)
        || is_mbx(&cargo)
        || is_nextest(&cargo)
        || is_opentofu(&cargo)
        || needs(&cargo).mbx
        || needs(&cargo).nextest
    {
        return Err(fail("cargo group misclassified"));
    }
    if !is_rust(&mbx) || !is_mbx(&mbx) || !is_nextest(&mbx) || is_opentofu(&mbx) {
        return Err(fail("mbx group misclassified"));
    }
    if is_rust(&tofu) || !is_opentofu(&tofu) || needs(&tofu).mbx || needs(&tofu).nextest {
        return Err(fail("tofu group misclassified"));
    }
    Ok(())
}

fn check_static_reader_writer_ids() -> Result<(), Box<dyn Error>> {
    if BRANCHES_PER_GROUP != 2 {
        return Err(fail("branch fan-out drifted"));
    }
    let branch = combined_branch_for("rust-demo")?;
    if branch.reader != "rust-demo-read" || branch.writer != "rust-demo-write" {
        return Err(fail("static branch ids drifted"));
    }
    if branch.reader == branch.writer || branch.group != "rust-demo" {
        return Err(fail("branch identity collision"));
    }
    if branch_job_count(3) != 6 || branch_job_count(0) != 0 {
        return Err(fail("branch accounting drifted"));
    }
    if branch_job_count(usize::MAX) != usize::MAX {
        return Err(fail("branch accounting overflows"));
    }
    Ok(())
}

fn check_branch_decided_before_compilation() -> Result<(), Box<dyn Error>> {
    let (branch, reader, writer) = fixture_branches()?;
    let jobs = BranchJobs {
        reader_job: &branch.reader,
        reader: &reader,
        writer_job: &branch.writer,
        writer: &writer,
    };
    validate_combined_branch(&branch, Some("gate"), true, &jobs)?;
    let writer_condition = writer
        .condition
        .as_deref()
        .ok_or_else(|| fail("missing writer gate"))?;
    if !writer_condition.contains(TRUSTED_WRITER_EVENT_GATE)
        || !writer_condition.contains(",rust-demo,")
    {
        return Err(fail("writer branch decided without trusted group gate"));
    }
    let reader_condition = reader
        .condition
        .as_deref()
        .ok_or_else(|| fail("missing reader gate"))?;
    if !reader_condition.contains(UNTRUSTED_READER_EVENT_GATE)
        || !reader_condition.contains(",rust-demo,")
    {
        return Err(fail("reader branch lost its static gate"));
    }
    if !writer.needs.contains(&branch.reader) {
        return Err(fail("writer branch unordered behind reader"));
    }
    if writer_recheck_step(&[]).is_some() {
        return Err(fail("recheck invented without a probe"));
    }
    Ok(())
}

fn check_recheck_after_queue_slot() -> Result<(), Box<dyn Error>> {
    let (_, _, writer) = fixture_branches()?;
    let Some(recheck) = writer_recheck_step(&fixture_base_steps()) else {
        return Err(fail("recheck probe missing"));
    };
    if recheck.name != WRITER_RECHECK_STEP_NAME
        || !is_recoverable_probe(&recheck)
        || recheck.id.is_some()
        || recheck.condition.is_some()
    {
        return Err(fail("recheck probe carries execution semantics"));
    }
    let first = writer
        .steps
        .first()
        .ok_or_else(|| fail("writer lost its steps"))?;
    if first.name != WRITER_RECHECK_STEP_NAME {
        return Err(fail("recheck not first after queue slot"));
    }
    let first_execution = writer
        .steps
        .iter()
        .position(|step| matches!(step.kind, StepKind::TaskExecution { .. }))
        .ok_or_else(|| fail("writer lost compilation"))?;
    if first_execution == 0 {
        return Err(fail("compilation precedes the recheck"));
    }
    Ok(())
}

fn tofu_execution_step(name: &str) -> Step {
    Step {
        role: Some(StepRole::TofuProviderUse),
        ..shell_step(name)
    }
}

fn check_tofu_group_without_probe() -> Result<(), Box<dyn Error>> {
    let branch = combined_branch_for("tofu-demo")?;
    let reader = fixture_job(
        Some(CacheMode::explicit_read()),
        Some(reader_branch_condition(None)?),
        vec![tofu_execution_step("Apply tofu lane")],
    )?;
    let writer = Job {
        needs: writer_branch_needs(&["plan".to_owned()], &branch.reader)?,
        condition: Some(writer_branch_condition(None)?),
        cache_mode: Some(CacheMode::push_write_expression()),
        ..fixture_job(None, None, vec![tofu_execution_step("Apply tofu lane")])?
    };
    validate_combined_branch(
        &branch,
        None,
        false,
        &BranchJobs {
            reader_job: &branch.reader,
            reader: &reader,
            writer_job: &branch.writer,
            writer: &writer,
        },
    )?;
    if writer_recheck_step(writer.steps.as_slice()).is_some() {
        return Err(fail("single-admission tofu state rechecked twice"));
    }
    Ok(())
}

fn check_recoverable_optional_cache() -> Result<(), Box<dyn Error>> {
    for role in [
        StepRole::ToolsCacheRestore,
        StepRole::CargoSourcesRestore,
        StepRole::MbxBundleRestore,
        StepRole::FamilyRestore,
    ] {
        if !is_recoverable_probe(&restore_step("probe", role)) {
            return Err(fail("optional cache failure not recoverable in-job"));
        }
    }
    if is_recoverable_probe(&shell_step("probe")) || is_recoverable_probe(&execution_step("probe"))
    {
        return Err(fail("non-cache step classified as probe"));
    }
    let tofu_probe = Step {
        role: Some(StepRole::TofuProvidersRestore),
        ..restore_step("probe", StepRole::TofuProvidersRestore)
    };
    if is_recoverable_probe(&tofu_probe) {
        return Err(fail("single-admission tofu restore duplicated"));
    }
    if writer_recheck_step(&[shell_step("only")]).is_some() {
        return Err(fail("recheck invented without cache state"));
    }
    Ok(())
}

#[path = "gc1_t32_a.rs"]
mod gc1_t32_a;
#[path = "gc1_t32_b.rs"]
mod gc1_t32_b;
pub(crate) use gc1_t32_a::*;
