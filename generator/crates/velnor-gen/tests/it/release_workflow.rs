// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_render::{
    RenderedJob, RenderedStep, RenderedWorkflow, SETUP_VELNOR_ACTION, check_rules,
};

type TestResult = Result<(), Box<dyn Error>>;

const WORKFLOW: &str = ".github/workflows/release-rust-ci-tools.yml";
const WRITER: &str = "release";
const PINNED: (&str, &str) = (
    "0.1.79",
    "a82f2edacbd781debf2da851d3b3380e81fce9fdec216c1101ca5ce91afe7386",
);
const ACTION_COMMIT: &str = "9ee3aedc9d3a9ebf90a212d960c9a5cfaae6c46b";

fn read() -> Result<String, Box<dyn Error>> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(WORKFLOW);
    Ok(fs::read_to_string(path)?)
}

fn jobs(text: &str) -> Vec<(String, Vec<&str>)> {
    let mut jobs: Vec<(String, Vec<&str>)> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line == "jobs:" {
            inside = true;
        } else if !inside {
        } else if let Some(id) = line
            .strip_prefix("  ")
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|rest| !rest.starts_with(' '))
        {
            jobs.push((id.to_owned(), Vec::new()));
        } else if let Some((_, lines)) = jobs.last_mut() {
            lines.push(line);
        }
    }
    jobs
}

fn values<'a>(lines: &[&'a str], key: &str) -> Vec<&'a str> {
    let prefix = format!("{key}: ");
    lines
        .iter()
        .filter_map(|line| {
            let line = line.trim_start();
            line.strip_prefix("- ")
                .unwrap_or(line)
                .strip_prefix(&prefix)
        })
        .collect()
}

fn steps(lines: &[&str]) -> Vec<RenderedStep> {
    let uses = values(lines, "uses")
        .into_iter()
        .map(|action| RenderedStep {
            uses: Some(action.to_owned()),
            ..RenderedStep::default()
        });
    let runs = values(lines, "run")
        .into_iter()
        .map(|command| RenderedStep {
            run: Some(command.to_owned()),
            ..RenderedStep::default()
        });
    uses.chain(runs).collect()
}

#[test]
fn every_step_of_the_release_workflow_passes_the_yaml_rules() -> TestResult {
    let text = read()?;
    let jobs = jobs(&text);
    assert!(jobs.len() >= 3, "{} jobs", jobs.len());
    let rendered = RenderedWorkflow {
        jobs: jobs
            .iter()
            .map(|(id, lines)| RenderedJob {
                id: id.clone(),
                runs_on: String::new(),
                timeout_minutes: 1,
                steps: steps(lines),
                ..RenderedJob::default()
            })
            .collect(),
    };
    assert!(rendered.jobs.iter().all(|job| !job.steps.is_empty()));
    assert_eq!(check_rules(&rendered), Ok(()));
    assert!(!text.contains("run: |"));
    assert!(!text.contains("run: >"));
    Ok(())
}

#[test]
fn the_first_job_gates_the_tag_with_a_pinned_release_and_the_build_needs_it() -> TestResult {
    let text = read()?;
    let jobs = jobs(&text);
    let (id, gate) = &jobs[0];
    assert_eq!(id, "gate");
    let (version, digest) = PINNED;
    let expected = format!(
        "    name: Gate
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    permissions:
      contents: read
      checks: read
    steps:
      - name: Install velnor-ci {version}
        uses: {SETUP_VELNOR_ACTION}@{ACTION_COMMIT}
        with:
          version: {version}
          sha256-linux-x64: {digest}
      - name: Check that the tagged commit is on the default branch and passed Required
        env:
          GITHUB_TOKEN: ${{{{ github.token }}}}
        run: velnor-ci release-gate --repo \"$GITHUB_REPOSITORY\" --sha \"$GITHUB_SHA\""
    );
    assert_eq!(gate.join("\n"), expected);
    assert_eq!(values(&jobs[1].1, "needs"), ["gate"]);
    for (id, lines) in &jobs {
        for line in lines {
            let key = line.trim_start().trim_start_matches("- ");
            assert!(
                !key.starts_with("if:") || key.contains("runner.os"),
                "{id}: {line}"
            );
            assert!(!key.starts_with("continue-on-error:"), "{id}: {line}");
        }
    }
    Ok(())
}

#[test]
fn only_the_last_job_can_write_and_it_checks_out_nothing_and_verifies_before_it_publishes()
-> TestResult {
    let text = read()?;
    assert!(text.lines().any(|line| line == "permissions: {}"));
    let jobs = jobs(&text);
    for (id, lines) in &jobs {
        let writes = values(lines, "contents")
            .iter()
            .filter(|access| **access == "write")
            .count();
        assert_eq!(writes, usize::from(id == WRITER), "{id}");
        assert_eq!(
            values(lines, "uses")
                .iter()
                .any(|action| action.starts_with("actions/checkout@")),
            id == "build",
            "{id}"
        );
        assert_eq!(
            values(lines, "run")
                .iter()
                .any(|run| run.starts_with("gh ")),
            id == WRITER,
            "{id}"
        );
    }
    let tag_checks: Vec<&str> = values(&jobs[1].1, "run")
        .into_iter()
        .filter(|run| run.ends_with(" check-tag \"$GITHUB_REF_NAME\""))
        .collect();
    assert_eq!(
        tag_checks,
        [
            "dist/velnor-ci-\"$SUFFIX\" check-tag \"$GITHUB_REF_NAME\"",
            "dist/velnor-gen-\"$SUFFIX\" check-tag \"$GITHUB_REF_NAME\""
        ]
    );
    assert_eq!(jobs[1].0, "build");
    assert_eq!(jobs.last().map(|(id, _)| id.as_str()), Some(WRITER));
    let writer = &jobs[jobs.len() - 1].1;
    let runs = values(writer, "run");
    let verify = runs
        .iter()
        .position(|run| run.starts_with("sha256sum --check"))
        .ok_or("the writing job never verifies SHA256SUMS")?;
    let publish = runs
        .iter()
        .position(|run| run.starts_with("gh release create"))
        .ok_or("the writing job never creates the release")?;
    assert!(verify < publish);
    assert!(runs[publish].contains(" --prerelease "));
    let published = runs[publish].matches(" dist/").count();
    assert_eq!(published, 8);
    assert!(runs[publish].contains(" dist/COMMIT "));
    assert!(
        !runs
            .iter()
            .any(|run| run.contains("dist/COMMIT >") || run.contains("> dist/COMMIT"))
    );
    let checksums = jobs
        .iter()
        .find(|(id, _)| id == "checksums")
        .ok_or("no checksums job")?;
    let checksum_runs = values(&checksums.1, "run");
    let record = checksum_runs
        .iter()
        .position(|run| *run == "echo \"$GITHUB_SHA\" > dist/COMMIT")
        .ok_or("the checksums job never records the commit of the release")?;
    let sums = checksum_runs
        .iter()
        .position(|run| run.starts_with("sha256sum ") && run.ends_with("> SHA256SUMS"))
        .ok_or("the checksums job never writes SHA256SUMS")?;
    assert!(record < sums);
    assert!(checksum_runs[sums].contains(" COMMIT "));
    Ok(())
}
