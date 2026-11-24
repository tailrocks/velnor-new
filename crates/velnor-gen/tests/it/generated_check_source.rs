// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{PackageName, RepoPath, RepoView, Target, TargetKind, TargetName};
use velnor_rust::{CONFIG_PATH, DetectError, LOCK_PATH, LintFault, Reason};

use crate::facts::TestResult;
use crate::generated_check::CHECK;
use crate::golden::{load, samples};
use crate::lint_lane::{LINT, MISE, runs};
use crate::lock_archive::archive_lock_at;
use crate::pipeline::{view, workflow};
use crate::private_sources::{SECTION, private_repo};

pub(crate) const FROM_SOURCE: &str = "[[lint]]\nkind = 'generated-check'\nfrom_source = true\n";
const ID: &str = "lint-generated-check";
pub(crate) const RUN_FROM_SOURCE: &str = "cargo run --locked";
pub(crate) const FIRST: &str = "0.1.112";
const STORE: &str = "MBX_CACHE_DIR=$RUNNER_TEMP/mbx";
const MIRROR: &str = "[crates_mirror]\nrunner = 'mirror-box'\nregistry = 'sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/'\ncainfo = '/opt/velnor/crates-mirror/ca-bundle.pem'\n";

pub(crate) fn bin(name: &str) -> Result<Target, Box<dyn std::error::Error>> {
    Ok(Target {
        name: TargetName::new(name)?,
        kinds: vec![TargetKind::Bin],
        tests: false,
        doctest: false,
        source: None,
        requires_features: false,
    })
}

pub(crate) fn packages(
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
        metadata.members[0].targets = vec![bin(name)?];
    }
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files
        .insert(LOCK_PATH, archive_lock_at(version)?.into_bytes());
    Ok(repo)
}

pub(crate) fn locked(config: &str, version: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    packages(config, version, &[("Cargo.toml", "velnor-gen")])
}

pub(crate) fn rendered(config: &str, version: &str) -> Result<String, Box<dyn std::error::Error>> {
    workflow(&locked(config, version)?)
}

fn refusal(repo: &RepoView) -> Result<GenerateError, Box<dyn std::error::Error>> {
    let mut locked = repo.clone();
    crate::facts::lock_pinned_tools(&mut locked)?;
    expected_with_notes(&locked)
        .err()
        .ok_or_else(|| "the repository was not refused".into())
}

fn invalid(reason: Reason) -> GenerateError {
    GenerateError::Detect(DetectError::Invalid {
        path: CONFIG_PATH,
        reason,
    })
}

pub(crate) fn nested_sample_job() -> Result<Value, Box<dyn std::error::Error>> {
    let repo = load(&samples().join("nested-generator-root"))?;
    let (files, _) = velnor_gen::expected_with_notes(&repo)?;
    let text = files
        .get(&RepoPath::new(".github/workflows/ci.yml")?)
        .ok_or("no workflow")?;
    job(text)
}

pub(crate) fn job(text: &str) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(crate::yaml_tree::document(text)?["jobs"][ID].clone())
}

pub(crate) fn position(job: &Value, predicate: impl Fn(&Value) -> bool) -> Option<usize> {
    job["steps"].as_array()?.iter().position(predicate)
}

pub(crate) fn run_contains(step: &Value, text: &str) -> bool {
    step["run"].as_str().is_some_and(|run| run.contains(text))
}

