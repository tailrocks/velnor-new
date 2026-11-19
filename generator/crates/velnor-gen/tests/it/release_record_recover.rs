// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RELEASE_WORKFLOW_PATH;
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, Root, release_view, steps};
use crate::release_yml_binaries::job;
use crate::yaml_tree::{Node, parse};

type TestResult = Result<(), Box<dyn Error>>;

const ONE_ROOT: &str = "roots = [\".\"]\n";

fn tag_job(helper: &str, roots: &[Root], config: &str) -> Result<Node, Box<dyn Error>> {
    let mut view = release_view(roots, Some(config))?;
    view.files
        .insert(LOCK_PATH, LOCK.replace("0.1.79", helper).into_bytes());
    let text = expected_files(&view)?
        .remove(&RELEASE_WORKFLOW_PATH)
        .ok_or("no release.yml")?;
    let tree = parse(&text)?;
    Ok(job(&tree, "tag")?.clone())
}

fn record(tag: &Node) -> Result<&Node, Box<dyn Error>> {
    steps(tag)
        .iter()
        .find(|step| step.get("id").and_then(Node::text) == Some("record"))
        .ok_or_else(|| "no record step".into())
}

fn names(tag: &Node) -> Vec<&str> {
    steps(tag)
        .iter()
        .filter_map(|step| step.get("name").and_then(Node::text))
        .collect()
}

#[test]
fn the_record_step_runs_after_the_tag_steps_and_names_the_config_so_a_rerun_recovers_the_drafts_release_plz_skipped()
-> TestResult {
    let tag = tag_job("0.1.92", &[APP], ONE_ROOT)?;
    let run = record(&tag)?
        .get("run")
        .and_then(Node::text)
        .ok_or("no run")?;
    assert!(
        run.contains(
            " --app=velnor-release --config=release-plz.toml "
        ),
        "{run}"
    );
    let all = names(&tag);
    let last_root = all
        .iter()
        .rposition(|name| name.starts_with("Tag the releases of"))
        .ok_or("no tag step")?;
    let recorded = all
        .iter()
        .position(|name| name.starts_with("Record the released packages"))
        .ok_or("no record step")?;
    assert_eq!(recorded, last_root + 1);
    Ok(())
}

#[test]
fn the_record_step_reads_with_the_job_token_and_the_pinned_toolchain_never_the_app_token()
-> TestResult {
    let tag = tag_job("0.1.92", &[APP], ONE_ROOT)?;
    let step = record(&tag)?;
    assert_eq!(
        step.at(&["env", "GITHUB_TOKEN"]).and_then(Node::text),
        Some("${{ github.token }}")
    );
    assert_eq!(
        step.at(&["env", "RUSTUP_TOOLCHAIN"]).and_then(Node::text),
        Some("stable")
    );
    let env = step.get("env").ok_or("no env")?;
    for key in env.keys() {
        let value = env.get(key).and_then(Node::text).unwrap_or_default();
        assert!(!value.contains("steps.app"), "{key}={value}");
    }
    Ok(())
}

#[test]
fn a_helper_without_the_recovery_flags_records_only_what_release_plz_printed() -> TestResult {
    let tag = tag_job("0.1.91", &[APP], ONE_ROOT)?;
    let run = record(&tag)?
        .get("run")
        .and_then(Node::text)
        .ok_or("no run")?;
    assert!(!run.contains("--app"), "{run}");
    assert!(!run.contains("--config"), "{run}");
    Ok(())
}
