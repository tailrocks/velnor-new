// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_mbx::{
    MBX_BUNDLE_ROOT_ENV, MBX_BUNDLE_SNAPSHOT_ENV,
};
use velnor_actions_contract::{Job, StepId, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare};

use super::one_job_per_crate::mbx_repo;
use super::workflow_configuration_fixture::add_independent_mbx_package;

fn render(extra_roots: &[&str]) -> Result<BTreeMap<String, Job>, Box<dyn Error>> {
    let repo = mbx_repo(false)?;
    for (index, dir) in extra_roots.iter().enumerate() {
        add_independent_mbx_package(repo.path(), dir, &format!("extra{index}"))?;
    }
    let prepared = prepare(repo.path())?;
    Ok(finalized_jobs(&prepared)?)
}

fn snapshot_bindings(jobs: &BTreeMap<String, Job>) -> BTreeSet<(String, String)> {
    jobs.values()
        .flat_map(|job| &job.steps)
        .filter(|step| step.role == Some(StepRole::MbxBundleKey))
        .filter_map(|step| match &step.kind {
            StepKind::Shell { env, .. } => Some((
                env.get(MBX_BUNDLE_ROOT_ENV)?.clone(),
                env.get(MBX_BUNDLE_SNAPSHOT_ENV)?.clone(),
            )),
            _ => None,
        })
        .collect()
}

fn plan_key_script(jobs: &BTreeMap<String, Job>) -> Result<String, Box<dyn Error>> {
    let step = jobs
        .get("plan")
        .and_then(|plan| plan.steps.iter().find(|s| s.id == Some(StepId::CacheKeys)))
        .ok_or("plan job lost its cache keys step")?;
    match &step.kind {
        StepKind::Shell { run, .. } => Ok(run.join(" ")),
        _ => Err("cache keys step left its shell form".into()),
    }
}

fn error_of(extra_roots: &[&str]) -> Result<String, Box<dyn Error>> {
    match render(extra_roots) {
        Ok(_) => Err(format!("roots {extra_roots:?} generated a workflow").into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn generation_rejects_colliding_or_delimiter_roots() -> Result<(), Box<dyn Error>> {
    let widest = "d".repeat(48);
    let jobs = render(&[widest.as_str()])?;
    let bindings = snapshot_bindings(&jobs);
    let script = plan_key_script(&jobs)?;
    let wanted = [
        (
            "crates/nested",
            "${{needs.plan.outputs.state_crates_nested_snapshot}}",
        ),
        (
            widest.as_str(),
            &format!("${{{{needs.plan.outputs.state_{widest}_snapshot}}}}"),
        ),
    ];
    for (root, expr) in wanted {
        if !bindings.contains(&(root.to_owned(), expr.to_owned())) {
            return Err(format!("no snapshot binding for root {root}: {bindings:?}").into());
        }
        let name = expr
            .trim_start_matches("${{needs.plan.outputs.")
            .trim_end_matches("}}");
        if !script.contains(&format!("{name}=$snap")) {
            return Err(format!("plan never publishes {name}").into());
        }
    }
    for member in [
        "'crates/nested'",
        "'crates/nested/.cargo'",
        "'crates/nested/Cargo.toml'",
    ] {
        if !script.contains(member) {
            return Err(format!("plan snapshot lost its input {member}").into());
        }
    }
    if CacheFamily::MbxBundle.max_key_bytes()
        > velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES
    {
        return Err("widest state key outgrew the cache cap".into());
    }
    for (roots, needle) in [
        (vec!["a/b", "a-b"], "root_slug_collision"),
        (vec!["crates/s256-x"], "root_collides_with_digest_delimiter"),
        (vec!["e".repeat(49).leak() as &str], "root_not_sluggable"),
    ] {
        let message = error_of(&roots)?;
        if !message.contains(needle) {
            return Err(format!("roots {roots:?} failed without {needle}: {message}").into());
        }
    }
    Ok(())
}
