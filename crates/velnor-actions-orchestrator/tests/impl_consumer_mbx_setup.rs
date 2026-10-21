//! Consumer Rust setup: the `mr_boxington = true` Mise option alone selects
//! the MBX compile driver, the seed-only bundle writer, and the opaque
//! bundle transport (consumer Rust setup docs, WP-B1).
//!
//! The fixture is the documented consumer layout: one `mise.toml` with the
//! Rust pin, the minimal profile, clippy and rustfmt, `mr_boxington = true`,
//! an exact `mr-boxington` pin, and Nextest. `.velnor/config.toml` declares
//! no `[stacks.rust] compile_driver`; the Mise option is durable evidence.

use std::fs;

use velnor_actions_mise::catalog::{MR_BOXINGTON_VERSION, NEXTEST_VERSION, RUST_VERSION};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use crate::impl_common::{TestResult, config_with_branch, make_repo, write_nextest_task};

/// The recommended consumer `mise.toml` (docs/implemented/consumer-rust-setup.md).
fn consumer_mise_toml() -> String {
    format!(
        "[settings]\nminimum_release_age = \"0s\"\n\n[tools]\nrust = {{ version = \"{RUST_VERSION}\", profile = \"minimal\", components = [\"clippy\", \"rustfmt\"], mr_boxington = true }}\nmr-boxington = \"{MR_BOXINGTON_VERSION}\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"{NEXTEST_VERSION}\"\n"
    )
}

/// Split rendered `ci.yml` into `(job id, job text)` blocks.
fn jobs(yaml: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
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
            .filter(|rest| !rest.starts_with(' '))
            .and_then(|rest| rest.strip_suffix(':'))
        {
            out.push((id.to_owned(), String::new()));
        } else if let Some((_, text)) = out.last_mut() {
            text.push_str(line);
            text.push('\n');
        }
    }
    out
}

fn step_count(job: &str, name: &str) -> usize {
    job.lines()
        .filter(|line| line.trim_start().trim_start_matches("- ") == format!("name: {name}"))
        .count()
}

#[test]
fn mr_boxington_option_selects_mbx_seed_writer_and_opaque_bundles() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("mise.toml"), consumer_mise_toml())?;
    write_nextest_task(root)?;
    let prep = prepare(root)?;

    assert_eq!(prep.discovery.workspaces.len(), 1);
    let workspace = &prep.discovery.workspaces[0];
    assert_eq!(
        workspace.profile.compile_driver.as_str(),
        "mbx",
        "mr_boxington = true is durable evidence for the MBX driver"
    );
    assert!(
        workspace.findings.is_empty(),
        "no transient-evidence finding: {:?}",
        workspace.findings
    );
    assert!(prep.catalog.rust_mr_boxington());

    let tree = render_staged_tree(&prep)?;
    let yaml = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    assert!(
        !yaml.contains("github-cache-mode"),
        "the MBX action is install-only; no target-dir cache: {yaml}"
    );
    let jobs = jobs(yaml);
    let seed: Vec<_> = jobs
        .iter()
        .filter(|(_, job)| job.contains("name: Dependency seed"))
        .collect();
    assert_eq!(seed.len(), 1, "one dependency-seed job: {yaml}");
    let (seed_id, seed_job) = seed[0];
    for name in [
        "Compute MBX bundle key",
        "Restore MBX bundle",
        "Import MBX bundle",
        "Record MBX producer success",
        "Export MBX bundle",
        "Save MBX bundle",
    ] {
        assert_eq!(step_count(seed_job, name), 1, "{seed_id} runs {name}");
    }
    assert!(
        seed_job.contains("cache export --group") && seed_job.contains("--format directory"),
        "bundles are the opaque `mbx cache export --format directory` closure: {seed_job}"
    );

    let crate_jobs: Vec<_> = jobs
        .iter()
        .filter(|(id, _)| id.starts_with("rust-"))
        .collect();
    assert!(!crate_jobs.is_empty(), "a crate job exists: {yaml}");
    for (id, job) in crate_jobs {
        assert_eq!(step_count(job, "Restore MBX bundle"), 1, "{id} restores");
        assert_eq!(step_count(job, "Import MBX bundle"), 1, "{id} imports");
        assert!(
            job.contains("cache import"),
            "{id} restores only via `mbx cache import`"
        );
        for writer in ["Export MBX bundle", "Save MBX bundle"] {
            assert_eq!(
                step_count(job, writer),
                0,
                "{id} is read-only; only the seed job writes bundles"
            );
        }
    }
    Ok(())
}

#[test]
fn mr_boxington_consumer_generates_without_a_declared_driver() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    fs::write(root.join("mise.toml"), consumer_mise_toml())?;
    write_nextest_task(root)?;
    let prep = prepare(root)?;
    let parent = tempfile::TempDir::new()?;
    let out = parent.path().join("out");
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(out.clone()),
        },
    )?;
    let ci = fs::read_to_string(out.join(".github/workflows/ci.yml"))?;
    assert!(ci.contains("Import MBX bundle"), "{ci}");
    Ok(())
}
