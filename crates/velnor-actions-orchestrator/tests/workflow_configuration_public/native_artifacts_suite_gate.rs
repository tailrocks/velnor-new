// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_actionlint::actions::{MISE_ACTION_SHA, MR_BOXINGTON_ACTION_SHA};
use velnor_actions_contract::{
    Job, PullRequestCachePolicy, RustSuiteBuildMode, RustSuiteConfig, StepKind, StepRole,
};
use velnor_actions_mise::catalog::NEXTEST_VERSION;
use velnor_actions_mise::{
    MISE_BINARY_SHA256_LINUX_X64, MISE_VERSION, MR_BOXINGTON_VERSION, RUST_VERSION,
};
use velnor_actions_workflow_renderer::{
    CompileDriver,
    rust_suites::{RustSuitesRenderSpec, build_ci_suite_jobs},
    setup::MiseSetup,
    steps::{CHECKOUT_USES, check_mbx_gating},
    toolchain_env::credential_unset_prelude,
};

const SUITE_ID: &str = "suite-proof-suite";

#[test]
fn public_mbx_gate_accepts_generated_run_and_archive_commands() -> Result<(), Box<dyn Error>> {
    for (partitions, build) in [
        (1, RustSuiteBuildMode::Inline),
        (2, RustSuiteBuildMode::Inline),
        (2, RustSuiteBuildMode::Archive),
    ] {
        let built = build_ci_suite_jobs(&renderer_spec(CompileDriver::Mbx, partitions, build))?;
        check_mbx_gating(&built.jobs, &built.drivers)?;
    }
    Ok(())
}

#[test]
fn public_mbx_gate_accepts_exact_resolver_rust_token() -> Result<(), Box<dyn Error>> {
    let built = build_ci_suite_jobs(&renderer_spec(
        CompileDriver::Mbx,
        1,
        RustSuiteBuildMode::Inline,
    ))?;
    let mut jobs = built.jobs.clone();
    let job = jobs
        .get_mut(SUITE_ID)
        .ok_or("single suite job is missing")?;
    rewrite_run_script(job, |script| {
        script.replacen("rust@1.99.1", "rust@${VELNOR_TOOL_RUST}", 1)
    })?;
    assert!(
        shell_command(&jobs[SUITE_ID], "Run suite (proof-suite)")?
            .contains("rust@${VELNOR_TOOL_RUST}")
    );
    check_mbx_gating(&jobs, &built.drivers)?;
    Ok(())
}

#[test]
fn public_mbx_gating_rejects_tampered_suite_driver_selection() -> Result<(), Box<dyn Error>> {
    let mbx = build_ci_suite_jobs(&renderer_spec(
        CompileDriver::Mbx,
        1,
        RustSuiteBuildMode::Inline,
    ))?;
    let mbx_job = suite_job(&mbx.jobs, SUITE_ID)?;
    assert_gate_rejects(
        &mbx_job,
        &BTreeMap::from([(SUITE_ID.to_owned(), CompileDriver::Cargo)]),
        "mbx_action_without_selection:suite-proof-suite",
    )?;
    assert_gate_rejects(
        &mbx_job,
        &BTreeMap::new(),
        "mbx_preflight_without_selection:suite-proof-suite",
    )?;

    let cargo = build_ci_suite_jobs(&renderer_spec(
        CompileDriver::Cargo,
        1,
        RustSuiteBuildMode::Inline,
    ))?;
    let cargo_job = suite_job(&cargo.jobs, SUITE_ID)?;
    assert_gate_rejects(
        &cargo_job,
        &BTreeMap::from([(SUITE_ID.to_owned(), CompileDriver::Mbx)]),
        "mbx_missing_for_selection:suite-proof-suite",
    )
}

#[test]
fn public_mbx_gate_rejects_misleading_suite_commands_and_bad_order() -> Result<(), Box<dyn Error>> {
    let built = build_ci_suite_jobs(&renderer_spec(
        CompileDriver::Mbx,
        1,
        RustSuiteBuildMode::Inline,
    ))?;
    for (label, replacement) in [
        (
            "Cargo replacement",
            (" -- mbx nextest run", " -- cargo nextest run"),
        ),
        (
            "quoted MBX decoy",
            (" -- mbx nextest run", " -- \"mbx\" nextest run"),
        ),
        (
            "malformed Mise",
            ("--no-config --no-env", "--no-config --bad-env"),
        ),
        (
            "arbitrary Rust token",
            ("rust@1.99.1", "rust@${VELNOR_TOOL_MBX}"),
        ),
    ] {
        let mut job = built.jobs[SUITE_ID].clone();
        retain_driver_gate_steps(&mut job);
        rewrite_run_script(&mut job, |script| {
            script.replacen(replacement.0, replacement.1, 1)
        })?;
        assert_gate_rejects_with_context(
            &suite_job_from_job(SUITE_ID, job),
            &BTreeMap::from([(SUITE_ID.to_owned(), CompileDriver::Mbx)]),
            "mbx_version_check_order:suite-proof-suite",
            label,
        )?;
    }

    let mut preceded = built.jobs[SUITE_ID].clone();
    retain_driver_gate_steps(&mut preceded);
    rewrite_run_script(&mut preceded, |script| {
        script.replacen("mise --no-config", "printf decoy; mise --no-config", 1)
    })?;
    assert_gate_rejects(
        &suite_job_from_job(SUITE_ID, preceded),
        &BTreeMap::from([(SUITE_ID.to_owned(), CompileDriver::Mbx)]),
        "mbx_version_check_order:suite-proof-suite",
    )?;

    let mut reordered = built.jobs[SUITE_ID].clone();
    retain_driver_gate_steps(&mut reordered);
    move_run_before_version_check(&mut reordered)?;
    assert_gate_rejects(
        &suite_job_from_job(SUITE_ID, reordered),
        &BTreeMap::from([(SUITE_ID.to_owned(), CompileDriver::Mbx)]),
        "mbx_version_check_order:suite-proof-suite",
    )
}

