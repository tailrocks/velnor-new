// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    path::{Path, PathBuf},
    process::Command,
};
use velnor_actions_contract::config::PremergeEventProofConfig;
use velnor_actions_workflow_renderer::premerge_event_proof::{WORKFLOW_PATH, render};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn document() -> Result<serde_json::Value> {
    let config = PremergeEventProofConfig {
        repository_id: 7,
        owner_id: 11,
        environment: "premerge-proof".into(),
        reviewer_id: 60,
        caller_workflow: ".github/workflows/ci.yml".into(),
    };
    let file = render(&config, "example/project", "main", "0.1.78")?;
    assert_eq!(file.path, WORKFLOW_PATH);
    Ok(serde_saphyr::from_str(&file.bytes)?)
}

#[test]
fn reusable_producer_has_fixed_authority_and_no_candidate_execution() -> Result<()> {
    let document = document()?;
    assert_eq!(document["on"], serde_json::json!({"workflow_call": {}}));
    let jobs = document["jobs"].as_object().ok_or("jobs")?;
    assert_eq!(jobs.len(), 1);
    let job = &jobs["attest"];
    assert_eq!(
        job["permissions"],
        serde_json::json!({"actions":"read","contents":"read","pull-requests":"read","id-token":"write","attestations":"write"})
    );
    assert_eq!(job["environment"], "premerge-proof");
    assert!(job.get("if").is_none());
    assert!(job.get("needs").is_none());
    let steps = job["steps"].as_array().ok_or("steps")?;
    for step in &steps[..steps.len() - 2] {
        assert!(step.get("uses").is_none());
        assert_eq!(
            step["shell"],
            "/bin/bash --noprofile --norc -p -e -o pipefail {0}"
        );
        assert!(step["run"].as_str().ok_or("run")?.chars().count() <= 21_000);
    }
    assert_eq!(
        steps[steps.len() - 2]["uses"],
        velnor_actions_actionlint::ATTEST_BUILD_PROVENANCE_USES
    );
    assert_eq!(
        steps[steps.len() - 1]["uses"],
        velnor_actions_actionlint::UPLOAD_ARTIFACT_ACTION_USES
    );
    assert!(
        steps
            .iter()
            .all(|step| step.get("continue-on-error").is_none() && step.get("if").is_none())
    );
    let env = &steps[steps.len() - 3]["env"];
    assert_eq!(env["VELNOR_PROOF_SIGNER_SHA"], "${{ job.workflow_sha }}");
    assert_eq!(
        env["VELNOR_PROOF_CALLER_WORKFLOW"],
        ".github/workflows/ci.yml"
    );
    Ok(())
}

#[test]
fn generated_probe_and_final_command_obey_native_event_and_api() -> Result<()> {
    let document = document()?;
    let steps = document["jobs"]["attest"]["steps"]
        .as_array()
        .ok_or("steps")?;
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().canonicalize()?;
    let directory = materialize(&steps[..steps.len() - 3], &root)?;
    let run = steps[steps.len() - 3]["run"].as_str().ok_or("final")?;
    let (count, digest, loader) = final_arguments(run, &root)?;
    assert_final_rejects(run, &root, &directory, &digest, count)?;
    let source = root.join("generated-probe.py");
    let mut bytes = Vec::new();
    for index in 0..count {
        bytes.extend(std::fs::read(directory.join(format!("chunk-{index}")))?);
    }
    std::fs::write(&source, bytes)?;
    let output = Command::new("/usr/bin/python3")
        .args(["-B", "-m", "unittest", "discover", "-s"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests"))
        .args(["-p", "premerge_event_probe_test.py"])
        .env("VELNOR_PREMERGE_TEST_SOURCE", source)
        .env("VELNOR_PREMERGE_TEST_LOADER", loader)
        .env("VELNOR_PREMERGE_TEST_DIRECTORY", directory)
        .env("VELNOR_PREMERGE_TEST_COUNT", count.to_string())
        .env("VELNOR_PREMERGE_TEST_DIGEST", digest)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn materialize(steps: &[serde_json::Value], root: &Path) -> Result<PathBuf> {
    let environment = root.join("environment");
    std::fs::write(&environment, "")?;
    let mut directory = None;
    for step in steps {
        let run = step["run"].as_str().ok_or("run")?;
        let mut command = shell(run);
        command
            .env("RUNNER_TEMP", root)
            .env("GITHUB_ENV", &environment);
        if let Some(path) = &directory {
            command.env("VELNOR_PREMERGE_SOURCE", path);
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        directory = Some(
            std::fs::read_to_string(&environment)?
                .trim()
                .strip_prefix("VELNOR_PREMERGE_SOURCE=")
                .ok_or("source path")?
                .to_owned(),
        );
    }
    Ok(PathBuf::from(directory.ok_or("source")?))
}

fn final_arguments(run: &str, root: &Path) -> Result<(usize, String, PathBuf)> {
    let (prefix, call) = run.rsplit_once("\nexecute_source(").ok_or("final call")?;
    let (count, digest) = call
        .split_once(")\n")
        .ok_or("call end")?
        .0
        .split_once(", ")
        .ok_or("arguments")?;
    let digest = digest
        .strip_prefix('\'')
        .and_then(|s| s.strip_suffix('\''))
        .ok_or("digest")?
        .to_owned();
    let loader = prefix
        .split_once("<<'VELNOR_PREMERGE_SOURCE_PY'\n")
        .ok_or("loader")?
        .1;
    let path = root.join("generated-loader.py");
    std::fs::write(&path, loader)?;
    Ok((count.parse()?, digest, path))
}

fn assert_final_rejects(
    run: &str,
    root: &Path,
    directory: &Path,
    digest: &str,
    count: usize,
) -> Result<()> {
    for (script, error) in [
        (run.to_owned(), "premerge event proof failed"),
        (
            run.replace(digest, &"0".repeat(64)),
            "premerge source digest mismatch",
        ),
        (
            run.replace(
                &format!("execute_source({count},"),
                &format!("execute_source({},", count + 1),
            ),
            "invalid premerge source inventory",
        ),
    ] {
        let output = shell(&script)
            .env("RUNNER_TEMP", root)
            .env("VELNOR_PREMERGE_SOURCE", directory)
            .output()?;
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(error),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn shell(run: &str) -> Command {
    let mut command = Command::new("/bin/bash");
    command
        .args([
            "--noprofile",
            "--norc",
            "-p",
            "-e",
            "-o",
            "pipefail",
            "-c",
            run,
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    command
}
