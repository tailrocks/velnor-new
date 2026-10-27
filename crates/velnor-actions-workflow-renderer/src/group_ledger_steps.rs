// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

/// Matrix-side producer: runs the workspace CLI through `cargo run`
/// (pin-independent by design) with `VELNOR_INTERNAL_OP` selecting emit.
pub(crate) const EMIT_GROUP_LEDGER_OPERATION: &str = "emit-group-ledger-v1";

/// Fan-in assembly: runs the workspace CLI through `cargo run` with
/// `VELNOR_INTERNAL_OP` selecting assembly.
pub(crate) const ASSEMBLE_GROUP_LEDGERS_OPERATION: &str = "assemble-group-ledgers-v1";

pub(crate) const EMIT_GROUP_LEDGER_STEP_NAME: &str = "Emit group ledger";

pub(crate) const ASSEMBLE_GROUP_LEDGERS_STEP_NAME: &str = "Assemble group ledgers";

/// The matrix job key available to the emit step environment.
pub(crate) const MATRIX_KEY_ENV: &str = "VELNOR_MATRIX_KEY";

/// The job status available to the emit step environment.
pub(crate) const JOB_STATUS_ENV: &str = "VELNOR_JOB_STATUS";

fn cargo_run_argv() -> Vec<String> {
    vec![
        "cargo".to_owned(),
        "run".to_owned(),
        "--locked".to_owned(),
        "-p".to_owned(),
        "velnor-actions-cli".to_owned(),
        "--bin".to_owned(),
        "velnor-actions".to_owned(),
    ]
}

/// Matrix job step: emit this job's `group-ledger.json` after the task
/// reports are staged.
///
/// The step always runs (`if: always()`) so a failed task still records
/// its evidence loudly instead of silently skipping the ledger. The ledger
/// file rides the existing matrix report upload (whole `matrix_key`
/// directory), so no new upload step is emitted.
#[must_use]
pub(crate) fn emit_group_ledger_step() -> Step {
    Step {
        parallel: false,
        name: EMIT_GROUP_LEDGER_STEP_NAME.to_owned(),
        id: None,
        role: None,
        condition: Some(crate::render::UPLOAD_STEP_CONDITION.to_owned()),
        kind: StepKind::Shell {
            run: cargo_run_argv(),
            env: crate::toolchain_env::with_credential_scrub(&BTreeMap::from([
                (
                    crate::steps::INTERNAL_OP_ENV.to_owned(),
                    EMIT_GROUP_LEDGER_OPERATION.to_owned(),
                ),
                (
                    MATRIX_KEY_ENV.to_owned(),
                    "${{ matrix.matrix_key }}".to_owned(),
                ),
                (JOB_STATUS_ENV.to_owned(), "${{ job.status }}".to_owned()),
            ])),
        },
    }
}

/// Append the emit step to matrix reader/writer branch jobs, immediately
/// before the matrix report upload so the ledger file rides the artifact
/// (whole `matrix_key` directory). Runs at most once per job; non-branch
/// jobs are untouched.
///
/// Only jobs carrying the matrix report upload (artifact name
/// `velnor-matrix-…`, i.e. jobs with a `strategy.matrix.matrix_key`)
/// receive the step. The emit environment reads
/// `${{ matrix.matrix_key }}`: in single-unit jobs, which only carry the
/// per-job `velnor-crate-…` upload, that reference is undefined
/// (jactionlint `undefined-property`, and empty at runtime where the emit
/// requires a non-empty key). Those jobs have no matrix key to bind, so
/// no ledger is emitted for them.
pub(crate) fn append_emit_ledger_steps(jobs: &mut BTreeMap<String, velnor_actions_contract::Job>) {
    use velnor_actions_contract::workflow::crate_job::{GROUP_READER_SUFFIX, GROUP_WRITER_SUFFIX};

    for (id, job) in jobs.iter_mut() {
        if !(id.ends_with(GROUP_READER_SUFFIX) || id.ends_with(GROUP_WRITER_SUFFIX)) {
            continue;
        }
        if job
            .steps
            .iter()
            .any(|step| step.name == EMIT_GROUP_LEDGER_STEP_NAME)
        {
            continue;
        }
        let Some(at) = job.steps.iter().position(is_matrix_report_upload) else {
            continue;
        };
        job.steps.insert(at, emit_group_ledger_step());
    }
}