pub(crate) fn check_step(job: &Value) -> Value {
    job["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|step| run_contains(step, RUN_FROM_SOURCE))
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn names(job: &Value) -> Vec<String> {
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
    assert!(!check_step(&job).is_null(), "{job}");
    assert!(
        !names(&job)
            .iter()
            .any(|name| name.starts_with("Install velnor-gen")),
        "{job}"
    );
    assert!(!job.to_string().contains("VELNOR_GEN_ARCHIVE"), "{job}");
    assert!(
        check_step(&job)["run"]
            .as_str()
            .is_some_and(|run| run.contains("--package velnor-gen")),
        "{job}"
    );
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
    let check = check_step(&job);
    assert!(check["env"].is_null(), "{check}");
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
        let error = refusal(&locked(&format!("{LINT}\n{FROM_SOURCE}"), older)?)?;
        assert!(
            matches!(
                &error,
                GenerateError::ConfigKeyHelperTooOld { key: "lint.from_source", first: (0, 1, 112), pinned, .. }
                    if pinned.to_string() == older
            ),
            "{older}: {error:?}"
        );
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
    for (kind, name) in [
        ("kind = 'jactionlint'", "jactionlint"),
        ("kind = 'alint'\nconfig = '.alint.yml'", "alint"),
        ("kind = 'mise-task'\ntask = 'check'", "mise-task"),
    ] {
        let config = format!("[[lint]]\n{kind}\nfrom_source = true\n");
        let error = refusal(&locked(&config, FIRST)?)?;
        let foreign = Reason::LintKeyForeign {
            kind: name.to_owned(),
            key: "from_source",
        };
        assert_eq!(error, invalid(foreign), "{kind}");
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
    let error = refusal(&repo)?;
    assert_eq!(
        error,
        invalid(Reason::Lint(LintFault::ToolUnpinned("rust".to_owned())))
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
        let found = roots
            .iter()
            .filter(|(_, name)| *name == "velnor-gen")
            .count();
        let error = refusal(&repo)?;
        assert_eq!(
            error,
            invalid(Reason::GeneratorPackage { found }),
            "{roots:?}"
        );
    }
    Ok(())
}

#[test]
fn from_source_refuses_a_velnor_gen_package_without_a_binary() -> TestResult {
    let mut repo = locked(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?;
    let lib = Target {
        kinds: vec![TargetKind::Lib],
        ..bin("velnor-gen")?
    };
    repo.rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no root")?
        .members[0]
        .targets = vec![lib];
    let error = refusal(&repo)?;
    assert_eq!(error, invalid(Reason::GeneratorPackage { found: 0 }));
    Ok(())
}

pub(crate) fn private_generator() -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut repo = private_repo(&["Cargo.toml"], &format!("{SECTION}{FROM_SOURCE}"), FIRST)?;
    repo.tracked_paths.insert(LOCK_PATH);
    repo.files
        .insert(LOCK_PATH, archive_lock_at(FIRST)?.into_bytes());
    repo.files
        .insert(RepoPath::new("mise.toml")?, MISE.as_bytes().to_vec());
    let member = &mut repo
        .rust_metadata
        .get_mut(&RepoPath::new("Cargo.toml")?)
        .ok_or("no root")?
        .members[0];
    member.name = PackageName::new("velnor-gen")?;
    member.targets = vec![bin("velnor-gen")?];
    Ok(repo)
}

#[test]
fn from_source_fetches_private_dependencies_before_it_builds_the_generator() -> TestResult {
    let repo = private_generator()?;
    let job = job(&workflow(&repo)?)?;
    let token = position(&job, |step| step["id"] == "private-token").ok_or("no token")?;
    let fetch =
        position(&job, |step| run_contains(step, "\"$VELNOR_EXEC\" fetch ")).ok_or("no fetch")?;
    let check = position(&job, |step| run_contains(step, RUN_FROM_SOURCE)).ok_or("no check")?;
    assert!(token < fetch && fetch < check, "{job}");
    Ok(())
}

#[test]
fn from_source_restores_and_verifies_the_tool_home_before_installing_tools() -> TestResult {
    let job = job(&rendered(&format!("{LINT}\n{FROM_SOURCE}"), FIRST)?)?;
    let names = names(&job);
    let install = names
        .iter()
        .position(|name| name == "Install tools")
        .ok_or("no install tools step")?;
    for restore in [
        "Clear the tools directories",
        "Restore the tools cache",
        "Verify tools",
    ] {
        let at = names
            .iter()
            .position(|name| name == restore)
            .ok_or("no restore step")?;
        assert!(at < install, "{restore}: {job}");
    }
    Ok(())
}

#[test]
fn from_source_fetches_the_nested_generator_workspace_dependencies() -> TestResult {
    let roots = [
        ("Cargo.toml", "velnor-ci-app"),
        ("tools/gen/Cargo.toml", "velnor-gen"),
    ];
    let repo = packages(&format!("{SECTION}{FROM_SOURCE}"), FIRST, &roots)?;
    let job = job(&workflow(&repo)?)?;
    let fetch =
        position(&job, |step| run_contains(step, "\"$VELNOR_EXEC\" fetch ")).ok_or("no fetch")?;
    assert!(
        run_contains(&job["steps"][fetch], "tools/gen/Cargo.toml"),
        "{job}"
    );
    Ok(())
}
