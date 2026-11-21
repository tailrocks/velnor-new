// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_model::{PackageName, RepoPath, RepoView};
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::generated_check::CHECK;
use crate::lint_lane::{LINT, MISE, runs};
use crate::lock_archive::archive_lock_at;
use crate::pipeline::{view, workflow};
use crate::private_sources::{SECTION, private_repo};

const FROM_SOURCE: &str = "[[lint]]\nkind = 'generated-check'\nfrom_source = true\n";
const ID: &str = "lint-generated-check";
const RUN_FROM_SOURCE: &str = "eval \"$VELNOR_GEN_CHECK\"";
const FIRST: &str = "0.1.112";
const STORE: &str = "MBX_CACHE_DIR=$RUNNER_TEMP/mbx";
const MIRROR: &str = "[crates_mirror]\nrunner = 'mirror-box'\nregistry = 'sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/'\ncainfo = '/opt/velnor/crates-mirror/ca-bundle.pem'\n";

fn packages(
    config: &str,
    version: &str,
    roots: &[(&str, &str)],
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let manifests: Vec<&str> = roots.iter().map(|(manifest, _)| *manifest).collect();
    let mut repo = view(&manifests, Some(config.as_bytes()))?;
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    for (manifest, name) in roots {
        let metadata = repo
            .rust_metadata
            .get_mut(&RepoPath::new(*manifest)?)
            .ok_or("no such root")?;
        metadata.members[0].name = PackageName::new(*name)?;
    }
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files
        .insert(LOCK_PATH, archive_lock_at(version)?.into_bytes());
    Ok(repo)
}

fn locked(config: &str, version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    packages(config, version, &[("Cargo.toml", "velnor-gen")])
}

fn rendered(config: &str, version: &str) -> Result<String, Box<dyn std::error::Error>> {
    workflow(&locked(config, version)?)
}

fn job(text: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(text)?["jobs"][ID].clone())
}

fn position(job: &Value, predicate: impl Fn(&Value) -> bool) -> Option<usize> {
    job["steps"].as_array()?.iter().position(predicate)
}

fn run_contains(step: &Value, text: &str) -> bool {
    step["run"].as_str().is_some_and(|run| run.contains(text))
}

fn check_script(job: &Value) -> String {
    job["steps"]
        .as_array()
        .and_then(|steps| steps.last())
        .and_then(|step| step["env"]["VELNOR_GEN_CHECK"].as_str())
        .unwrap_or_default()
        .to_owned()
}

fn names(job: &Value) -> Vec<String> {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|step| step["name"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn from_source_builds_velnor_gen_from_the_checkout_instead_of_installing_the_release() -> TestResult
{
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    assert_eq!(runs(&job).last().map(String::as_str), Some(RUN_FROM_SOURCE));
    assert!(
        !names(&job)
            .iter()
            .any(|name| name.starts_with("Install velnor-gen")),
        "{job}"
    );
    assert!(!job.to_string().contains("VELNOR_GEN_ARCHIVE"), "{job}");
    assert!(check_script(&job).contains("--package velnor-gen"), "{job}");
    Ok(())
}

#[test]
fn from_source_keeps_the_job_free_of_secrets_and_write_access() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    assert_eq!(job["permissions"], serde_json::json!({"contents": "read"}));
    assert!(!job.to_string().contains("secrets"), "{job}");
    assert!(!job.to_string().contains("GITHUB_TOKEN"), "{job}");
    let steps = job["steps"].as_array().ok_or("no steps")?;
    assert_eq!(steps[0]["with"]["persist-credentials"], false);
    let check = steps.last().ok_or("no steps")?;
    assert!(
        check["env"].as_object().is_some_and(|env| env
            .keys()
            .all(|key| key == "VELNOR_GEN_CHECK" || key == "VELNOR_GEN_MANIFEST")),
        "{check}"
    );
    assert!(
        job["needs"].is_null() && job["environment"].is_null(),
        "{job}"
    );
    Ok(())
}

#[test]
fn from_source_false_renders_the_release_install_byte_for_byte() -> TestResult {
    let off = "[[lint]]\nkind = 'generated-check'\nfrom_source = false\n";
    assert_eq!(
        rendered(&format!("{LINT}\n{off}"), FIRST)?,
        rendered(&format!("{LINT}\n{CHECK}"), FIRST)?
    );
    Ok(())
}

#[test]
fn a_helper_older_than_from_source_is_refused_with_the_gate_error() -> TestResult {
    for older in ["0.1.110", "0.1.111"] {
        let error = workflow(&locked(&format!("{LINT}\n{FROM_SOURCE}"), older)?).err();
        let text = format!("{error:?}");
        assert!(text.contains("ConfigKeyHelperTooOld"), "{older}: {text}");
        assert!(text.contains("lint.from_source"), "{older}: {text}");
    }
    assert!(workflow(&locked(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?).is_ok());
    Ok(())
}

#[test]
fn a_release_without_the_key_still_renders_the_plain_check_unchanged() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{CHECK}"), "0.1.110")?)?;
    assert_eq!(
        runs(&job).last().map(String::as_str),
        Some("velnor-gen check --verify-release")
    );
    assert!(
        names(&job)
            .iter()
            .any(|name| name.starts_with("Install velnor-gen")),
        "{job}"
    );
    Ok(())
}

#[test]
fn from_source_is_refused_on_every_other_lint_kind() -> TestResult {
    for kind in [
        "kind = 'jactionlint'",
        "kind = 'alint'\nconfig = '.alint.yml'",
        "kind = 'mise-task'\ntask = 'check'",
    ] {
        let config = format!("[[lint]]\n{kind}\nfrom_source = true\n");
        let error = workflow(&locked(&config, FIRST)?).err();
        let text = format!("{error:?}");
        assert!(text.contains("LintKeyForeign"), "{kind}: {text}");
        assert!(text.contains("from_source"), "{kind}: {text}");
        assert!(!text.contains("HelperTooOld"), "{kind}: {text}");
    }
    Ok(())
}

#[test]
fn from_source_gives_the_job_a_private_mbx_store_before_the_tools_install() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let store = position(&job, |step| run_contains(step, STORE)).ok_or("no MBX store")?;
    let tools = position(&job, |step| {
        step["uses"]
            .as_str()
            .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
    })
    .ok_or("no tools step")?;
    let cargo = position(&job, |step| run_contains(step, RUN_FROM_SOURCE)).ok_or("no check")?;
    assert!(store < tools && tools < cargo, "{job}");
    Ok(())
}

