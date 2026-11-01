// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;

use velnor_actions_contract::workflow::step_identity::{TOOLS_CACHE_SCRUB_USES, ToolsRestoreForm};
use velnor_actions_contract::workflow::{RUST_VERIFIER_USES, TOOLS_PRODUCER_INSTALL_CONDITION};
use velnor_actions_contract::{Job, Step, StepKind, StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{
    GenerationPreparation, OrchestratorError, finalized_jobs, prepare, render_staged_tree,
};

use super::maintenance_features::owner_feature_repo;
use super::tools_producers::{POLICIES, identity_digest, is_tools_producer, positions};
use super::workflow_configuration_fixture;

pub(super) const SCHEDULED_SUITES_CONFIG: &str = r#"schema = 1

[workflow]
name = "CI"
policy = "consumer-v1"
default_branch = "testmain"
generator_validation = "bootstrap"

[stacks.rust]
compile_driver = "cargo"

[[stacks.rust.suites]]
id = "nightly"
profile = "default"
triggers = ["schedule"]
schedule = ["0 4 * * *"]
package = "demo"
manifest_path = "Cargo.toml"
"#;

fn is_install_role(role: Option<StepRole>) -> bool {
    matches!(
        role,
        Some(
            StepRole::PreparePinnedTools
                | StepRole::PrepareRustComponents
                | StepRole::VerifyRustManifest
                | StepRole::VerifyJactionlint
        )
    )
}

fn is_restored_verifier(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, .. } if uses == RUST_VERIFIER_USES[0])
}

fn donor_installs(job: &Job) -> Vec<&Step> {
    job.steps
        .iter()
        .filter(|step| is_install_role(step.role) && !is_restored_verifier(step))
        .collect()
}

fn run_text(step: &Step) -> Option<String> {
    match &step.kind {
        StepKind::Shell { run, .. } => Some(run.join(" ")),
        _ => None,
    }
}

fn prepared_for(
    policy: WorkflowPolicy,
) -> Result<(tempfile::TempDir, GenerationPreparation), Box<dyn Error>> {
    let repo = owner_feature_repo(policy, false, "default", "default")?;
    let prepared = prepare(repo.path())?;
    Ok((repo, prepared))
}

fn finalized_after(
    prepared: &mut GenerationPreparation,
    edit: impl Fn(&mut Job),
) -> Result<(), OrchestratorError> {
    prepared.workflow.ir.jobs.values_mut().for_each(edit);
    finalized_jobs(prepared).map(|_| ())
}

fn scrub_env(step: &mut Step) {
    if let StepKind::Shell { env, .. } = &mut step.kind {
        env.extend(velnor_actions_workflow_renderer::toolchain_env::credential_scrub());
    }
}

fn assert_hard_error(
    result: Result<(), OrchestratorError>,
    code: &str,
) -> Result<(), Box<dyn Error>> {
    let message = result
        .err()
        .ok_or_else(|| format!("a donor that cannot be reproduced rendered: wanted {code}"))?
        .to_string();
    assert!(
        message.contains(code),
        "wrong error, wanted {code}: {message}"
    );
    Ok(())
}

#[test]
fn donor_digest_implying_rust_components_without_prepare_is_a_render_error()
-> Result<(), Box<dyn Error>> {
    let (_repo, mut prepared) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
    finalized_jobs(&prepared)?;
    let result = finalized_after(&mut prepared, |job| {
        if job
            .steps
            .iter()
            .any(|step| step.role == Some(StepRole::PrepareRustComponents))
        {
            for step in &mut job.steps {
                if step.role == Some(StepRole::PreparePinnedTools) {
                    step.role = None;
                    scrub_env(step);
                }
            }
        }
    });
    assert_hard_error(result, "tools_producer_donor_components_without_prepare")
}

#[test]
fn donor_with_an_install_outside_its_prepare_step_is_a_render_error() -> Result<(), Box<dyn Error>>
{
    let (_repo, mut prepared) = prepared_for(WorkflowPolicy::VelnorRepositoryV1)?;
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
        else {
            return;
        };
        let mut extra = job.steps[at].clone();
        extra.role = None;
        extra.name = "Install extra tool".to_owned();
        scrub_env(&mut extra);
        if let StepKind::Shell { run, .. } = &mut extra.kind {
            *run = [
                "mise",
                "--no-config",
                "--no-env",
                "--no-hooks",
                "install",
                "extra-tool@1.2.3",
            ]
            .map(str::to_owned)
            .to_vec();
        }
        job.steps.insert(at + 1, extra);
    });
    assert_hard_error(result, "tools_producer_donor_inline_install")
}