/// Whether the step is the matrix-variant crate report upload, whose
/// artifact name carries the `matrix.matrix_key` the emit step binds.
/// Single-unit jobs carry the per-job `velnor-crate-…` upload instead.
fn is_matrix_report_upload(step: &Step) -> bool {
    step.name == crate::steps::CRATE_REPORT_UPLOAD_NAME
        && matches!(&step.kind, StepKind::Action { with, .. }
            if with.get("name").is_some_and(|name| name.starts_with("velnor-matrix-")))
}

/// Required-job step: assemble the downloaded matrix `group-ledger.json`
/// files into the run `group-ledgers.json` consumed by merge.
#[must_use]
pub(crate) fn assemble_group_ledgers_step() -> Step {
    Step {
        parallel: false,
        name: ASSEMBLE_GROUP_LEDGERS_STEP_NAME.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Internal {
            operation: ASSEMBLE_GROUP_LEDGERS_OPERATION.to_owned(),
            env: BTreeMap::new(),
        },
    }
}

// gc1-t56 pins the group-ledger step constructors: exact names, the
// always() condition on emit only, the workspace `cargo run` command for emit,
// the authenticated helper operation for assembly, the op env wiring for emit,
// and no upload step.
#[cfg(test)]
mod gc1_t56 {
    use velnor_actions_contract::StepKind;

    use super::{
        ASSEMBLE_GROUP_LEDGERS_OPERATION, ASSEMBLE_GROUP_LEDGERS_STEP_NAME,
        EMIT_GROUP_LEDGER_OPERATION, EMIT_GROUP_LEDGER_STEP_NAME, MATRIX_KEY_ENV,
        assemble_group_ledgers_step, emit_group_ledger_step,
    };

    #[test]
    fn emit_uses_always_and_cargo_run() {
        let step = emit_group_ledger_step();
        assert_eq!(step.name, EMIT_GROUP_LEDGER_STEP_NAME);
        assert_eq!(step.condition.as_deref(), Some("always()"));
        assert!(!step.parallel);
        assert!(step.id.is_none() && step.role.is_none());
        let StepKind::Shell { run, env } = &step.kind else {
            assert!(matches!(step.kind, StepKind::Shell { .. }));
            return;
        };
        assert_eq!(
            run,
            &[
                "cargo",
                "run",
                "--locked",
                "-p",
                "velnor-actions-cli",
                "--bin",
                "velnor-actions"
            ]
        );
        assert!(crate::commands::validate_command_argv(run).is_ok());
        assert!(crate::commands::validate_env(env).is_ok());
        assert_eq!(
            env.get(crate::steps::INTERNAL_OP_ENV).map(String::as_str),
            Some(EMIT_GROUP_LEDGER_OPERATION)
        );
        assert_eq!(
            env.get(MATRIX_KEY_ENV).map(String::as_str),
            Some("${{ matrix.matrix_key }}")
        );
    }

    #[test]
    fn assemble_needs_no_condition_or_upload() {
        let step = assemble_group_ledgers_step();
        assert_eq!(step.name, ASSEMBLE_GROUP_LEDGERS_STEP_NAME);
        assert!(step.condition.is_none());
        assert!(!step.parallel);
        let StepKind::Internal { operation, env } = &step.kind else {
            assert!(matches!(step.kind, StepKind::Internal { .. }));
            return;
        };
        assert_eq!(operation, ASSEMBLE_GROUP_LEDGERS_OPERATION);
        assert!(env.is_empty());
    }

    #[test]
    fn op_strings_match_cli_protocol() {
        assert_eq!(EMIT_GROUP_LEDGER_OPERATION, "emit-group-ledger-v1");
        assert_eq!(
            ASSEMBLE_GROUP_LEDGERS_OPERATION,
            "assemble-group-ledgers-v1"
        );
    }

