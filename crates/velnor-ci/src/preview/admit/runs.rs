// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::error::CiError;
use crate::preview::authority::Client;
use crate::preview::authority::model::positive;
use crate::preview::json::{Field, Res, is_hex, require};
use crate::preview::{
    CI_WORKFLOW, QUALIFICATION_WORKFLOW, REPOSITORY, REPOSITORY_ID, SIGNER_WORKFLOW,
};

/// The one item of `items`, and a refusal for none or several.
fn only<'a>(mut items: impl Iterator<Item = &'a Value>) -> Res<&'a Value> {
    match (items.next(), items.next()) {
        (Some(item), None) => Ok(item),
        _ => Err(CiError::new("source preview item not unique")),
    }
}

/// Port of `verify_jobs`: the attempt has fewer than 100 jobs, and each job of `expected`
/// is there once, succeeded, and belongs to this run, attempt and commit.
pub fn verify_jobs(
    client: &dyn Client,
    run: &Value,
    expected: &[&str],
) -> Res<BTreeMap<String, Value>> {
    let result = client.get(&format!(
        "/actions/runs/{}/attempts/{}/jobs?per_page=100",
        positive(run.at("id"))?,
        positive(run.at("run_attempt"))?
    ))?;
    let jobs = result.at("jobs").as_array().map_or(&[][..], Vec::as_slice);
    require(
        !jobs.is_empty() && jobs.len() < 100 && result.at("total_count").is_u(jobs.len() as u64),
    )?;
    let ids: BTreeSet<u64> = jobs
        .iter()
        .map(|job| positive(job.at("id")))
        .collect::<Res<_>>()?;
    require(ids.len() == jobs.len())?;
    let head = run.at("head_sha").text();
    let mut selected = BTreeMap::new();
    for name in expected {
        let job = only(jobs.iter().filter(|job| job.at("name").is_s(name)))?;
        require(
            job.at("status").is_s("completed")
                && job.at("conclusion").is_s("success")
                && job.at("run_id") == run.at("id")
                && job.at("run_attempt") == run.at("run_attempt")
                && !head.is_empty()
                && job.at("head_sha").is_s(head),
        )?;
        selected.insert((*name).to_owned(), job.clone());
    }
    Ok(selected)
}

fn workflow_path(run: &Value) -> (&str, Option<&str>) {
    let mut parts = run.at("path").text().splitn(2, '@');
    (parts.next().unwrap_or(""), parts.next())
}

fn from_repository(run: &Value) -> bool {
    let head = run.at("head_repository");
    head.at("full_name").is_s(REPOSITORY) && head.at("id").is_u(REPOSITORY_ID)
}

/// Port of `successful_run`: the newest successful run of the workflow at `path` for
/// `source` on `main`, with the jobs `expected` succeeded.
pub fn successful_run(
    client: &dyn Client,
    source: &str,
    path: &str,
    event: &str,
    expected: &[&str],
) -> Res<Value> {
    let file = path.rsplit('/').next().unwrap_or("");
    let result = client.get(&format!(
        "/actions/workflows/{file}/runs?head_sha={source}&event={event}&status=success&per_page=20"
    ))?;
    let runs = result
        .at("workflow_runs")
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    require(!runs.is_empty() && runs.len() <= 20)?;
    let mut accepted = Vec::new();
    for run in runs {
        let (file_path, git_ref) = workflow_path(run);
        if run.at("head_sha").is_s(source)
            && run.at("head_branch").is_s("main")
            && run.at("event").is_s(event)
            && run.at("status").is_s("completed")
            && run.at("conclusion").is_s("success")
            && file_path == path
            && git_ref.is_none_or(|r| r == "main" || r == "refs/heads/main")
            && from_repository(run)
        {
            accepted.push((positive(run.at("id"))?, run));
        }
    }
    let newest = accepted
        .iter()
        .fold(None, |best: Option<&(u64, &Value)>, item| {
            best.filter(|b| b.0 >= item.0).or(Some(item))
        });
    let &(id, run) = newest.ok_or_else(|| CiError::new("source preview qualification identity"))?;
    verify_jobs(client, run, expected)?;
    Ok(json!({
        "workflow_path": path,
        "workflow_ref": format!("{REPOSITORY}/{path}@refs/heads/main"),
        "workflow_sha": source,
        "run_id": id,
        "run_attempt": positive(run.at("run_attempt"))?,
    }))
}

/// Port of `producer_artifact`: the payload artifact of the producer run of `target`, and
/// the id of its qualification job.
pub fn producer_artifact(
    client: &dyn Client,
    source: &str,
    run_id: u64,
    attempt: u64,
    target: &str,
) -> Res<(Value, u64)> {
    let path = format!("/actions/runs/{run_id}");
    let run = client.get(&format!("{path}/attempts/{attempt}"))?;
    require(
        run.at("id").is_u(run_id)
            && run.at("run_attempt").is_u(attempt)
            && run.at("head_sha").is_s(source)
            && run.at("head_branch").is_s("main")
            && run.at("event").is_s("push")
            && run.at("status").is_s("completed")
            && run.at("conclusion").is_s("success")
            && workflow_path(&run).0 == SIGNER_WORKFLOW
            && from_repository(&run),
    )?;
    let label = if target == "linux" { "Linux" } else { "Darwin" };
    let qualify = format!("Qualify source preview {label}");
    let jobs = verify_jobs(
        client,
        &run,
        &[&format!("Build source preview {label}"), &qualify],
    )?;
    let inventory = client.get(&format!("{path}/artifacts?per_page=100"))?;
    let artifacts = inventory
        .at("artifacts")
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    require(
        inventory.at("artifacts").is_array()
            && artifacts.len() < 100
            && inventory.at("total_count").is_u(artifacts.len() as u64),
    )?;
    let name = format!("source-preview-payload-{target}-{attempt}");
    let artifact = only(artifacts.iter().filter(|a| a.at("name").is_s(&name)))?;
    let digest = artifact.at("digest").text();
    require(
        positive(artifact.at("id")).is_ok()
            && artifact.at("expired") == &Value::Bool(false)
            && digest
                .strip_prefix("sha256:")
                .is_some_and(|d| is_hex(d, 64))
            && artifact.at("workflow_run").at("id").is_u(run_id)
            && artifact.at("workflow_run").at("head_sha").is_s(source),
    )?;
    let job = jobs
        .get(&qualify)
        .ok_or_else(|| CiError::new("source preview job"))?;
    Ok((artifact.clone(), positive(job.at("id"))?))
}

/// Port of `qualification_proofs`: the successful `ci.yml` and source qualification runs
/// of the source commit.
pub fn qualification_proofs(client: &dyn Client, source: &str) -> Res<(Value, Value)> {
    let ci = successful_run(client, source, CI_WORKFLOW, "push", &["Required verdict"])?;
    let qualification = successful_run(
        client,
        source,
        QUALIFICATION_WORKFLOW,
        "workflow_dispatch",
        &[
            "Source qualification ordinary harness",
            "Source qualification keep harness",
            "Source qualification generation and repository checks",
            "Source qualification",
        ],
    )?;
    Ok((ci, qualification))
}
