// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::source_qualification_sources_support::{
    PREFETCH_PHASE, PREPARE_PHASE, RUNNER_TEMP_EXPR, gather_ids, only, output_refs, positions,
    producer_ids, render, runs_phase, sources_repo, steps_of, text, uses_prefix,
};

#[test]
fn every_harness_job_restores_the_sources_family_read_only_before_prefetch()
-> Result<(), Box<dyn Error>> {
    let repo = sources_repo()?;
    let sq = render(repo.path())?.sq;
    let producers = producer_ids(&sq);
    assert!(
        producers.len() >= 2,
        "expected parallel producers: {producers:?}"
    );
    for job in &producers {
        let steps = steps_of(&sq, job)?;
        let checkout = only(
            &positions(steps, |s| uses_prefix(s, "actions/checkout@")),
            "checkout",
            job,
        )?;
        let keys = only(
            &positions(steps, |s| s["name"] == "Compute cache keys"),
            "Compute cache keys step",
            job,
        )?;
        assert_eq!(steps[keys]["id"], "cache-keys", "job {job}");
        let restore = only(
            &positions(steps, |s| uses_prefix(s, "actions/cache/restore@")),
            "cache restore",
            job,
        )?;
        let cache_steps = positions(steps, |s| uses_prefix(s, "actions/cache"));
        assert_eq!(cache_steps, [restore], "job {job} touches the cache twice");
        assert_eq!(steps[restore]["id"], "sources-restore", "job {job}");
        assert_eq!(
            steps[restore]["if"], "github.event_name != 'workflow_dispatch'",
            "job {job} restore condition"
        );
        let prepare = only(
            &positions(steps, |s| runs_phase(s, PREPARE_PHASE)),
            "prepare phase",
            job,
        )?;
        let prefetch = only(
            &positions(steps, |s| runs_phase(s, PREFETCH_PHASE)),
            "prefetch phase",
            job,
        )?;
        assert!(
            checkout < keys && keys < restore && restore < prefetch && prepare < prefetch,
            "job {job} order: checkout {checkout}, prepare {prepare}, keys {keys}, restore {restore}, prefetch {prefetch}"
        );
        let with = &steps[restore]["with"];
        assert_eq!(
            output_refs(text(&with["key"], "restore key")?),
            [("cache-keys".to_owned(), "sources_key".to_owned())],
            "job {job} restore key"
        );
        assert_eq!(
            output_refs(text(&with["restore-keys"], "restore-keys")?),
            [("cache-keys".to_owned(), "sources_restore_keys".to_owned())],
            "job {job} restore-keys"
        );
    }
    for job in gather_ids(&sq) {
        let steps = steps_of(&sq, &job)?;
        assert!(
            positions(steps, |s| uses_prefix(s, "actions/cache")).is_empty(),
            "gather job {job} touches the cache"
        );
    }
    Ok(())
}

#[test]
fn prefetch_reads_only_outputs_of_earlier_steps() -> Result<(), Box<dyn Error>> {
    let repo = sources_repo()?;
    let sq = render(repo.path())?.sq;
    for job in producer_ids(&sq) {
        let steps = steps_of(&sq, &job)?;
        let prefetch = only(
            &positions(steps, |s| runs_phase(s, PREFETCH_PHASE)),
            "prefetch phase",
            &job,
        )?;
        let env = steps[prefetch]["env"]
            .as_object()
            .ok_or("prefetch has no env")?;
        let mut referenced = BTreeSet::new();
        for (name, value) in env {
            let refs = output_refs(text(value, name)?);
            assert!(
                !refs.is_empty(),
                "job {job} env {name} reads no step output"
            );
            for (id, _) in refs {
                assert!(
                    steps[..prefetch].iter().any(|s| s["id"] == id.as_str()),
                    "job {job} env {name} reads steps.{id}, which no earlier step defines"
                );
                referenced.insert(id);
            }
        }
        assert_eq!(referenced, BTreeSet::from(["sources-restore".to_owned()]));
    }
    Ok(())
}

fn resolved_cargo_home(sq: &Value, job: &str, runner_temp: &str) -> Result<String, Box<dyn Error>> {
    let steps = steps_of(sq, job)?;
    let prepare = only(
        &positions(steps, |s| runs_phase(s, PREPARE_PHASE)),
        "prepare phase",
        job,
    )?;
    Ok(
        text(&steps[prepare]["env"]["CARGO_HOME"], "prepare CARGO_HOME")?
            .replace(RUNNER_TEMP_EXPR, runner_temp),
    )
}

#[test]
fn cargo_home_is_where_the_sources_cache_lands() -> Result<(), Box<dyn Error>> {
    let repo = sources_repo()?;
    let sq = render(repo.path())?.sq;
    for job in producer_ids(&sq) {
        let home = resolved_cargo_home(&sq, &job, RUNNER_TEMP_EXPR)?;
        let steps = steps_of(&sq, &job)?;
        let restore = only(
            &positions(steps, |s| uses_prefix(s, "actions/cache/restore@")),
            "cache restore",
            &job,
        )?;
        let paths: Vec<&str> = text(&steps[restore]["with"]["path"], "restore path")?
            .lines()
            .collect();
        assert_eq!(paths.len(), 3, "job {job}: {paths:?}");
        for path in paths {
            assert!(
                path.starts_with(&format!("{home}/")),
                "job {job}: {path} is not under CARGO_HOME {home}"
            );
        }
    }
    Ok(())
}

#[test]
fn the_prepare_phase_exports_the_generator_cargo_home_to_every_later_step()
-> Result<(), Box<dyn Error>> {
    let repo = sources_repo()?;
    let sq = render(repo.path())?.sq;
    let job = producer_ids(&sq)
        .into_iter()
        .next()
        .ok_or("no producer job")?;
    let scratch = tempfile::tempdir()?;
    let base = fs::canonicalize(scratch.path())?;
    let runner_temp = base.join("runner-temp");
    let workspace = base.join("workspace");
    fs::create_dir_all(&runner_temp)?;
    fs::create_dir_all(&workspace)?;
    let github_env = base.join("github-env");
    fs::write(&github_env, "")?;
    let home = resolved_cargo_home(&sq, &job, &runner_temp.display().to_string())?;
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification.sh");
    let output = Command::new("bash")
        .arg(script)
        .arg(PREPARE_PHASE)
        .current_dir(&workspace)
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_ENV", &github_env)
        .env("CARGO_HOME", &home)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = format!("CARGO_HOME={home}");
    let exported = fs::read_to_string(&github_env)?;
    assert!(
        exported.lines().any(|line| line == expected),
        "GITHUB_ENV lacks {expected}: {exported}"
    );
    let recorded = fs::read_to_string(
        runner_temp.join("source-qualification-artifacts/build-environment.txt"),
    )?;
    assert!(
        recorded.lines().any(|line| line == expected),
        "build-environment.txt lacks {expected}: {recorded}"
    );
    Ok(())
}