fn renderer_spec(
    compile_driver: CompileDriver,
    partitions: u16,
    build: RustSuiteBuildMode,
) -> RustSuitesRenderSpec {
    let mut suite = RustSuiteConfig::new("proof-suite", "default");
    suite.partitions = partitions;
    suite.build = build;
    suite.required = true;
    let verifier_argv = vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!("{} exit 0", credential_unset_prelude()),
    ];
    RustSuitesRenderSpec {
        suites: vec![suite],
        workflow_dispatch: None,
        default_runs_on: "ubuntu-24.04".to_owned(),
        checkout_uses: CHECKOUT_USES.to_owned(),
        generator_version: env!("CARGO_PKG_VERSION").to_owned(),
        mise: MiseSetup {
            uses: format!("jdx/mise-action@{MISE_ACTION_SHA}"),
            version: MISE_VERSION.to_owned(),
            sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
        },
        rust_version: RUST_VERSION.to_owned(),
        cargo_nextest_version: NEXTEST_VERSION.to_owned(),
        compile_driver,
        mbx_action: (compile_driver == CompileDriver::Mbx).then(|| {
            (
                format!("jdx/mr-boxington-action@{MR_BOXINGTON_ACTION_SHA}"),
                MR_BOXINGTON_VERSION.to_owned(),
            )
        }),
        pull_request_cache_policy: PullRequestCachePolicy::default(),
        verify_restored_rust_argv: verifier_argv.clone(),
        verify_rust_argv: verifier_argv,
        export_rustup_toolchain: false,
    }
}

fn assert_gate_rejects(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    assert_gate_rejects_with_context(jobs, drivers, expected, expected)
}

fn assert_gate_rejects_with_context(
    jobs: &BTreeMap<String, Job>,
    drivers: &BTreeMap<String, CompileDriver>,
    expected: &str,
    context: &str,
) -> Result<(), Box<dyn Error>> {
    let error = match check_mbx_gating(jobs, drivers) {
        Ok(()) => {
            return Err(format!("MBX gate accepted invalid input: {context} ({expected})").into());
        }
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains(expected),
        "expected {expected}; found {error}"
    );
    Ok(())
}

fn suite_job(
    jobs: &BTreeMap<String, Job>,
    id: &str,
) -> Result<BTreeMap<String, Job>, Box<dyn Error>> {
    Ok(suite_job_from_job(
        id,
        jobs.get(id).ok_or("suite job is missing")?.clone(),
    ))
}

fn suite_job_from_job(id: &str, job: Job) -> BTreeMap<String, Job> {
    BTreeMap::from([(id.to_owned(), job)])
}

fn shell_command(job: &Job, name: &str) -> Result<String, Box<dyn Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == name)
        .ok_or_else(|| format!("shell step {name} is missing"))?;
    let StepKind::Shell { run, .. } = &step.kind else {
        return Err(format!("step {name} is not a shell command").into());
    };
    Ok(run.join(" "))
}

fn rewrite_run_script(
    job: &mut Job,
    rewrite: impl FnOnce(&str) -> String,
) -> Result<(), Box<dyn Error>> {
    let step = job
        .steps
        .iter_mut()
        .find(|step| step.name == "Run suite (proof-suite)")
        .ok_or("suite run step is missing")?;
    let StepKind::Shell { run, .. } = &mut step.kind else {
        return Err("suite run step is not a shell command".into());
    };
    let script = run.get_mut(2).ok_or("suite run script is missing")?;
    *script = rewrite(script);
    Ok(())
}

fn retain_driver_gate_steps(job: &mut Job) {
    job.steps.retain(|step| {
        matches!(
            step.role,
            Some(StepRole::MbxPreflight | StepRole::MbxCache | StepRole::MbxVersionCheck)
        ) || step.name == "Run suite (proof-suite)"
    });
}

fn move_run_before_version_check(job: &mut Job) -> Result<(), Box<dyn Error>> {
    let run_at = job
        .steps
        .iter()
        .position(|step| step.name == "Run suite (proof-suite)")
        .ok_or("suite run step is missing")?;
    let run = job.steps.remove(run_at);
    let check_at = job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::MbxVersionCheck))
        .ok_or("MBX version check is missing")?;
    job.steps.insert(check_at, run);
    Ok(())
}
