// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::declared_fixture::{TestResult, rendered_text};
use crate::yaml_tree::document;

const WORKFLOW: &str = "release";
const DRIFT: &str = "tag-ruleset-drift";
const WRITER: &str = "publish-family";
const BUILDS: [&str; 4] = [
    "build-velnor-ci-linux-x64",
    "build-velnor-ci-linux-arm64",
    "build-velnor-gen-linux-x64",
    "build-velnor-gen-linux-arm64",
];
const TAG_PUSH: &str = "${{ github.ref_type == 'tag' }}";

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
fn the_committed_ruleset_drift_workflow_is_what_the_repository_config_renders() -> TestResult {
    let config = read(".velnor/workflows.toml")?;
    assert_eq!(
        read(&format!(".github/workflows/{DRIFT}.yml"))?,
        rendered_text(&config, DRIFT)?
    );
    Ok(())
}

fn needs(job: &Value) -> Vec<&str> {
    job["needs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

#[test]
fn the_release_dag_gates_the_tag_on_default_branch_ancestry_before_anything_else() -> TestResult {
    let workflow = committed(WORKFLOW)?;
    let jobs = jobs(&workflow)?;
    let mut ids: Vec<&str> = jobs.keys().map(String::as_str).collect();
    ids.sort_unstable();
    let mut expected = vec![
        "gate",
        "family-versions",
        "family",
        "sums-family",
        "verify-family",
        WRITER,
    ];
    expected.extend(BUILDS);
    expected.sort_unstable();
    assert_eq!(ids, expected);
    assert_eq!(
        workflow["on"]["push"]["tags"],
        serde_json::json!(["tools-v*"])
    );
    assert_eq!(workflow["on"].as_object().map(Map::len), Some(1));
    let gate = &jobs["gate"];
    assert_eq!(gate["if"], TAG_PUSH);
    assert!(gate.get("needs").is_none());
    let compare = runs(gate)
        .into_iter()
        .find(|run| run.contains("/compare/$DEFAULT_BRANCH...$COMMIT"))
        .ok_or("the gate never compares the tag with the default branch")?;
    assert!(compare.ends_with(
        "--jq '.status == \"behind\" or .status == \"identical\" or error(\"the tagged commit is not on the default branch\")'"
    ));
    assert!(position(&runs(gate), "velnor-ci release-gate").is_ok());
    for (id, job) in jobs.iter().filter(|(id, _)| *id != "gate") {
        assert!(needs(job).contains(&"gate"), "{id}");
    }
    for build in BUILDS {
        assert!(needs(&jobs[build]).contains(&"family"), "{build}");
    }
    assert_eq!(needs(&jobs["sums-family"]).len(), 2 + BUILDS.len());
    assert!(needs(&jobs["verify-family"]).contains(&"sums-family"));
    assert!(needs(&jobs[WRITER]).contains(&"verify-family"));
    Ok(())
}

#[test]
fn only_the_publish_job_can_write_contents_and_verification_precedes_it() -> TestResult {
    let workflow = committed(WORKFLOW)?;
    assert_eq!(workflow["permissions"], serde_json::json!({}));
    let jobs = jobs(&workflow)?;
    for (id, job) in jobs {
        let permissions = job["permissions"].as_object().ok_or("no permissions")?;
        assert_eq!(
            permissions
                .get("contents")
                .is_some_and(|access| access == "write"),
            id == WRITER,
            "{id}"
        );
        let attests = permissions.contains_key("id-token")
            || permissions.contains_key("attestations") && permissions["attestations"] == "write";
        assert_eq!(attests, id == "sums-family", "{id}");
        assert_eq!(
            runs(job)
                .iter()
                .any(|run| run.starts_with("velnor-ci release publish")),
            id == WRITER,
            "{id}"
        );
        assert_eq!(
            uses(job)
                .iter()
                .any(|action| action.starts_with("actions/checkout@")),
            id == "family-versions" || BUILDS.contains(&id.as_str()),
            "{id}"
        );
    }
    for build in BUILDS {
        let build_runs = runs(&jobs[build]);
        let built = position(&build_runs, "cargo build --profile release-tools ")?;
        let smoke = build_runs
            .iter()
            .position(|run| run.ends_with(" check-tag \"$TAG\""))
            .ok_or("no tag check")?;
        let packed = position(&build_runs, "tar -czf ")?;
        assert!(built < smoke && smoke < packed, "{build}");
    }
    let sums = runs(&jobs["sums-family"]);
    let record = position(&sums, "echo \"$COMMIT\" > COMMIT")?;
    let written = position(&sums, "sha256sum velnor-ci-x86_64")?;
    assert!(record < written);
    let attest = uses(&jobs["sums-family"])
        .iter()
        .any(|action| action.starts_with("actions/attest-build-provenance@"));
    assert!(attest);
    assert!(
        runs(&jobs["verify-family"])
            .iter()
            .any(|run| run.contains("gh attestation verify"))
    );
    Ok(())
}

#[test]
fn the_ruleset_drift_check_runs_nightly_and_on_dispatch_and_reads_its_admin_token_in_one_step()
-> TestResult {
    let workflow = committed(DRIFT)?;
    assert_eq!(
        workflow["on"],
        serde_json::json!({
            "workflow_dispatch": {},
            "schedule": [{ "cron": "23 4 * * *" }]
        })
    );
    assert_eq!(workflow["permissions"], serde_json::json!({}));
    let jobs = jobs(&workflow)?;
    assert_eq!(jobs.keys().collect::<Vec<_>>(), [DRIFT]);
    let job = &jobs[DRIFT];
    assert_eq!(
        job["permissions"],
        serde_json::json!({ "contents": "read" })
    );
    assert!(job.get("env").is_none());
    let steps = job["steps"].as_array().ok_or("no steps")?;
    let readers: Vec<&Value> = steps
        .iter()
        .filter(|step| step["env"].get("TAG_RULESET_ADMIN_READ_TOKEN").is_some())
        .collect();
    let [step] = readers.as_slice() else {
        return Err("exactly one step reads the admin token".into());
    };
    assert_eq!(
        step["env"]["TAG_RULESET_ADMIN_READ_TOKEN"],
        "${{ secrets.TAG_RULESET_ADMIN_READ_TOKEN }}"
    );
    assert_eq!(step["run"], "node scripts/tag-ruleset-drift.mjs");
    Ok(())
}
