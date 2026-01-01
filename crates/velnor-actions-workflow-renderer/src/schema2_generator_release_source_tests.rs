use super::QUALIFICATION_SOURCE_PREPARE;
use crate::schema2::{GeneratorReleasePins, Schema2WorkflowRequest};
use crate::setup::MiseSetup;
use crate::yaml::Yaml;
use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-qualification-source-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn fetch_uses_exact_public_commit_without_credentials_or_helpers() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let workspace = scratch.0.join("workspace");
    let bin = scratch.0.join("bin");
    let log = scratch.0.join("git-arguments");
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(&bin)?;
    let git = bin.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
set -eu
test -z "${GITHUB_TOKEN:-}"
test -z "${GH_TOKEN:-}"
test -z "${ACTIONS_RUNTIME_TOKEN:-}"
test -z "${ACTIONS_ID_TOKEN_REQUEST_TOKEN:-}"
test -z "${MISE_GITHUB_TOKEN:-}"
printf '%s\n' "$@" >> "$MOCK_GIT_LOG"
if [ "$1" = rev-parse ]; then printf '%s\n' "$MOCK_EXPECTED_SHA"; fi
"#,
    )?;
    let mut permissions = fs::metadata(&git)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&git, permissions)?;
    let original_path = std::env::var("PATH")?;
    let path = format!("{}:{original_path}", bin.display());
    let commit = "0123456789abcdef0123456789abcdef01234567";
    let result = Command::new("bash")
        .arg("-c")
        .arg(QUALIFICATION_SOURCE_PREPARE)
        .env("PATH", path)
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", commit)
        .env("GITHUB_TOKEN", "workflow-token")
        .env("GH_TOKEN", "gh-token")
        .env("ACTIONS_RUNTIME_TOKEN", "runtime-token")
        .env("ACTIONS_ID_TOKEN_REQUEST_TOKEN", "oidc-token")
        .env("MISE_GITHUB_TOKEN", "mise-token")
        .env("MOCK_GIT_LOG", &log)
        .env("MOCK_EXPECTED_SHA", commit)
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let arguments = fs::read_to_string(log)?;
    assert!(arguments.contains("https://github.com/tailrocks/velnor-new.git"));
    assert!(arguments.contains(commit));
    assert!(arguments.contains("credential.helper="));
    assert!(arguments.contains("--depth=1"));
    assert!(arguments.contains("--no-tags"));
    assert!(QUALIFICATION_SOURCE_PREPARE.contains("timeout=60"));
    Ok(())
}

#[test]
fn reject_invalid_source_identity_before_running_git() -> Result<(), Box<dyn Error>> {
    let scratch = Scratch::new()?;
    let workspace = scratch.0.join("workspace");
    fs::create_dir_all(&workspace)?;
    let log = scratch.0.join("git-arguments");
    let result = Command::new("bash")
        .arg("-c")
        .arg(QUALIFICATION_SOURCE_PREPARE)
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_REPOSITORY", "attacker/other")
        .env("GITHUB_SHA", "not-a-commit")
        .env("MOCK_GIT_LOG", &log)
        .output()?;
    assert!(!result.status.success());
    assert!(!log.exists(), "Git ran for an invalid source identity");
    Ok(())
}

#[test]
fn every_qualifier_fetches_validated_public_source_before_local_action()
-> Result<(), Box<dyn Error>> {
    let request = Schema2WorkflowRequest {
        version: "0.1.1".to_owned(),
        hosted_label: "ubuntu-26.04".to_owned(),
        scale_set: Schema2WorkflowRequest::canonical_scale_set()?,
        workflows: BTreeSet::new(),
        mbx_qualification: None,
        generator_release: Some(test_pins()),
    };
    let workflow = super::super::generator_release(&request)?.workflow;
    let jobs = map_field(map_entries(&workflow)?, "jobs")?;
    let jobs = map_entries(jobs)?;

    for (job_id, action) in [
        (
            "qualify-linux",
            "./.github/actions/generator-release-qualify-linux",
        ),
        (
            "qualify-macos",
            "./.github/actions/generator-release-qualify-macos",
        ),
    ] {
        let job = map_field(jobs, job_id)?;
        let steps = sequence(map_field(map_entries(job)?, "steps")?)?;
        assert_eq!(steps.len(), 2, "{job_id} must fetch source, then qualify");
        assert_source_step(&steps[0], job_id)?;
        assert_qualifier_step(&steps[1], steps, action, job_id)?;
    }
    Ok(())
}