#[test]
fn producer_installs_are_the_donor_installs() -> Result<(), Box<dyn Error>> {
    let mut inline = 0;
    let mut cloned = 0;
    for policy in POLICIES {
        let (_repo, prepared) = prepared_for(policy)?;
        let jobs = finalized_jobs(&prepared)?;
        for (id, producer) in jobs.iter().filter(|(_, job)| is_tools_producer(job)) {
            let digest = identity_digest(producer).ok_or("producer without identity")?;
            let (donor_id, donor) = jobs
                .iter()
                .find(|(_, job)| !is_tools_producer(job) && identity_digest(job) == Some(digest))
                .ok_or_else(|| format!("{id} has no donor"))?;
            let installs: Vec<Step> = donor_installs(producer)
                .into_iter()
                .map(|step| Step {
                    condition: None,
                    ..step.clone()
                })
                .collect();
            let donor_steps = donor_installs(donor);
            if donor_steps
                .iter()
                .any(|step| step.role == Some(StepRole::PreparePinnedTools))
            {
                cloned += 1;
                assert_eq!(
                    installs.iter().collect::<Vec<_>>(),
                    donor_steps,
                    "{id} differs from its donor {donor_id}"
                );
                continue;
            }
            inline += 1;
            let prepare_step = installs
                .iter()
                .find(|step| step.role == Some(StepRole::PreparePinnedTools))
                .ok_or_else(|| format!("{id} installs nothing"))?;
            let command = run_text(prepare_step).ok_or("install is not a shell step")?;
            assert!(
                donor
                    .steps
                    .iter()
                    .filter_map(run_text)
                    .any(|run| run.contains(&command)),
                "{id}: no donor step of {donor_id} runs {command}"
            );
        }
    }
    assert!(cloned > 0, "no producer cloned a donor install");
    assert!(inline > 0, "no producer synthesized its install");
    Ok(())
}

#[test]
fn producers_scrub_and_verify_mise_before_installing() -> Result<(), Box<dyn Error>> {
    for policy in POLICIES {
        let (_repo, prepared) = prepared_for(policy)?;
        let jobs = finalized_jobs(&prepared)?;
        let pinned: BTreeSet<&str> = jobs
            .values()
            .flat_map(|job| &job.steps)
            .filter(|step| step.role == Some(StepRole::ToolsCacheRestore))
            .filter_map(|step| match &step.kind {
                StepKind::Action { with, .. }
                    if ToolsRestoreForm::of(&step.kind) == Some(ToolsRestoreForm::Composite) =>
                {
                    with.get("mise-sha256").map(String::as_str)
                }
                _ => None,
            })
            .collect();
        assert!(!pinned.is_empty());
        for (id, producer) in jobs.iter().filter(|(_, job)| is_tools_producer(job)) {
            let scrub = positions(producer, StepRole::ToolsCacheScrub);
            assert_eq!(scrub.len(), 1, "{policy:?} {id}");
            let step = &producer.steps[scrub[0]];
            let StepKind::Action { uses, with, env } = &step.kind else {
                return Err(format!("{id} scrub is not an action").into());
            };
            assert_eq!(uses, TOOLS_CACHE_SCRUB_USES);
            assert!(env.is_empty());
            assert!(
                with.get("mise-sha256")
                    .is_some_and(|sha| pinned.contains(sha.as_str())),
                "{id} scrubs against a digest no consumer pins"
            );
            assert_eq!(
                step.condition.as_deref(),
                Some(TOOLS_PRODUCER_INSTALL_CONDITION)
            );
            let probe = positions(producer, StepRole::ToolsCacheRestore)[0];
            let save = positions(producer, StepRole::ToolsCacheSave)[0];
            assert!(probe < scrub[0] && scrub[0] < save, "{id}");
            assert!(
                producer
                    .steps
                    .iter()
                    .enumerate()
                    .filter(|(_, step)| is_install_role(step.role))
                    .all(|(at, _)| at > scrub[0]),
                "{id} installs before the scrub"
            );
        }
        let tree = render_staged_tree(&prepared)?;
        let restore = tree
            .get(".github/actions/velnor-tools-cache-restore/action.yml")
            .ok_or("restore action is missing")?;
        let scrub = tree
            .get(".github/actions/velnor-tools-cache-scrub/action.yml")
            .ok_or("scrub action is missing")?;
        let script = restore
            .lines()
            .find(|line| line.contains("verify_mise()"))
            .ok_or("restore admission script is missing")?;
        assert!(scrub.lines().any(|line| line == script));
        assert!(scrub.contains("TOOLS_CACHE_HIT: \"false\""));
        assert!(scrub.contains("TOOLS_SEED_ADMITTED: \"false\""));
    }
    Ok(())
}

pub(super) fn producer_ids_and_digests(workflow: &str) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut produced = BTreeSet::new();
    let mut consumed = BTreeSet::new();
    let mut owner = String::new();
    for line in workflow.lines() {
        if line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':') {
            line.trim().trim_end_matches(':').clone_into(&mut owner);
            if let Some(short) = owner.strip_prefix("tools-producer-") {
                produced.insert(short.to_owned());
            }
        }
        if let Some(digest) = line.trim().strip_prefix("d: ")
            && !owner.starts_with("tools-producer-")
        {
            consumed.insert(digest.chars().take(8).collect());
        }
    }
    (produced, consumed)
}

#[test]
fn every_generated_workflow_has_a_producer_per_tools_identity() -> Result<(), Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(SCHEDULED_SUITES_CONFIG)?;
    let prepared = prepare(repo.path())?;
    let tree = render_staged_tree(&prepared)?;
    let mut covered = BTreeMap::new();
    for file in tree.files.iter().filter(|file| {
        file.path.starts_with(".github/workflows/")
            && Path::new(&file.path)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("yml"))
    }) {
        let (produced, consumed) = producer_ids_and_digests(&file.bytes);
        assert_eq!(produced, consumed, "{}", file.path);
        if !consumed.is_empty() {
            covered.insert(file.path.clone(), consumed.len());
        }
    }
    assert!(
        covered.len() >= 2,
        "the scheduled suite workflow carries no tools identity: {covered:?}"
    );
    Ok(())
}
