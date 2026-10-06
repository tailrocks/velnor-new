//! G4 end to end: generated CI carries per-kind timeouts on every job.
use std::collections::BTreeMap;
use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// Per-job `timeout-minutes` parsed from one rendered workflow.
fn job_timeouts(yaml: &str) -> BTreeMap<String, Vec<String>> {
    let mut jobs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    let mut in_jobs = false;
    for line in yaml.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        if let Some(id) = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && rest.ends_with(':') && !rest.contains(' '))
        {
            current = Some(id.trim_end_matches(':').to_owned());
            continue;
        }
        if let (Some(id), Some(value)) =
            (current.as_ref(), line.strip_prefix("    timeout-minutes: "))
        {
            jobs.entry(id.clone()).or_default().push(value.to_owned());
        }
    }
    jobs
}

#[test]
fn generated_ci_bounds_every_job_per_kind() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    let yaml = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
    let timeouts = job_timeouts(&yaml);
    for (id, minutes) in &timeouts {
        assert_eq!(minutes.len(), 1, "job {id} must carry exactly one timeout");
    }
    for (id, want) in [
        ("plan", "20"),
        ("required", "10"),
        ("actionlint", "10"),
        ("rust-demo", "30"),
        ("publish-baseline", "10"),
    ] {
        assert_eq!(
            timeouts.get(id).map(Vec::as_slice),
            Some([want.to_owned()].as_slice()),
            "job {id} carries its per-kind bound"
        );
    }
    assert_eq!(timeouts.len(), 5, "every job must be bounded: {timeouts:?}");
    Ok(())
}