#[test]
fn from_source_configures_the_crates_mirror_after_velnor_ci_and_before_the_tools() -> TestResult {
    let config = format!("runner = 'mirror-box'\n{LINT}\n{FROM_SOURCE}\n{MIRROR}");
    let job = job(&rendered(&config, FIRST)?)?;
    let helper = position(&job, |step| {
        step["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("Install velnor-ci"))
    })
    .ok_or("no velnor-ci install")?;
    let mirror = position(&job, |step| run_contains(step, "crates-mirror")).ok_or("no mirror")?;
    let tools = position(&job, |step| {
        step["uses"]
            .as_str()
            .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
    })
    .ok_or("no tools step")?;
    let cargo = position(&job, |step| run_contains(step, RUN_FROM_SOURCE)).ok_or("no check")?;
    assert!(helper < mirror && mirror < tools && tools < cargo, "{job}");
    Ok(())
}

#[test]
fn from_source_installs_every_pinned_tool_the_build_needs() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let tools = job["steps"]
        .as_array()
        .ok_or("no steps")?
        .iter()
        .find(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("jdx/mise-action@"))
        })
        .ok_or("no tools step")?;
    assert!(tools["with"]["install_args"].is_null(), "{job}");
    Ok(())
}

#[test]
fn from_source_requires_a_rust_pin() -> TestResult {
    let mut repo = locked(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?;
    let without_rust = MISE.replace("rust = { version = \"1.99.1\", mr_boxington = true }\n", "");
    assert_ne!(without_rust, MISE);
    repo.files
        .insert(RepoPath::new("mise.toml")?, without_rust.into_bytes());
    let error = workflow(&repo).err();
    let text = format!("{error:?}");
    assert!(text.contains("ToolUnpinned"), "{text}");
    assert!(text.contains("rust"), "{text}");
    Ok(())
}

#[test]
fn the_source_build_takes_its_host_triple_from_rustc_on_the_runner() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    assert!(check_script(&job).contains("rustc -vV"), "{job}");
    let text = job.to_string();
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "aarch64-apple-darwin",
    ] {
        assert!(!text.contains(triple), "{triple}: {job}");
    }
    Ok(())
}

#[test]
fn from_source_builds_the_generator_from_the_workspace_that_holds_it() -> TestResult {
    let roots = [
        ("Cargo.toml", "velnor-ci-app"),
        ("tools/gen/Cargo.toml", "velnor-gen"),
    ];
    let repo = packages(&format!("{LINT}\n{FROM_SOURCE}"), FIRST, &roots)?;
    let job = job(&workflow(&repo)?)?;
    let step = job["steps"]
        .as_array()
        .and_then(|steps| steps.last())
        .ok_or("no steps")?;
    assert_eq!(
        step["env"]["VELNOR_GEN_MANIFEST"], "tools/gen/Cargo.toml",
        "{job}"
    );
    Ok(())
}

#[test]
fn from_source_refuses_a_repository_without_exactly_one_velnor_gen() -> TestResult {
    for roots in [
        vec![("Cargo.toml", "velnor-ci-app")],
        vec![
            ("Cargo.toml", "velnor-gen"),
            ("tools/gen/Cargo.toml", "velnor-gen"),
        ],
    ] {
        let repo = packages(&format!("{LINT}\n{FROM_SOURCE}"), FIRST, &roots)?;
        let error = workflow(&repo).err();
        let text = format!("{error:?}");
        assert!(text.contains("GeneratorPackage"), "{roots:?}: {text}");
    }
    Ok(())
}

#[test]
fn from_source_fetches_private_dependencies_before_it_builds_the_generator() -> TestResult {
    let mut repo = private_repo(&["Cargo.toml"], &format!("{SECTION}{FROM_SOURCE}"), FIRST)?;
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files
        .insert(LOCK_PATH, archive_lock_at(FIRST)?.into_bytes());
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    repo.rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no root")?
        .members[0]
        .name = PackageName::new("velnor-gen")?;
    let job = job(&workflow(&repo)?)?;
    let token = position(&job, |step| step["id"] == "private-token").ok_or("no token")?;
    let fetch =
        position(&job, |step| run_contains(step, "\"$VELNOR_EXEC\" fetch ")).ok_or("no fetch")?;
    let check = position(&job, |step| run_contains(step, "VELNOR_GEN_CHECK")).ok_or("no check")?;
    assert!(token < fetch && fetch < check, "{job}");
    Ok(())
}
