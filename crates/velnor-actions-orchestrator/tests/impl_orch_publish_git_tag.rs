//! `registry = "git-tag"` emission over tempdir-built fixtures.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};

use crate::impl_common::{TestResult, err_of, git, make_repo, plan_for};

const GIT_TAG_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[stacks.rust.release]\nenabled = true\nregistry = \"git-tag\"\npackages = [\"demo\"]\n";

/// Private crate (`publish = false`) with a GitHub origin.
fn git_tag_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n",
    )?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/widgets.git",
        ],
        root,
    )?;
    git(&["add", "."], root)?;
    git(&["commit", "-m", "git-tag fixture"], root)?;
    Ok(repo)
}

fn generated(config: &str) -> Result<(String, String, Vec<String>), Box<dyn std::error::Error>> {
    let repo = git_tag_repo(config)?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    let report = generate(
        &prepare(repo.path())?,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    Ok((
        fs::read_to_string(preview.join(".github/workflows/publish.yml"))?,
        fs::read_to_string(preview.join(".github/release-plz.toml"))?,
        report.files_written,
    ))
}

#[test]
fn git_tag_emits_publication_workflow_and_config_only() -> TestResult {
    let (_, _, files) = generated(GIT_TAG_CONFIG)?;
    assert!(files.contains(&".github/workflows/publish.yml".to_owned()));
    assert!(files.contains(&".github/release-plz.toml".to_owned()));
    for absent in [
        ".github/workflows/release.yml",
        ".github/release-plz-bootstrap.toml",
    ] {
        assert!(!files.contains(&absent.to_owned()), "{absent}");
    }
    let repo = git_tag_repo(GIT_TAG_CONFIG)?;
    let plan = plan_for(&prepare(repo.path())?)?;
    assert!(
        plan.contains("release-plz.toml") && plan.contains("publish.yml"),
        "{plan}"
    );
    assert!(
        !plan.contains("release.yml") && !plan.contains("bootstrap"),
        "{plan}"
    );
    Ok(())
}

#[test]
fn release_plz_config_creates_tags_and_releases_only() -> TestResult {
    let (_, toml, _) = generated(GIT_TAG_CONFIG)?;
    for line in [
        "publish = false",
        "git_only = true",
        "git_tag_enable = true",
        "git_release_enable = true",
        "release_always = true",
        "semver_check = false",
        "git_tag_name = \"{{ package }}-v{{ version }}\"",
        "name = \"demo\"",
    ] {
        assert!(toml.contains(line), "{line}:\n{toml}");
    }
    assert!(
        !toml.contains("publish_no_verify") && !toml.contains("publish = true"),
        "{toml}"
    );
    Ok(())
}

#[test]
fn tag_release_is_gated_unprivileged_beyond_contents_write() -> TestResult {
    let (yaml, _, _) = generated(GIT_TAG_CONFIG)?;
    assert!(yaml.contains("branches:\n      - testmain"), "{yaml}");
    assert!(
        !yaml.contains("tags:"),
        "no tag trigger without bundles:\n{yaml}"
    );
    for forbidden in [
        "pull_request",
        "workflow_run",
        "workflow_dispatch",
        "id-token",
        "CARGO_REGISTRY_TOKEN",
        "pull-requests",
    ] {
        assert!(!yaml.contains(forbidden), "{forbidden}:\n{yaml}");
    }
    assert!(
        yaml.contains("rust-tag-release:") && yaml.contains("publish-branch-authority:"),
        "{yaml}"
    );
    assert!(yaml.contains("environment: release"), "{yaml}");
    assert!(yaml.contains("contents: write"), "{yaml}");
    assert!(
        yaml.contains("Wait for Required check") && yaml.contains("check_name=Required"),
        "{yaml}"
    );
    assert!(yaml.contains("refs/heads/testmain"), "{yaml}");
    assert!(
        yaml.contains("GIT_TOKEN: ${{ secrets.GITHUB_TOKEN }}"),
        "{yaml}"
    );
    assert!(
        yaml.contains("release-plz@") && yaml.contains(" release "),
        "{yaml}"
    );
    assert!(yaml.contains("--config .github/release-plz.toml"), "{yaml}");
    assert!(yaml.contains("timeout-minutes: 35"), "{yaml}");
    Ok(())
}

#[test]
fn credentials_and_workspace_mode_are_contradictory() -> TestResult {
    for extra in [
        "authentication = \"trusted-publishing\"\n",
        "authentication = \"bootstrap-token\"\n",
    ] {
        let config = format!("{GIT_TAG_CONFIG}{extra}");
        let repo = git_tag_repo(&config)?;
        let err = err_of(prepare(repo.path()).map(|_| ()), "credential")?.to_string();
        assert!(err.contains("contradictory_authentication"), "{err}");
    }
    let workspace = GIT_TAG_CONFIG.replace("packages = [\"demo\"]", "publishable_workspace = true");
    let repo = git_tag_repo(&workspace)?;
    let err = err_of(prepare(repo.path()).map(|_| ()), "workspace")?.to_string();
    assert!(err.contains("git_tag_requires_packages"), "{err}");
    Ok(())
}

#[test]
fn bundles_and_git_tag_share_one_workflow_with_split_authorities() -> TestResult {
    let config = format!(
        "{GIT_TAG_CONFIG}[[publish.bundles]]\nid = \"demo-schema\"\npaths = [\"src\"]\nversion = \"1.0.0\"\n"
    );
    let (yaml, _, _) = generated(&config)?;
    assert!(
        yaml.contains("tags:") && yaml.contains("branches:\n      - testmain"),
        "{yaml}"
    );
    assert!(yaml.contains("if: github.ref_type == 'tag'"), "{yaml}");
    assert!(yaml.contains("if: github.ref_type == 'branch'"), "{yaml}");
    assert!(
        yaml.contains("publish-authority:") && yaml.contains("publish-branch-authority:"),
        "{yaml}"
    );
    Ok(())
}

#[test]
fn unselected_or_unknown_packages_fail_generation() -> TestResult {
    let config = GIT_TAG_CONFIG.replace("[\"demo\"]", "[\"ghost\"]");
    let repo = git_tag_repo(&config)?;
    let err = err_of(render_staged_tree(&prepare(repo.path())?), "ghost")?.to_string();
    assert!(err.contains("ghost"), "{err}");
    Ok(())
}
