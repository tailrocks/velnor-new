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

const FROM_SOURCE: &str = "[[lint]]\nkind = 'generated-check'\nfrom_source = true\n";
const ID: &str = "lint-generated-check";
const CHECK_FROM_SOURCE: &str = "cargo run --locked --manifest-path Cargo.toml --target \"$VELNOR_GEN_TARGET\" --package velnor-gen -- check --verify-release";
const NESTED: &str = "tools/gen/Cargo.toml";
const FIRST: &str = "0.1.111";
const STORE: &str = "MBX_CACHE_DIR=$RUNNER_TEMP/mbx";
const MIRROR: &str = "[crates_mirror]\nrunner = 'mirror-box'\nregistry = 'sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/'\ncainfo = '/opt/velnor/crates-mirror/ca-bundle.pem'\n";

fn locked(config: &str, version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    generators(config, version, &["Cargo.toml"], &["Cargo.toml"])
}

fn generators(
    config: &str,
    version: &str,
    roots: &[&str],
    generator_roots: &[&str],
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = view(roots, Some(config.as_bytes()))?;
    view.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    view.tracked_paths.insert(LOCK_PATH);
    view.files
        .insert(LOCK_PATH, archive_lock_at(version)?.into_bytes());
    for root in generator_roots {
        let metadata = view
            .rust_metadata
            .get_mut(&RepoPath::new(*root)?)
            .ok_or("no metadata for the generator root")?;
        for member in &mut metadata.members {
            member.name = PackageName::new("velnor-gen")?;
        }
    }
    Ok(view)
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
    assert_eq!(
        runs(&job).last().map(String::as_str),
        Some(CHECK_FROM_SOURCE)
    );
    assert!(
        !names(&job)
            .iter()
            .any(|name| name.starts_with("Install velnor-gen")),
        "{job}"
    );
    assert!(!job.to_string().contains("VELNOR_GEN_ARCHIVE"), "{job}");
    let text = job.to_string();
    assert!(text.contains("mr-boxington"), "{job}");
    assert!(text.contains("x86_64-unknown-linux-gnu"), "{job}");
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
        check["env"]
            .as_object()
            .is_some_and(|env| env.keys().all(|key| key == "VELNOR_GEN_TARGET")),
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
    let error = workflow(&locked(&format!("{LINT}\n{FROM_SOURCE}"), "0.1.110")?).err();
    let text = format!("{error:?}");
    assert!(text.contains("ConfigKeyHelperTooOld"), "{text}");
    assert!(text.contains("lint.from_source"), "{text}");
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
    let cargo = position(&job, |step| run_contains(step, CHECK_FROM_SOURCE)).ok_or("no check")?;
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
    let cargo = position(&job, |step| run_contains(step, CHECK_FROM_SOURCE)).ok_or("no check")?;
    assert!(helper < mirror && mirror < tools && tools < cargo, "{job}");
    Ok(())
}

#[test]
fn from_source_builds_the_generator_from_the_root_that_holds_its_package() -> TestResult {
    let view = generators(
        &format!("{LINT}\n{FROM_SOURCE}"),
        FIRST,
        &["Cargo.toml", NESTED],
        &[NESTED],
    )?;
    let job = job(&workflow(&view)?)?;
    assert_eq!(
        runs(&job).last().map(String::as_str),
        Some(
            "cargo run --locked --manifest-path tools/gen/Cargo.toml --target \"$VELNOR_GEN_TARGET\" --package velnor-gen -- check --verify-release"
        )
    );
    Ok(())
}

#[test]
fn from_source_refuses_a_repository_without_a_velnor_gen_package() -> TestResult {
    let view = generators(
        &format!("{LINT}\n{FROM_SOURCE}"),
        FIRST,
        &["Cargo.toml"],
        &[],
    )?;
    let error = workflow(&view).err();
    let text = format!("{error:?}");
    assert!(text.contains("GeneratorPackage"), "{text}");
    assert!(text.contains("found: 0"), "{text}");
    Ok(())
}

#[test]
fn from_source_refuses_two_roots_that_both_hold_a_velnor_gen_package() -> TestResult {
    let view = generators(
        &format!("{LINT}\n{FROM_SOURCE}"),
        FIRST,
        &["Cargo.toml", NESTED],
        &["Cargo.toml", NESTED],
    )?;
    let error = workflow(&view).err();
    let text = format!("{error:?}");
    assert!(text.contains("GeneratorPackage"), "{text}");
    assert!(text.contains("found: 2"), "{text}");
    Ok(())
}
