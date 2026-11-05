// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use base64::Engine as _;
use std::collections::BTreeMap;

struct Api(BTreeMap<String, Value>);
impl GitHubAdmissionRead for Api {
    fn get(&self, route: &str) -> Result<Value, OrchestratorError> {
        self.0
            .get(route)
            .cloned()
            .ok_or_else(|| internal("fixture_missing"))
    }
    fn get_optional(&self, route: &str) -> Result<Option<Value>, OrchestratorError> {
        Ok(self.0.get(route).cloned())
    }
}

fn fixture() -> Result<(WitnessRequest, Api), Box<dyn std::error::Error>> {
    use crate::crate_release_pr_tree::{TreeItem, git_tree_sha};
    let request = WitnessRequest {
        repository: "owner/repo".into(),
        repository_id: 7,
        workflow_sha: "a".repeat(40),
        workflow_path: ".github/workflows/release-rehearsal.yml".into(),
        run_id: 42,
        run_attempt: 1,
        job_id: 4,
        environment: "qualification".into(),
        reviewer_id: 5,
        output_bundle: false,
        script: "qualification/rehearse.sh".into(),
        source_sha256: "b".repeat(64),
        candidate_pr: 9,
        candidate_sha: "c".repeat(40),
    };
    let config = ReleaseRehearsalConfig {
        script: request.script.clone(),
        environment: request.environment.clone(),
        reviewer_id: request.reviewer_id,
        output_bundle: request.output_bundle,
    };
    let bytes = velnor_actions_workflow_renderer::release_tree::rehearsal::render(
        &config,
        env!("CARGO_PKG_VERSION"),
    )?
    .bytes
    .into_bytes();
    let blob = crate::release_policy_admission::git_blob_sha1(&bytes)?;
    let mut objects = BTreeMap::new();
    let mut entry = TreeItem {
        name: "release-rehearsal.yml".into(),
        mode: "100644".into(),
        sha: blob.clone(),
    };
    for parent_name in ["workflows", ".github", "root"] {
        let tree_sha = git_tree_sha(&[entry.clone()])?;
        let kind = if entry.mode == "040000" {
            "tree"
        } else {
            "blob"
        };
        objects.insert(format!("repos/owner/repo/git/trees/{tree_sha}"), serde_json::json!({"sha":tree_sha,"truncated":false,"tree":[{"path":entry.name,"mode":entry.mode,"type":kind,"sha":entry.sha}]}));
        entry = TreeItem {
            name: parent_name.into(),
            mode: "040000".into(),
            sha: tree_sha,
        };
    }
    objects.insert(format!("repos/owner/repo/git/blobs/{blob}"), serde_json::json!({"sha":blob,"encoding":"base64","size":bytes.len(),"content":base64::engine::general_purpose::STANDARD.encode(&bytes)}));
    objects.insert(
        "repos/owner/repo".into(),
        serde_json::json!({"id":7,"full_name":"owner/repo","default_branch":"main"}),
    );
    objects.insert(
        format!("repos/owner/repo/git/commits/{}", request.workflow_sha),
        serde_json::json!({"sha":request.workflow_sha,"tree":{"sha":entry.sha}}),
    );
    let jobs = ["preflight", "gate", "execute", "witness"].iter().enumerate().map(|(i,name)| serde_json::json!({"name":name,"id":i+1,"run_id":42,"run_attempt":1,"head_sha":request.workflow_sha,"status":"completed","conclusion":"success"})).collect::<Vec<_>>();
    objects.insert(
        "repos/owner/repo/actions/runs/42/attempts/1/jobs?per_page=100&page=1".into(),
        serde_json::json!({"total_count":4,"jobs":jobs}),
    );
    Ok((request, Api(objects)))
}

#[test]
fn canonical_workflow_tree_and_successful_jobs_are_required()
-> Result<(), Box<dyn std::error::Error>> {
    let (request, mut api) = fixture()?;
    let evidence = verify_canonical_rehearsal(&request, "main", &api)?;
    assert_eq!(evidence.workflow_sha256.len(), 64);
    let jobs = api
        .0
        .get_mut("repos/owner/repo/actions/runs/42/attempts/1/jobs?per_page=100&page=1")
        .ok_or("jobs")?;
    jobs["jobs"][2]["id"] = 1.into();
    assert!(verify_canonical_rehearsal(&request, "main", &api).is_err());
    let jobs = api
        .0
        .get_mut("repos/owner/repo/actions/runs/42/attempts/1/jobs?per_page=100&page=1")
        .ok_or("jobs")?;
    jobs["jobs"][2]["id"] = 3.into();
    jobs["jobs"][2]["conclusion"] = "skipped".into();
    assert!(verify_canonical_rehearsal(&request, "main", &api).is_err());
    Ok(())
}

#[test]
fn rejects_different_generator_output_and_forged_git_tree() -> Result<(), Box<dyn std::error::Error>>
{
    let (mut request, mut api) = fixture()?;
    request.script = "qualification/other.sh".into();
    assert!(verify_canonical_rehearsal(&request, "main", &api).is_err());
    request.script = "qualification/rehearse.sh".into();
    let tree = api
        .0
        .values_mut()
        .find(|value| value.get("tree").is_some_and(Value::is_array))
        .ok_or("tree")?;
    tree["tree"][0]["sha"] = "0".repeat(40).into();
    assert!(verify_canonical_rehearsal(&request, "main", &api).is_err());
    Ok(())
}
