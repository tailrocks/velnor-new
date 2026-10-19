//! G4 end to end: generated CI carries bounded timeouts on every job.
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

/// Generate a preview tree from `config` and return per-job CI timeouts.
fn generated_timeouts(
    config: &str,
) -> Result<BTreeMap<String, Vec<String>>, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
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
    Ok(job_timeouts(&yaml))
}

#[test]
fn generated_ci_bounds_every_job_per_kind() -> TestResult {
    let timeouts = generated_timeouts(config_with_branch())?;
    for (id, minutes) in &timeouts {
        assert_eq!(minutes.len(), 1, "job {id} must carry exactly one timeout");
    }
    for (id, want) in [
        ("plan", "5"),
        ("required", "5"),
        ("required-gate", "5"),
        ("actionlint", "5"),
        ("rust-demo", "5"),
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

#[test]
fn configured_crate_timeout_reaches_every_crate_job_only() -> TestResult {
    let config = format!("{}crate_timeout_minutes = 45\n", config_with_branch());
    let timeouts = generated_timeouts(&config)?;
    for (id, want) in [
        ("plan", "5"),
        ("required", "5"),
        ("actionlint", "5"),
        ("rust-demo", "45"),
    ] {
        assert_eq!(
            timeouts.get(id).map(Vec::as_slice),
            Some([want.to_owned()].as_slice()),
            "job {id} carries its configured bound"
        );
    }
    assert_eq!(timeouts.len(), 5, "every job must be bounded: {timeouts:?}");
    Ok(())
}

#[test]
fn out_of_range_crate_timeout_fails_before_generation() -> TestResult {
    for minutes in ["0", "361"] {
        let config = format!(
            "{}crate_timeout_minutes = {minutes}\n",
            config_with_branch()
        );
        let repo = make_repo(&config)?;
        let error = prepare(repo.path()).expect_err("out-of-range crate timeout must fail");
        let text = error.to_string();
        assert!(text.contains("workflow.crate_timeout_minutes"), "{text}");
        assert!(
            text.contains(&format!("bad_timeout:{minutes}:expected_1..=360")),
            "{text}"
        );
    }
    Ok(())
}
