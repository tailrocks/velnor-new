//! The opt-in MBX release probe is scoped to one same-repository PR run.

use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;
use velnor_actions_mise::{
    MR_BOXINGTON_PR_QUALIFICATION_SHA, MR_BOXINGTON_PR_QUALIFICATION_VERSION,
};
use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};
use crate::impl_schema2_routing::{job_body, required_file, workflow_config};

const ACTION_CANDIDATE_SHA: &str = "d0825fbaf3cc36ca2609aa38e71046265a1f1e37";
type StringResult = Result<String, Box<dyn std::error::Error>>;

#[test]
fn same_repository_opt_in_emits_an_isolated_candidate_pair() -> TestResult {
    let config = same_repository_config();
    let repo = make_repo(&config)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/qualification.yml")?;
    let writer = job_body(workflow, "mbx-pr-candidate-write")?;
    let reader = job_body(workflow, "mbx-pr-candidate-read")?;

    assert!(workflow.contains("pull_request: {}"), "{workflow}");
    assert_job_admission(writer);
    assert_job_admission(reader);
    assert_cache_identity(writer, reader);
    assert_writer_permissions(writer);
    assert_reader_permissions(reader);
    assert_candidate_action(writer, reader);
    assert_import_and_reuse_proofs(reader);
    Ok(())
}

