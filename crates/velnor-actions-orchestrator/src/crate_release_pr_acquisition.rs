// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{OrchestratorError, internal::internal};
use serde_json::Value;

pub(crate) struct RunBinding<'a> {
    pub repository: &'a str,
    pub branch: &'a str,
    pub sha: &'a str,
    pub run_id: u64,
    pub attempt: u32,
}

pub(crate) fn verify_run_and_producer(
    binding: &RunBinding<'_>,
    repository: &Value,
    run: &Value,
    jobs: &Value,
    branch_ref: &Value,
) -> Result<u64, OrchestratorError> {
    let invalid = || internal("release_pr_run_unverified");
    let repo_id = repository["id"]
        .as_u64()
        .filter(|id| *id > 0)
        .ok_or_else(invalid)?;
    if binding.run_id == 0
        || binding.attempt != 1
        || repository["full_name"].as_str() != Some(binding.repository)
        || repository["default_branch"].as_str() != Some(binding.branch)
        || run["id"].as_u64() != Some(binding.run_id)
        || run["run_attempt"].as_u64() != Some(u64::from(binding.attempt))
        || !matches!(run["path"].as_str(), Some(path) if path == ".github/workflows/release.yml" || path == format!(".github/workflows/release.yml@{}", binding.branch))
        || run["event"].as_str() != Some("push")
        || run["status"].as_str() != Some("in_progress")
        || run["head_sha"].as_str() != Some(binding.sha)
        || run["head_branch"].as_str() != Some(binding.branch)
        || run["repository"]["id"].as_u64() != Some(repo_id)
        || run["head_repository"]["id"].as_u64() != Some(repo_id)
        || run["repository"]["full_name"].as_str() != Some(binding.repository)
        || run["head_repository"]["full_name"].as_str() != Some(binding.repository)
        || branch_ref["ref"].as_str() != Some(format!("refs/heads/{}", binding.branch).as_str())
        || branch_ref["object"]["type"].as_str() != Some("commit")
        || branch_ref["object"]["sha"].as_str() != Some(binding.sha)
    {
        return Err(invalid());
    }
    let entries = jobs["jobs"].as_array().ok_or_else(invalid)?;
    if entries.len() >= 100 || jobs["total_count"].as_u64() != Some(entries.len() as u64) {
        return Err(invalid());
    }
    let mut matched = entries
        .iter()
        .filter(|job| job["name"].as_str() == Some("Release preparation"));
    let job = matched.next().ok_or_else(invalid)?;
    if matched.next().is_some()
        || job["id"].as_u64().is_none_or(|id| id == 0)
        || job["run_id"].as_u64() != Some(binding.run_id)
        || job["head_sha"].as_str() != Some(binding.sha)
        || job["status"].as_str() != Some("completed")
        || job["conclusion"].as_str() != Some("success")
    {
        return Err(invalid());
    }
    Ok(repo_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn trusts_only_current_successful_producer_run() -> Result<(), Box<dyn std::error::Error>> {
        let sha = "a".repeat(40);
        let binding = RunBinding {
            repository: "example/demo",
            branch: "main",
            sha: &sha,
            run_id: 42,
            attempt: 1,
        };
        let repository = json!({"id":7,"full_name":"example/demo","default_branch":"main"});
        let run = json!({"id":42,"run_attempt":1,"path":".github/workflows/release.yml","event":"push","status":"in_progress","head_sha":sha,"head_branch":"main","repository":{"id":7,"full_name":"example/demo"},"head_repository":{"id":7,"full_name":"example/demo"}});
        let jobs = json!({"total_count":1,"jobs":[{"id":5,"name":"Release preparation","run_id":42,"run_attempt":1,"head_sha":sha,"status":"completed","conclusion":"success"}]});
        let reference = json!({"ref":"refs/heads/main","object":{"type":"commit","sha":sha}});
        assert_eq!(
            verify_run_and_producer(&binding, &repository, &run, &jobs, &reference)?,
            7
        );
        for (target, path, value) in [
            (0, "/default_branch", json!("other")),
            (1, "/event", json!("workflow_dispatch")),
            (1, "/head_sha", json!("b".repeat(40))),
            (1, "/run_attempt", json!(2)),
            (1, "/path", json!(".github/workflows/other.yml")),
            (1, "/head_repository/id", json!(8)),
            (2, "/jobs/0/conclusion", json!("failure")),
            (2, "/jobs/0/run_id", json!(43)),
            (2, "/total_count", json!(2)),
            (3, "/object/sha", json!("b".repeat(40))),
        ] {
            let (mut a, mut b, mut c, mut d) = (
                repository.clone(),
                run.clone(),
                jobs.clone(),
                reference.clone(),
            );
            let object = match target {
                0 => &mut a,
                1 => &mut b,
                2 => &mut c,
                _ => &mut d,
            };
            *object.pointer_mut(path).ok_or("missing fixture key")? = value;
            assert!(
                verify_run_and_producer(&binding, &a, &b, &c, &d).is_err(),
                "accepted {path}"
            );
        }
        let mut duplicate = jobs.clone();
        duplicate["jobs"]
            .as_array_mut()
            .ok_or("jobs")?
            .push(jobs["jobs"][0].clone());
        duplicate["total_count"] = json!(2);
        assert!(
            verify_run_and_producer(&binding, &repository, &run, &duplicate, &reference).is_err()
        );
        Ok(())
    }
}

pub(crate) fn verify_reviewed_workflow(
    bytes: &[u8],
    intent: &velnor_actions_contract::config::CrateReleaseIntent,
) -> Result<(), OrchestratorError> {
    use velnor_actions_contract::config::RustReleaseConfig;
    use velnor_actions_mise::{MISE_VERSION, PinnedTool, ToolCatalog};
    use velnor_actions_workflow_renderer::{MiseSetup, setup::MISE_BINARY_SHA256_LINUX_X64};
    let invalid = || internal("release_pr_workflow_unverified");
    if bytes.len() > 1024 * 1024 || intent.generator_version != env!("CARGO_PKG_VERSION") {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut options = serde_saphyr::Options::default();
    options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
    let workflow: Value =
        serde_saphyr::from_str_with_options(text, options).map_err(|_| invalid())?;
    let catalog = ToolCatalog::pinned()
        .with_project_tool(PinnedTool::Rust, &intent.rust_version)
        .map_err(|_| invalid())?
        .with_export_rustup_toolchain(false);
    if catalog.version(PinnedTool::ReleasePlz) != intent.release_plz_version {
        return Err(invalid());
    }
    let release = RustReleaseConfig {
        release_pr: true,
        manifest_path: intent.manifest_path.clone(),
        environment: intent.environment.clone(),
        authentication: Some(intent.authentication),
        ..RustReleaseConfig::default()
    };
    let mise = MiseSetup {
        uses: format!(
            "jdx/mise-action@{}",
            velnor_actions_actionlint::actions::MISE_ACTION_SHA
        ),
        version: MISE_VERSION.to_owned(),
        sha256: MISE_BINARY_SHA256_LINUX_X64.to_owned(),
    };
    let acquire = velnor_actions_workflow_renderer::release_generator_bootstrap::acquire_step(
        &intent.generator_version,
        &intent.runner,
        &intent.generator_source_sha,
        &intent.generator_manifest_sha256,
    )
    .map_err(|_| invalid())?;
    let preparation_gate = format!(
        "github.repository == '{}' && github.event_name == 'push' && github.ref == 'refs/heads/{}' && needs.release-preflight.result == 'success' && needs.release-preflight.outputs.eligible == 'false'",
        intent.repository, intent.default_branch
    );
    let jobs = crate::release_steps::assemble_jobs(&crate::release_steps::JobInputs {
        release: &release,
        gate: String::new(),
        preparation_gate,
        registry: None,
        label: &intent.runner,
        mise: &mise,
        catalog: &catalog,
        acquire: &acquire,
        generator_version: &intent.generator_version,
    })?;
    for id in [
        "release-preflight",
        "release-preparation",
        "release-pr-create",
    ] {
        let expected_yaml = velnor_actions_workflow_renderer::release_tree::render_release_job(
            jobs.get(id).ok_or_else(invalid)?,
        )
        .map_err(|_| invalid())?;
        let expected: Value = serde_saphyr::from_str(&expected_yaml).map_err(|_| invalid())?;
        if workflow["jobs"][id] != expected {
            return Err(invalid());
        }
    }
    Ok(())
}
