// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole, WorkflowPolicy};
use velnor_actions_mise::PinnedTool;
use velnor_actions_orchestrator::{GenerationPreparation, finalized_jobs, prepare};

use super::tools_producers::is_tools_producer;
use super::tools_producers_donors::{
    SCHEDULED_SUITES_CONFIG, assert_hard_error, finalized_after, prepared_for, run_text, scrub_env,
};
use super::workflow_configuration_fixture::make_repo;

const MINIMAL_RUST_PREFIX: &str = "rust[profile=minimal]@";

struct DonorRustRewrite<'a> {
    label: &'a str,
    spec: &'a str,
    drop_verifier: bool,
    promote_install: bool,
}

fn rewrite_standalone_rust(
    prepared: &mut GenerationPreparation,
    row: &DonorRustRewrite<'_>,
) -> Result<(), Box<dyn Error>> {
    let mut rewritten = 0;
    for job in prepared
        .workflow
        .context
        .standalone_tools_donors
        .values_mut()
    {
        let mut job_hit = false;
        for step in &mut job.steps {
            let mut step_hit = false;
            if let StepKind::Shell { run, .. } = &mut step.kind {
                for token in run.iter_mut() {
                    if let Some(version) = token.strip_prefix(MINIMAL_RUST_PREFIX) {
                        *token = row.spec.replace("{v}", version);
                        step_hit = true;
                        rewritten += 1;
                    }
                }
            }
            if step_hit && row.promote_install {
                step.role = Some(StepRole::PreparePinnedTools);
            }
            job_hit |= step_hit;
        }
        if job_hit && row.drop_verifier {
            job.steps
                .retain(|step| step.role != Some(StepRole::VerifyRustManifest));
        }
    }
    if rewritten == 1 {
        Ok(())
    } else {
        Err(format!("{}: rewrote {rewritten} tokens", row.label).into())
    }
}

#[test]
fn a_standalone_donor_with_a_non_canonical_rust_install_is_a_render_error()
-> Result<(), Box<dyn Error>> {
    let repo = make_repo(SCHEDULED_SUITES_CONFIG)?;
    let control = prepare(repo.path())?;
    let producers = finalized_jobs(&control)?
        .values()
        .filter(|job| is_tools_producer(job))
        .filter(|job| {
            job.steps
                .iter()
                .filter(|step| step.role == Some(StepRole::PreparePinnedTools))
                .filter_map(run_text)
                .any(|run| run.contains(MINIMAL_RUST_PREFIX))
        })
        .count();
    assert!(
        producers > 0,
        "no producer installs the minimal Rust profile"
    );
    let rows = [
        DonorRustRewrite {
            label: "complete profile",
            spec: "rust[profile=complete]@{v}",
            drop_verifier: false,
            promote_install: false,
        },
        DonorRustRewrite {
            label: "extra components option",
            spec: "rust[profile=minimal,components=rust-docs]@{v}",
            drop_verifier: false,
            promote_install: false,
        },
        DonorRustRewrite {
            label: "complete profile without verifier",
            spec: "rust[profile=complete]@{v}",
            drop_verifier: true,
            promote_install: false,
        },
        DonorRustRewrite {
            label: "complete profile in a prepare step",
            spec: "rust[profile=complete]@{v}",
            drop_verifier: false,
            promote_install: true,
        },
        DonorRustRewrite {
            label: "bare rust in a prepare step",
            spec: "rust@{v}",
            drop_verifier: false,
            promote_install: true,
        },
        DonorRustRewrite {
            label: "bare rust",
            spec: "rust@{v}",
            drop_verifier: false,
            promote_install: false,
        },
    ];
    for row in rows {
        let mut prepared = prepare(repo.path())?;
        rewrite_standalone_rust(&mut prepared, &row)?;
        assert_hard_error(
            finalized_jobs(&prepared).map(|_| ()),
            "tools_producer_donor_rust_install_not_minimal",
        )
        .map_err(|error| format!("{}: {error}", row.label))?;
    }
    Ok(())
}

#[test]
fn a_plain_rust_install_next_to_the_minimal_install_is_a_render_error() -> Result<(), Box<dyn Error>>
{
    let (_repo, mut prepared) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    let version = prepared
        .emission_catalog()
        .version(PinnedTool::Rust)
        .to_owned();
    let mutated = std::cell::Cell::new(0);
    let result = finalized_after(&mut prepared, |job| {
        let crate_job = job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::TaskExecution { .. }));
        let Some(at) = job
            .steps
            .iter()
            .position(|step| step.role == Some(StepRole::PreparePinnedTools))
            .filter(|_| crate_job)
            .filter(|at| {
                run_text(&job.steps[*at]).is_some_and(|run| run.contains(MINIMAL_RUST_PREFIX))
            })
        else {
            return;
        };
        mutated.set(mutated.get() + 1);
        let mut extra = job.steps[at].clone();
        extra.role = None;
        extra.name = "Install plain rust".to_owned();
        scrub_env(&mut extra);
        if let StepKind::Shell { run, .. } = &mut extra.kind {
            *run = [
                "mise".to_owned(),
                "--no-config".to_owned(),
                "--no-env".to_owned(),
                "--no-hooks".to_owned(),
                "install".to_owned(),
                format!("rust@{version}"),
            ]
            .to_vec();
        }
        job.steps.insert(at + 1, extra);
    });
    assert!(
        mutated.get() > 0,
        "no crate job installed the minimal Rust profile"
    );
    assert_hard_error(result, "conflicting_tools_cache_spec:rust")
}
