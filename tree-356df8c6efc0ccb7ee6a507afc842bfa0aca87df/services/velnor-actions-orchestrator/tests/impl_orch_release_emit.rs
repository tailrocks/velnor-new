//! Release emission integration cases over tempdir-built fixtures.

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{
    GenerateOptions, OrchestratorError, generate, prepare, render_staged_tree,
};

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, plan_for,
    without_ambient_identity,
};

/// Release-enabled consumer config selecting the fixture crate.
const OIDC_CONFIG: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n";

/// Full generated tree with release enabled, in sorted path order.
const RELEASE_FAMILY: [&str; 8] = [
    ".github/AGENTS.md",
    ".github/CLAUDE.md",
    ".github/actionlint.yaml",
    ".github/actions/velnor-tool-seed/action.yml",
    ".github/release-plz-bootstrap.toml",
    ".github/release-plz.toml",
    ".github/workflows/ci.yml",
    ".github/workflows/release.yml",
];

/// Committed fixture repo with a GitHub origin (release identity inputs).
fn release_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let repo = make_repo(config)?;
    let root = repo.path();
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
    git(&["commit", "-m", "release fixture"], root)?;
    Ok(repo)
}

/// Expected family as owned strings for report comparison.
fn family_vec() -> Vec<String> {
    RELEASE_FAMILY.iter().map(ToString::to_string).collect()
}

/// Forge bindings, omitted default registry, and checkout split (OIDC mode).
fn assert_oidc_release_shape(yaml: &str, head: &str) {
    assert_eq!(
        yaml.matches("GIT_TOKEN: ${{ secrets.GITHUB_TOKEN }}")
            .count(),
        4,
        "forge binding on every release-plz step:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("secrets.").count(),
        4,
        "no secret outside the forge binding:\n{yaml}"
    );
    assert!(
        !yaml.contains("--registry"),
        "default registry omits the flag:\n{yaml}"
    );
    assert_eq!(
        yaml.matches(&format!("ref: {head}")).count(),
        2,
        "source pin on preflight plus publish only:\n{yaml}"
    );
    assert!(
        yaml.contains("--manifest-path release-source/Cargo.toml"),
        "source manifest binding:\n{yaml}"
    );
    assert!(
        yaml.contains("--manifest-path Cargo.toml"),
        "policy manifest binding:\n{yaml}"
    );
    assert!(
        yaml.contains("fetch-depth: \"0\""),
        "full history everywhere:\n{yaml}"
    );
}

#[test]
fn release_disabled_emits_base_tree_only() -> TestResult {
    let explicit = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = false\n";
    for config in [config_with_branch(), explicit] {
        let repo = make_repo(config)?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let paths: Vec<&str> = tree.files.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                ".github/AGENTS.md",
                ".github/actionlint.yaml",
                ".github/actions/velnor-tool-seed/action.yml",
                ".github/workflows/ci.yml"
            ],
            "release absent without opt-in"
        );
    }
    Ok(())
}

#[test]
fn plan_lists_release_files_iff_enabled() -> TestResult {
    let repo = release_repo(OIDC_CONFIG)?;
    let plan = plan_for(&prepare(repo.path())?)?;
    for path in [
        ".github/release-plz-bootstrap.toml",
        ".github/release-plz.toml",
        ".github/workflows/release.yml",
    ] {
        assert!(plan.contains(path), "plan names {path}:\n{plan}");
    }
    let bare = make_repo(config_with_branch())?;
    let bare_plan = plan_for(&prepare(bare.path())?)?;
    assert!(
        !bare_plan.contains("release.yml") && !bare_plan.contains("release-plz"),
        "no release inventory without opt-in:\n{bare_plan}"
    );
    Ok(())
}

#[test]
fn release_enabled_emits_family_oidc() -> TestResult {
    let repo = release_repo(OIDC_CONFIG)?;
    let root = repo.path();
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let prep = prepare(root)?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    assert_eq!(report.files_written, family_vec());
    let yaml = fs::read_to_string(preview.join(".github/workflows/release.yml"))?;
    assert!(
        yaml.contains("github.repository == 'acme/widgets'"),
        "repo gate"
    );
    assert!(
        yaml.contains(&format!("release-{}", &head[..12])),
        "plan binding"
    );
    assert!(yaml.contains(&head), "exact source binding");
    assert!(
        yaml.contains("--config .github/release-plz.toml"),
        "explicit config"
    );
    assert!(yaml.contains("environment: release"), "pinned env");
    assert!(yaml.contains("id-token: write"), "oidc grant");
    assert!(
        !yaml.contains("CARGO_REGISTRY_TOKEN"),
        "oidc carries no token"
    );
    assert_oidc_release_shape(&yaml, &head);
    assert!(
        !yaml.contains("release-publish-bootstrap"),
        "oidc has no bootstrap job"
    );
    assert!(
        !yaml.contains("inputs.version"),
        "oidc binds no generation-time version"
    );
    let normal = fs::read_to_string(preview.join(".github/release-plz.toml"))?;
    assert!(normal.contains("release_always = false"), "normal policy");
    assert!(normal.contains("name = \"demo\""), "allowlist:\n{normal}");
    let bootstrap = fs::read_to_string(preview.join(".github/release-plz-bootstrap.toml"))?;
    assert!(
        bootstrap.contains("release_always = true"),
        "bootstrap policy"
    );
    for rel in &report.files_written {
        let text = fs::read_to_string(preview.join(rel))?;
        assert!(
            text.starts_with("# Generated by Velnor Actions "),
            "marker for {rel}"
        );
    }
    Ok(())
}