fn assert_source_step(step: &Yaml, job_id: &str) -> Result<(), Box<dyn Error>> {
    let source = map_entries(step)?;
    assert_eq!(scalar(map_field(source, "shell")?)?, "bash", "{job_id}");
    let source_run = scalar(map_field(source, "run")?)?;
    assert!(
        !has_field(source, "uses"),
        "{job_id} must acquire source with an inline executable step"
    );
    assert!(
        source_run.contains("tailrocks/velnor-new")
            && source_run.contains("https://github.com/{repository}.git")
            && source_run.contains("GITHUB_REPOSITORY")
            && source_run.contains("GITHUB_SHA")
            && source_run.contains(r#"r"[0-9a-f]{40}""#)
            && source_run.contains("--depth=1")
            && source_run.contains("--no-tags")
            && source_run
                .contains("run_git(\"checkout\", \"--quiet\", \"--detach\", \"FETCH_HEAD\"")
            && source_run.contains("head != commit"),
        "{job_id} must validate, fetch, and verify the exact public source commit"
    );
    for credential in [
        "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "GITHUB_TOKEN",
        "GH_TOKEN",
        "MISE_GITHUB_TOKEN",
    ] {
        assert!(
            source_run.contains(&format!("\"{credential}\""))
                && source_run.contains("environment.pop(name, None)"),
            "{job_id} must scrub {credential} before fetching source"
        );
    }
    assert!(
        !has_field(source, "env"),
        "{job_id} source acquisition must not receive a credential env"
    );
    Ok(())
}

fn assert_qualifier_step(
    step: &Yaml,
    steps: &[Yaml],
    expected_action: &str,
    job_id: &str,
) -> Result<(), Box<dyn Error>> {
    let qualification = map_entries(step)?;
    assert_eq!(
        scalar(map_field(qualification, "uses")?)?,
        expected_action,
        "{job_id}"
    );
    for step in steps {
        let fields = map_entries(step)?;
        if let Some((_, uses)) = fields.iter().find(|(key, _)| key == "uses") {
            assert!(
                !scalar(uses)?.starts_with("actions/checkout@"),
                "{job_id} must not depend on the default checkout action"
            );
        }
    }
    Ok(())
}

fn test_pins() -> GeneratorReleasePins {
    let setup = MiseSetup {
        uses: format!("jdx/mise-action@{}", "a".repeat(40)),
        version: "2026.9.18".to_owned(),
        sha256: "b".repeat(64),
    };
    GeneratorReleasePins {
        linux_x86_64_setup: setup.clone(),
        macos_arm64_setup: setup,
        install_gate_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_build_tools_argv: vec!["mise".to_owned(), "install".to_owned()],
        install_gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "install",
            "gh@2.102.0",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        build_argv: vec!["mise".to_owned(), "exec".to_owned()],
        actionlint_argv: vec!["mise".to_owned(), "exec".to_owned()],
        zizmor_argv: vec!["mise".to_owned(), "exec".to_owned()],
        gh_argv: [
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "gh@2.102.0",
            "--",
            "gh",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        rust_version: "1.98.1".to_owned(),
        mr_boxington_version: "1.21.1".to_owned(),
    }
}

fn map_entries(value: &Yaml) -> Result<&[(String, Yaml)], Box<dyn Error>> {
    match value {
        Yaml::Map(entries) => Ok(entries),
        _ => Err("expected YAML mapping".into()),
    }
}

fn map_field<'a>(entries: &'a [(String, Yaml)], key: &str) -> Result<&'a Yaml, Box<dyn Error>> {
    entries
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| format!("missing YAML mapping field {key}").into())
}

fn has_field(entries: &[(String, Yaml)], key: &str) -> bool {
    entries.iter().any(|(name, _)| name == key)
}

fn sequence(value: &Yaml) -> Result<&[Yaml], Box<dyn Error>> {
    match value {
        Yaml::Seq(items) => Ok(items),
        _ => Err("expected YAML sequence".into()),
    }
}

fn scalar(value: &Yaml) -> Result<&str, Box<dyn Error>> {
    match value {
        Yaml::Str(value) | Yaml::Quoted(value) | Yaml::Annotated { value, .. } => Ok(value),
        _ => Err(format!("expected YAML scalar string, got {value:?}").into()),
    }
}