    #[test]
    fn ops_are_allow_listed_as_self_selecting() {
        let mut allow_listed = true;
        for op in [
            EMIT_GROUP_LEDGER_OPERATION,
            ASSEMBLE_GROUP_LEDGERS_OPERATION,
        ] {
            match crate::steps_internal::split_internal_operation(op) {
                Ok((resolved, target)) => {
                    allow_listed = allow_listed && resolved == op && target == op;
                }
                Err(_) => {
                    allow_listed = false;
                }
            }
        }
        assert!(allow_listed, "group ledger ops must be allow-listed");
        assert!(crate::steps_internal::split_internal_operation("bogus-ledger-op-v9").is_err());
    }
}

#[cfg(test)]
mod gc1_t57 {
    use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

    use super::{
        EMIT_GROUP_LEDGER_STEP_NAME, JOB_STATUS_ENV, append_emit_ledger_steps,
        emit_group_ledger_step,
    };

    fn branch_job(id: &str, with_upload: bool) -> Job {
        let mut steps = vec![Step {
            name: "Wait for parallel checks".to_owned(),
            id: None,
            role: None,
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: std::collections::BTreeMap::new(),
            },
        }];
        if with_upload {
            steps.push(Step {
                name: crate::steps::CRATE_REPORT_UPLOAD_NAME.to_owned(),
                id: None,
                role: None,
                condition: Some("always()".to_owned()),
                parallel: false,
                kind: StepKind::Action {
                    uses: crate::steps::UPLOAD_ARTIFACT_USES.to_owned(),
                    with: std::collections::BTreeMap::from([(
                        "name".to_owned(),
                        "velnor-matrix-rtest-group".to_owned(),
                    )]),
                    env: std::collections::BTreeMap::new(),
                },
            });
        }
        Job {
            display_name: id.to_owned(),
            runs_on: "ubuntu-24.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::REQUIRED,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            cache_mode: None,
            environment: None,
            services: Vec::new(),
            outputs: std::collections::BTreeMap::new(),
            steps,
        }
    }

    fn step_names(job: &Job) -> Vec<&str> {
        job.steps.iter().map(|step| step.name.as_str()).collect()
    }

    #[test]
    fn emit_binds_job_status_env() {
        let step = emit_group_ledger_step();
        let StepKind::Shell { env, .. } = &step.kind else {
            panic!("emit step kind drifted");
        };
        assert_eq!(
            env.get(JOB_STATUS_ENV).map(String::as_str),
            Some("${{ job.status }}")
        );
    }

    #[test]
    fn emit_lands_before_crate_upload() {
        let mut jobs = std::collections::BTreeMap::from([
            ("group-read".to_owned(), branch_job("group-read", true)),
            ("group-write".to_owned(), branch_job("group-write", true)),
            ("plan".to_owned(), branch_job("plan", true)),
        ]);
        append_emit_ledger_steps(&mut jobs);
        for id in ["group-read", "group-write"] {
            assert_eq!(
                step_names(&jobs[id]),
                vec![
                    "Wait for parallel checks",
                    EMIT_GROUP_LEDGER_STEP_NAME,
                    crate::steps::CRATE_REPORT_UPLOAD_NAME,
                ],
                "{id} must emit before the upload"
            );
        }
        assert_eq!(jobs["plan"].steps.len(), 2, "non-branch jobs untouched");
        append_emit_ledger_steps(&mut jobs);
        for id in ["group-read", "group-write"] {
            assert_eq!(
                jobs[id]
                    .steps
                    .iter()
                    .filter(|step| step.name == EMIT_GROUP_LEDGER_STEP_NAME)
                    .count(),
                1,
                "{id} emits exactly once"
            );
        }
    }

    #[test]
    fn emit_skipped_without_matrix_upload() {
        let mut jobs = std::collections::BTreeMap::from([(
            "group-read".to_owned(),
            branch_job("group-read", false),
        )]);
        append_emit_ledger_steps(&mut jobs);
        assert_eq!(
            step_names(&jobs["group-read"]),
            vec!["Wait for parallel checks"],
            "single-unit jobs carry no matrix key, so no ledger is emitted"
        );
    }
}