#[test]
fn release_emission_is_byte_deterministic() -> TestResult {
    let repo = release_repo(OIDC_CONFIG)?;
    let first = render_staged_tree(&prepare(repo.path())?)?;
    let second = render_staged_tree(&prepare(repo.path())?)?;
    assert_eq!(first, second, "identical inputs render identical trees");
    Ok(())
}

#[test]
fn release_bootstrap_mode_adds_token_job() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
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
    git(&["commit", "-m", "bootstrap fixture"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\nauthentication = \"bootstrap-token\"\n[stacks.rust.release.bootstrap]\npackage = \"demo\"\nversion = \"0.1.0\"\nsource_sha = \"{head}\"\n"
    );
    fs::write(root.join(".velnor/config.toml"), config)?;
    let prep = prepare(root)?;
    let parent = TempDir::new()?;
    let preview = parent.path().join("preview");
    let report = generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview.clone()),
        },
    )?;
    assert_eq!(report.files_written, family_vec());
    let yaml = fs::read_to_string(preview.join(".github/workflows/release.yml"))?;
    assert!(
        yaml.contains("release-publish-bootstrap"),
        "bootstrap job present"
    );
    assert_eq!(
        yaml.matches("--config .github/release-plz-bootstrap.toml")
            .count(),
        2,
        "preflight plus bootstrap publish bind the bootstrap config:\n{yaml}"
    );
    assert_eq!(
        yaml.matches("--config .github/release-plz.toml").count(),
        3,
        "preparation, oidc publish, and reconcile keep the normal config:\n{yaml}"
    );
    assert!(
        yaml.contains("${{ secrets.CARGO_REGISTRY_TOKEN }}"),
        "single registry binding"
    );
    assert_eq!(
        yaml.matches("CARGO_REGISTRY_TOKEN").count(),
        2,
        "exactly one registry binding (key plus ref)"
    );
    assert_eq!(
        yaml.matches("GIT_TOKEN: ${{ secrets.GITHUB_TOKEN }}")
            .count(),
        5,
        "forge binding on every release-plz step:\n{yaml}"
    );
    assert!(
        !yaml.contains("--registry"),
        "default registry omits the flag:\n{yaml}"
    );
    assert_eq!(
        yaml.matches(&format!("ref: {head}")).count(),
        3,
        "source pin on preflight plus both publishers:\n{yaml}"
    );
    assert!(
        yaml.contains("github.event.inputs.version == '0.1.0'"),
        "publishers gate on the bootstrap version:\n{yaml}"
    );
    assert!(
        yaml.contains("version:") && yaml.contains("0.1.0"),
        "dispatch carries the version input:\n{yaml}"
    );
    Ok(())
}

#[test]
fn release_without_release_pr_validates_instead() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\nrelease_pr = false\n";
    let repo = release_repo(config)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let yaml = tree
        .get(".github/workflows/release.yml")
        .ok_or_else(|| std::io::Error::other("missing workflow"))?;
    assert!(yaml.contains("Validate release"), "dry-run preparation");
    assert!(!yaml.contains(" release-pr "), "no release-pr phase");
    Ok(())
}

#[test]
fn release_requires_origin_identity() -> TestResult {
    let repo = make_repo(OIDC_CONFIG)?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "no origin"], root)?;
    let prep = prepare(root)?;
    let err = err_of(render_staged_tree(&prep), "origin required")?;
    assert!(
        matches!(err, OrchestratorError::IdentityRejected { .. }),
        "got {err}"
    );
    Ok(())
}

#[test]
fn release_unknown_package_fails_closed() -> TestResult {
    let config = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = true\npackages = [\"nope\"]\n";
    let repo = release_repo(config)?;
    let prep = prepare(repo.path())?;
    let err = err_of(render_staged_tree(&prep), "unknown package")?;
    assert!(
        err.to_string().contains("unknown_package:nope"),
        "got {err}"
    );
    Ok(())
}

#[test]
fn release_bootstrap_mismatch_fails_closed() -> TestResult {
    let head = "0123456789abcdef0123456789abcdef01234567";
    for (package, version, want) in [
        ("other", "0.1.0", "bootstrap_package_not_selected"),
        ("demo", "9.9.9", "bootstrap_version_mismatch"),
    ] {
        let config = format!(
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\nauthentication = \"bootstrap-token\"\n[stacks.rust.release.bootstrap]\npackage = \"{package}\"\nversion = \"{version}\"\nsource_sha = \"{head}\"\n"
        );
        let repo = release_repo(&config)?;
        let prep = prepare(repo.path())?;
        let err = err_of(render_staged_tree(&prep), want)?;
        assert!(err.to_string().contains(want), "got {err}");
    }
    Ok(())
}

#[test]
fn release_velnor_policy_rejects_enabled() -> TestResult {
    without_ambient_identity("release_velnor_policy_rejects_enabled", || {
        let config = "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\npolicy = \"velnor-repository-v1\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n";
        let repo = make_repo(config)?;
        let git_config = repo.path().join(".git/config");
        let mut text = fs::read_to_string(&git_config)?;
        text.push_str("[remote \"origin\"]\n\turl = https://github.com/tailrocks/velnor-new.git\n");
        fs::write(&git_config, text)?;
        let prep = prepare(repo.path())?;
        let err = err_of(render_staged_tree(&prep), "velnor policy")?;
        assert!(
            err.to_string().contains("release_requires_consumer_policy"),
            "got {err}"
        );
        Ok(())
    })
}