#[test]
fn default_read_only_policy_emits_no_pull_request_cache_writer() -> TestResult {
    let repo = make_repo(&workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/qualification.yml")?;

    assert!(!workflow.contains("pull_request:"), "{workflow}");
    assert!(!workflow.contains("mbx-pr-candidate-write"), "{workflow}");
    assert!(!workflow.contains("mbx-pr-candidate-read"), "{workflow}");
    Ok(())
}

#[test]
fn candidate_pin_matches_held_inventory_and_keeps_production_pins() -> TestResult {
    let inventory: Value =
        serde_json::from_str(include_str!("../../../.velnor/freshness-inventory.json"))?;
    let version_policy: Value =
        toml::from_str(include_str!("../../../.velnor/version-policy.toml"))?;
    assert_eq!(
        version_policy["tools"]["mr-boxington"].as_str(),
        Some("1.21.1")
    );
    let tool = inventory["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|item| item["name"] == "mr-boxington"))
        .ok_or("missing MBX freshness entry")?;
    assert_eq!(
        tool["latest"],
        format!("v{MR_BOXINGTON_PR_QUALIFICATION_VERSION}")
    );
    assert_eq!(tool["latest_sha"], MR_BOXINGTON_PR_QUALIFICATION_SHA);
    assert_eq!(tool["pinned"], "1.21.1");
    assert_eq!(tool["qualified"], "1.21.1");
    assert_eq!(tool["status"], "held");

    let action = inventory["actions"]
        .as_array()
        .and_then(|actions| {
            actions
                .iter()
                .find(|item| item["key"] == "jdx/mr-boxington-action")
        })
        .ok_or("missing MBX action freshness entry")?;
    assert_eq!(action["latest"], "v1.7.1");
    assert_eq!(action["latest_sha"], ACTION_CANDIDATE_SHA);
    assert_eq!(action["pinned_version"], "v1.6.0");
    assert_eq!(
        action["pinned_sha"],
        "1687e54eb349cadf61fa38b5813a77875489e8e6"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn generated_candidate_key_separates_attempts_and_rejects_forks() -> TestResult {
    let repo = make_repo(&same_repository_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let workflow = required_file(&tree, ".github/workflows/qualification.yml")?;
    let writer = job_body(workflow, "mbx-pr-candidate-write")?;
    let script = rendered_step_run(writer, "Bind same-repository candidate cache key")?;

    let first = candidate_key_output(&script, "731", "1", "false")?;
    let retry = candidate_key_output(&script, "731", "2", "false")?;
    let next_run = candidate_key_output(&script, "732", "1", "false")?;
    assert_ne!(first, retry);
    assert_ne!(first, next_run);
    assert!(
        first.starts_with(&format!(
            "qualification-mbx-pr-{MR_BOXINGTON_PR_QUALIFICATION_VERSION}-action-{ACTION_CANDIDATE_SHA}-pr-42-"
        )),
        "{first}"
    );
    assert!(candidate_key_output(&script, "731", "1", "true").is_err());
    Ok(())
}

#[cfg(unix)]
fn candidate_key_output(script: &str, run_id: &str, attempt: &str, fork: &str) -> StringResult {
    let scratch = TempDir::new()?;
    let output_file = scratch.path().join("github-output");
    let result = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("GITHUB_OUTPUT", &output_file)
        .env("GITHUB_RUN_ID", run_id)
        .env("GITHUB_RUN_ATTEMPT", attempt)
        .env("RUNNER_OS", "Linux")
        .env("RUNNER_ARCH", "X64")
        .env("MBX_VERSION", MR_BOXINGTON_PR_QUALIFICATION_VERSION)
        .env("MBX_ACTION_SHA", ACTION_CANDIDATE_SHA)
        .env("MBX_EVENT_NAME", "pull_request")
        .env("MBX_REPOSITORY", "tailrocks/velnor-new")
        .env("MBX_HEAD_REPOSITORY", "tailrocks/velnor-new")
        .env("MBX_BASE_REPOSITORY", "tailrocks/velnor-new")
        .env("MBX_HEAD_REPOSITORY_FORK", fork)
        .env("MBX_PR_NUMBER", "42")
        .env("MBX_PR_HEAD_SHA", "a".repeat(40))
        .output()?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
    }
    let contents = fs::read_to_string(output_file)?;
    contents
        .lines()
        .find_map(|line| line.strip_prefix("key="))
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("missing candidate key in {contents}").into())
}

fn rendered_step_run(job: &str, name: &str) -> StringResult {
    let needle = format!("- name: {name}");
    let start = job
        .find(&needle)
        .ok_or_else(|| format!("missing `{needle}` in {job}"))?;
    let tail = &job[start..];
    let scalar = tail
        .lines()
        .find_map(|line| line.trim().strip_prefix("run: "))
        .ok_or_else(|| format!("missing run for `{name}` in {job}"))?;
    Ok(serde_json::from_str(scalar)?)
}

fn same_repository_config() -> String {
    workflow_config().replace(
        "\n[execution]",
        "\npull_request_cache_policy = \"same-repository-scoped\"\n[execution]",
    )
}

fn assert_job_admission(job: &str) {
    for condition in [
        "github.event_name == 'pull_request'",
        "github.repository == github.event.pull_request.head.repo.full_name",
        "github.repository == github.event.pull_request.base.repo.full_name",
        "github.event.pull_request.head.repo.fork == false",
    ] {
        assert!(job.contains(condition), "missing `{condition}`: {job}");
    }
}

fn assert_cache_identity(writer: &str, reader: &str) {
    for job in [writer, reader] {
        for identity in [
            "MBX_EVENT_NAME: ${{ github.event_name }}",
            "MBX_HEAD_REPOSITORY: ${{ github.event.pull_request.head.repo.full_name }}",
            "MBX_BASE_REPOSITORY: ${{ github.event.pull_request.base.repo.full_name }}",
            "toJSON(github.event.pull_request.head.repo.fork)",
            "qualification-mbx-pr-${MBX_VERSION}-action-${MBX_ACTION_SHA}",
            "${GITHUB_RUN_ID}-attempt-${GITHUB_RUN_ATTEMPT}",
            "${MBX_PR_NUMBER}-${MBX_PR_HEAD_SHA}",
            "no_fallback=%s-no-fallback-",
        ] {
            assert!(job.contains(identity), "missing `{identity}`: {job}");
        }
    }
    assert!(writer.contains("cache-key: ${{ steps.mbx-pr-key.outputs.key }}"));
    assert!(reader.contains("cache-key: ${{ steps.mbx-pr-key.outputs.key }}"));
    assert!(writer.contains("restore-keys: ${{ steps.mbx-pr-key.outputs.no_fallback }}"));
    assert!(reader.contains("restore-keys: ${{ steps.mbx-pr-key.outputs.no_fallback }}"));
}

fn assert_writer_permissions(writer: &str) {
    assert!(writer.contains("actions: write"), "{writer}");
    assert!(writer.contains("ACTIONS_CACHE_MODE: write"), "{writer}");
    assert!(
        writer.contains("save-on-pull-request: \"true\""),
        "{writer}"
    );
    assert!(writer.contains("cache-save-eligible"), "{writer}");
    assert!(writer.contains("same-repository pull request"), "{writer}");
    assert!(writer.contains("id: mbx-pr-key"), "{writer}");
    assert!(
        writer.contains("test \\\"$CACHE_HIT\\\" = 'false'"),
        "{writer}"
    );
}

fn assert_reader_permissions(reader: &str) {
    assert!(
        reader.contains("needs:\n      - mbx-pr-candidate-write"),
        "{reader}"
    );
    assert!(reader.contains("actions: read"), "{reader}");
    assert!(reader.contains("ACTIONS_CACHE_MODE: read"), "{reader}");
    assert!(
        reader.contains("save-on-pull-request: \"false\""),
        "{reader}"
    );
    assert!(
        reader.contains("test \\\"$CACHE_HIT\\\" = 'true'"),
        "{reader}"
    );
}

fn assert_candidate_action(writer: &str, reader: &str) {
    for job in [writer, reader] {
        assert!(
            job.contains(&format!(
                "uses: jdx/mr-boxington-action@{ACTION_CANDIDATE_SHA}"
            )),
            "{job}"
        );
        assert!(
            job.contains(&format!("version: {MR_BOXINGTON_PR_QUALIFICATION_VERSION}")),
            "{job}"
        );
        assert!(job.contains("isolate-objects-cache: \"true\""), "{job}");
        assert!(job.contains("github-cache-mode: objects"), "{job}");
    }
}

fn assert_import_and_reuse_proofs(reader: &str) {
    for proof in [
        "Require imported MBX objects",
        "mbx cache stats --json",
        ".objects > 0",
        "Compile MBX cache probe",
        "mbx stats --json",
        ".savings.cached_compilations > 0",
        "df -B1 -P",
        "df -i -P",
    ] {
        assert!(reader.contains(proof), "missing `{proof}`: {reader}");
    }
}
