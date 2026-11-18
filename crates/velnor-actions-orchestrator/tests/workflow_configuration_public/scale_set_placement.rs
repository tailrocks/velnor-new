// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `execution.overrides` places the job image candidate, generator source
//! preview and cache prune jobs on the scale set. Without an override the
//! rendered workflows stay on the hosted label.

use std::collections::BTreeMap;
use std::error::Error;
use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use super::workflow_configuration_fixture;

const SCALE_RUNS_ON: &str = "runs-on: [velnor, ubuntu-26.04-scale-set]";
const CANDIDATE: &str = ".github/workflows/job-image-candidate.yml";
const GENERATOR: &str = ".github/workflows/product-release-generator.yml";
const PRUNE: &str = ".github/workflows/cache-prune.yml";
const CANDIDATE_JOBS: [&str; 2] = ["build-image-candidate", "attest-image-candidate"];
const PREVIEW_JOBS: [&str; 2] = ["source-preview-build-linux", "source-preview-qualify-linux"];

fn config() -> String {
    "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nworkflows = [\"generator_release\", \"macos_binary_release\", \"image_release\"]\n\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n".to_owned()
}

fn rendered(ids: &[&str]) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let overrides: String = ids
        .iter()
        .map(|id| {
            format!("\n[execution.overrides.{id}]\nprofile = \"local\"\nrole = \"control\"\n")
        })
        .collect();
    let repo = workflow_configuration_fixture::make_repo(&format!("{}{overrides}", config()))?;
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    Ok(render_staged_tree(&prepare(repo.path())?)?
        .files
        .into_iter()
        .map(|file| (file.path, file.bytes))
        .collect())
}

fn job_block<'a>(text: &'a str, id: &str) -> Result<&'a str, Box<dyn Error>> {
    let start = text
        .find(&format!("\n  {id}:\n"))
        .ok_or_else(|| format!("job {id} missing"))?;
    let rest = &text[start + 1..];
    let end = rest
        .match_indices("\n  ")
        .find(|(at, _)| rest[at + 3..].chars().next().is_some_and(|c| c != ' '))
        .map_or(rest.len(), |(at, _)| at);
    Ok(&rest[..end])
}

fn file<'a>(tree: &'a BTreeMap<String, String>, path: &str) -> Result<&'a str, Box<dyn Error>> {
    Ok(tree.get(path).ok_or_else(|| format!("{path} missing"))?)
}

#[test]
fn without_overrides_the_jobs_stay_hosted() -> Result<(), Box<dyn Error>> {
    let tree = rendered(&[])?;
    for id in CANDIDATE_JOBS {
        assert!(job_block(file(&tree, CANDIDATE)?, id)?.contains("runs-on: ubuntu-26.04"));
    }
    for id in PREVIEW_JOBS {
        assert!(job_block(file(&tree, GENERATOR)?, id)?.contains("runs-on: ubuntu-26.04"));
    }
    assert!(!file(&tree, CANDIDATE)?.contains(SCALE_RUNS_ON));
    assert!(!file(&tree, GENERATOR)?.contains(SCALE_RUNS_ON));
    Ok(())
}

#[test]
fn candidate_overrides_move_build_and_attest_but_not_publication() -> Result<(), Box<dyn Error>> {
    let tree = rendered(&CANDIDATE_JOBS)?;
    let text = file(&tree, CANDIDATE)?;
    for id in CANDIDATE_JOBS {
        assert!(job_block(text, id)?.contains(SCALE_RUNS_ON), "{id}");
    }
    let publish = job_block(text, "publish-job-image")?;
    assert!(publish.contains("runs-on: ubuntu-26.04"), "{publish}");
    assert!(
        text.contains("\"on\":\n  push: {}\n  workflow_dispatch: {}\n"),
        "the candidate workflow gained a trigger a fork could use"
    );
    assert!(job_block(text, "attest-image-candidate")?.contains("id-token: write"));
    Ok(())
}

#[test]
fn one_candidate_override_moves_only_its_job() -> Result<(), Box<dyn Error>> {
    let tree = rendered(&["build-image-candidate"])?;
    let text = file(&tree, CANDIDATE)?;
    assert!(job_block(text, "build-image-candidate")?.contains(SCALE_RUNS_ON));
    assert!(job_block(text, "attest-image-candidate")?.contains("runs-on: ubuntu-26.04"));
    Ok(())
}

#[test]
fn preview_overrides_move_the_linux_jobs_and_keep_the_push_to_main_gate()
-> Result<(), Box<dyn Error>> {
    let tree = rendered(&PREVIEW_JOBS)?;
    let text = file(&tree, GENERATOR)?;
    for id in PREVIEW_JOBS {
        let block = job_block(text, id)?;
        assert!(block.contains(SCALE_RUNS_ON), "{id}");
        assert!(block.contains("shell: bash -e {0}"), "{id}");
        assert!(
            block.contains("github.event_name == 'push' && github.repository == 'tailrocks/velnor-new' && github.ref == 'refs/heads/main'"),
            "{id} lost its push-to-main gate"
        );
    }
    for id in [
        "source-preview-build-darwin",
        "source-preview-qualify-darwin",
    ] {
        assert!(job_block(text, id)?.contains("runs-on: macos-15"), "{id}");
    }
    Ok(())
}

#[test]
fn a_macos_preview_job_is_not_a_valid_placement() -> Result<(), Box<dyn Error>> {
    let error = rendered(&["source-preview-build-darwin"])
        .err()
        .ok_or("a macOS preview job was accepted as a scale-set override")?;
    assert!(error.to_string().contains("unknown_logical_job"), "{error}");
    Ok(())
}

#[test]
fn prune_override_moves_the_prune_job_and_keeps_its_push_authority() -> Result<(), Box<dyn Error>> {
    let hosted = rendered(&[])?;
    let placed = rendered(&["prune"])?;
    let Some(prune) = placed.get(PRUNE) else {
        return Err("cache-prune.yml was not rendered".into());
    };
    assert!(
        job_block(prune, "prune")?.contains(SCALE_RUNS_ON),
        "{prune}"
    );
    assert!(prune.contains("shell: bash -e {0}"));
    assert!(
        prune.contains("github.event.workflow_run.head_repository.full_name == github.repository")
    );
    assert!(file(&hosted, PRUNE)?.contains("runs-on: ubuntu-26.04"));
    Ok(())
}
