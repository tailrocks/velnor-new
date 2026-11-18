// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_render::{
    RenderedJob, RenderedStep, RenderedWorkflow, SETUP_VELNOR_ACTION, check_rules,
};

type TestResult = Result<(), Box<dyn Error>>;

const WORKFLOW: &str = ".github/workflows/release-rust-ci-tools.yml";
const WRITER: &str = "release";
const TAG_PUSH: &str =
    "    if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/rust-ci-v')";
const PINNED: (&str, &str) = (
    "0.1.81",
    "3d348312a7fe47668b5ee54a33683f1cc257479b44198a39d286fd3543836113",
);
const ACTION_COMMIT: &str = "a87c35aed5b68dd3e82b36df3e8d7364311abba8";

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

fn release_jobs(text: &str) -> BTreeMap<String, Vec<&str>> {
    let jobs = jobs(text);
    let mut ids: Vec<&str> = jobs.iter().map(|(id, _)| id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        ["build", "checksums", "gate", "release", "tag-ruleset-drift"]
    );
    jobs.into_iter()
        .filter(|(id, _)| id != "tag-ruleset-drift")
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
    let jobs = release_jobs(&text);
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
    for lines in jobs.values() {
        assert!(
            values(lines, "run")
                .iter()
                .all(|run| !run.starts_with(['|', '>']))
        );
    }
    Ok(())
}

#[test]
fn the_release_dag_gates_the_tag_with_a_pinned_release_before_building() -> TestResult {
    let text = read()?;
    let jobs = release_jobs(&text);
    let gate = &jobs["gate"];
    let (version, digest) = PINNED;
    let expected = format!(
        "    name: Gate
{TAG_PUSH}
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
    assert_eq!(values(gate, "needs"), [] as [&str; 0]);
    for (id, dependency) in [
        ("build", "gate"),
        ("checksums", "build"),
        (WRITER, "checksums"),
    ] {
        assert_eq!(values(&jobs[id], "needs"), [dependency], "{id}");
    }
    for (id, lines) in &jobs {
        for line in lines {
            let key = line.trim_start().trim_start_matches("- ");
            assert!(
                !key.starts_with("if:")
                    || (id == "gate" && *line == TAG_PUSH)
                    || (line.starts_with("        if:") && key.contains("runner.os")),
                "{id}: {line}"
            );
            assert!(!key.starts_with("continue-on-error:"), "{id}: {line}");
        }
    }
    Ok(())
}

#[test]
fn only_the_release_job_can_write_and_it_checks_out_nothing_and_verifies_before_it_publishes()
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
    let release_jobs = release_jobs(&text);
    let tag_checks: Vec<&str> = values(&release_jobs["build"], "run")
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
    let writer = &release_jobs[WRITER];
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
    let checksum_runs = values(&release_jobs["checksums"], "run");
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
