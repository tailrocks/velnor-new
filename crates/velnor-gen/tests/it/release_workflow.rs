// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::declared_fixture::{TestResult, rendered_text};
use crate::yaml_tree::document;

const WORKFLOW: &str = "release-rust-ci-tools";
const DRIFT: &str = "tag-ruleset-drift";
const WRITER: &str = "release";
const BUILDS: [&str; 2] = ["build-linux-x64", "build-linux-arm64"];
const TAG_PUSH: &str = "${{ github.event_name == 'push' && (startsWith(github.ref, 'refs/tags/rust-ci-v') || startsWith(github.ref, 'refs/tags/tools-v')) }}";

fn read(path: &str) -> Result<String, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Ok(fs::read_to_string(root.join(path))?)
}

fn committed(file: &str) -> Result<Value, Box<dyn Error>> {
    Ok(document(&read(&format!(".github/workflows/{file}.yml"))?)?)
}

fn jobs(workflow: &Value) -> Result<&Map<String, Value>, Box<dyn Error>> {
    Ok(workflow["jobs"].as_object().ok_or("no jobs")?)
}

fn runs(job: &Value) -> Vec<&str> {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["run"].as_str())
        .collect()
}

fn uses(job: &Value) -> Vec<&str> {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["uses"].as_str())
        .collect()
}

fn position(runs: &[&str], prefix: &str) -> Result<usize, Box<dyn Error>> {
    runs.iter()
        .position(|run| run.starts_with(prefix))
        .ok_or_else(|| format!("no run starts with {prefix}").into())
}

#[test]
fn both_committed_workflows_are_what_the_repository_config_renders() -> TestResult {
    let config = read(".velnor/workflows.toml")?;
    for file in [WORKFLOW, DRIFT] {
        assert_eq!(
            read(&format!(".github/workflows/{file}.yml"))?,
            rendered_text(&config, file)?,
            "{file}"
        );
    }
    Ok(())
}

#[test]
fn the_release_dag_gates_the_tag_on_default_branch_ancestry_before_building() -> TestResult {
    let workflow = committed(WORKFLOW)?;
    let jobs = jobs(&workflow)?;
    let mut ids: Vec<&str> = jobs.keys().map(String::as_str).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "build-linux-arm64",
            "build-linux-x64",
            "checksums",
            "gate",
            WRITER
        ]
    );
    let gate = &jobs["gate"];
    assert_eq!(gate["if"], TAG_PUSH);
    assert!(gate.get("needs").is_none());
    assert_eq!(uses(gate), [] as [&str; 0]);
    let compare = runs(gate)
        .into_iter()
        .find(|run| run.contains("/compare/$DEFAULT_BRANCH...$GITHUB_SHA"))
        .ok_or("the gate never compares the tag with the default branch")?;
    assert!(compare.ends_with(
        "--jq '.status == \"behind\" or .status == \"identical\" or error(\"the tagged commit is not on the default branch\")'"
    ));
    for build in BUILDS {
        assert_eq!(jobs[build]["needs"], serde_json::json!(["gate"]), "{build}");
    }
    assert_eq!(jobs["checksums"]["needs"], serde_json::json!(BUILDS));
    assert_eq!(jobs[WRITER]["needs"], serde_json::json!(["checksums"]));
    for (id, job) in jobs {
        assert_eq!(job.get("if").is_some(), id == "gate", "{id}");
        for step in job["steps"].as_array().ok_or("no steps")? {
            assert!(step.get("if").is_none(), "{id}");
            assert!(step.get("continue-on-error").is_none(), "{id}");
        }
    }
    Ok(())
}

#[test]
fn only_the_release_job_can_write_and_it_checks_out_nothing_and_verifies_before_it_publishes()
-> TestResult {
    let workflow = committed(WORKFLOW)?;
    assert_eq!(workflow["permissions"], serde_json::json!({}));
    let jobs = jobs(&workflow)?;
    for (id, job) in jobs {
        let permissions = job["permissions"].as_object().ok_or("no permissions")?;
        assert_eq!(
            permissions
                .values()
                .filter(|access| *access == "write")
                .count(),
            usize::from(id == WRITER),
            "{id}"
        );
        assert_eq!(
            uses(job)
                .iter()
                .any(|action| action.starts_with("actions/checkout@")),
            BUILDS.contains(&id.as_str()),
            "{id}"
        );
        assert_eq!(
            runs(job).iter().any(|run| run.starts_with("gh release")),
            id == WRITER,
            "{id}"
        );
    }
    for build in BUILDS {
        let tag_checks: Vec<&str> = runs(&jobs[build])
            .into_iter()
            .filter(|run| run.ends_with(" check-tag \"$GITHUB_REF_NAME\""))
            .collect();
        assert_eq!(
            tag_checks,
            [
                "env -C stage ./velnor-ci check-tag \"$GITHUB_REF_NAME\"",
                "env -C stage ./velnor-gen check-tag \"$GITHUB_REF_NAME\""
            ]
        );
    }
    let writer = runs(&jobs[WRITER]);
    let verify = position(&writer, "env -C dist sha256sum --check --strict SHA256SUMS")?;
    let publish = position(&writer, "gh release create")?;
    assert!(verify < publish);
    assert!(writer[publish].contains(" --prerelease "));
    assert_eq!(writer[publish].matches(" dist/").count(), 7);
    assert!(writer[publish].contains(" dist/COMMIT "));
    let sums = runs(&jobs["checksums"]);
    let record = position(&sums, "echo \"$GITHUB_SHA\" > dist/COMMIT")?;
    let written = position(&sums, "env -C dist sha256sum COMMIT ")?;
    assert!(record < written);
    assert!(sums[written].ends_with(" > dist/SHA256SUMS"));
    Ok(())
}

#[test]
fn the_ruleset_drift_check_reads_its_admin_token_only_on_dispatch_and_holds_no_scope() -> TestResult
{
    let workflow = committed(DRIFT)?;
    assert_eq!(
        workflow["on"],
        serde_json::json!({ "workflow_dispatch": {} })
    );
    assert_eq!(workflow["permissions"], serde_json::json!({}));
    let jobs = jobs(&workflow)?;
    assert_eq!(jobs.keys().collect::<Vec<_>>(), [DRIFT]);
    let job = &jobs[DRIFT];
    assert_eq!(job["permissions"], serde_json::json!({}));
    let [step] = job["steps"].as_array().ok_or("no steps")?.as_slice() else {
        return Err("the drift job has more than one step".into());
    };
    assert_eq!(
        step["env"]["TAG_RULESET_ADMIN_READ_TOKEN"],
        "${{ secrets.TAG_RULESET_ADMIN_READ_TOKEN }}"
    );
    assert!(step["with"].get("github-token").is_none());
    let script = step["with"]["script"].as_str().ok_or("no script")?;
    assert!(script.contains("const admin = getOctokit(process.env.TAG_RULESET_ADMIN_READ_TOKEN);"));
    assert_eq!(script.matches("await admin.").count(), 2);
    assert!(!script.contains("await github."));
    for name in ["protect-tags", "release-tags", "refs/tags/tools-v*"] {
        assert!(script.contains(name), "{name}");
    }
    Ok(())
}
