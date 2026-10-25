//! I/O hardening cases: config sample, MBX transport.

use crate::impl_common::git_fixture;

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::DetectionStatus;
use velnor_actions_contract::{PullRequestCachePolicy, WorkflowPolicy};
use velnor_actions_orchestrator::prepare;

use crate::impl_common::without_ambient_identity;

#[path = "impl_validators_repo_schema.rs"]
mod schema_tests;

/// Test error shortcut.
type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Run git with inherited failure context.
fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = git_fixture::command(cwd)?
        .args(args)
        .current_dir(cwd)
        .status()?;
    assert!(status.success(), "git {args:?} failed");
    Ok(())
}

/// Release-manifest fixture so consumer `prepare` succeeds.
fn fixture_manifest_json() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let targets = velnor_actions_contract::SUPPORTED_TARGETS
    .iter()
    .map(|target| {
        format!(
            "{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}\",\"sha256\":\"{}\"}}",
            "a".repeat(64)
        )
    })
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{version}\",\"repository\":\"tailrocks/velnor-new\",\"targets\":[{targets}]}}"
    )
}

/// Live repository sample text (the file under review for drift).
fn repo_sample_text() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!("{}/../../.velnor/config.toml", env!("CARGO_MANIFEST_DIR"));
    Ok(fs::read_to_string(path)?)
}

/// Live task source paired with the repository configuration sample.
fn repo_mise_text() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!("{}/../../mise.toml", env!("CARGO_MANIFEST_DIR"));
    Ok(fs::read_to_string(path)?)
}

/// Restrict the live check to its task source inside the minimal plan fixture.
fn fixture_config_sample(sample: &str) -> Result<String, Box<dyn std::error::Error>> {
    let table: toml::Table = toml::from_str(sample)?;
    let mut table = table;
    let check = table
        .get_mut("checks")
        .and_then(toml::Value::as_array_mut)
        .ok_or("live sample has no checks array")?
        .first_mut()
        .and_then(toml::Value::as_table_mut)
        .ok_or("live sample has no named check")?;
    check.insert(
        "inputs".into(),
        toml::Value::Array(vec![toml::Value::String("mise.toml".into())]),
    );
    // Keep the sample's native-helper and CLI rules; the rest name live
    // packages that this compact fixture does not carry.
    if let Some(inputs) = table
        .get_mut("stacks")
        .and_then(|stacks| stacks.get_mut("rust"))
        .and_then(|rust| rust.get_mut("inputs"))
        .and_then(toml::Value::as_array_mut)
    {
        inputs.retain(|rule| {
            rule.get("packages")
                .and_then(toml::Value::as_array)
                .is_some_and(|packages| {
                    packages.iter().all(|name| {
                        matches!(
                            name.as_str(),
                            Some("velnor-actions-workflow-renderer" | "velnor-actions-cli")
                        )
                    })
                })
        });
    }
    Ok(toml::to_string(&table)?)
}

/// Git fixture carrying the live sample plus its declared CLI input owner.
fn make_sample_repo(sample: &str, mise: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    git(
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/tailrocks/velnor-new.git",
        ],
        root,
    )?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(root.join(".velnor/config.toml"), sample)?;
    fs::write(root.join("mise.toml"), mise)?;
    fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/velnor-actions-cli\", \"crates/velnor-actions-workflow-renderer\"]\nresolver = \"3\"\n",
    )?;
    let cli = root.join("crates/velnor-actions-cli");
    fs::create_dir_all(cli.join("src"))?;
    fs::write(
        cli.join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(cli.join("src/main.rs"), "fn main() {}\n")?;
    let renderer = root.join("crates/velnor-actions-workflow-renderer");
    fs::create_dir_all(renderer.join("src"))?;
    fs::write(
        renderer.join("Cargo.toml"),
        "[package]\nname = \"velnor-actions-workflow-renderer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(renderer.join("src/lib.rs"), "pub fn f() {}\n")?;
    git(&["add", "-A"], root)?;
    git(&["commit", "-m", "fixture"], root)?;
    git(
        &["update-ref", "refs/remotes/origin/testmain", "HEAD"],
        root,
    )?;
    git(
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/testmain",
        ],
        root,
    )?;
    Ok(dir)
}

#[test]
fn repo_config_sample_parses_through_prepare() -> TestResult {
    without_ambient_identity("repo_config_sample_parses_through_prepare", || {
        let sample = fixture_config_sample(&repo_sample_text()?)?;
        let mise = repo_mise_text()?;
        let repo = make_sample_repo(&sample, &mise)?;
        let prep = prepare(repo.path())?;
        assert_eq!(prep.config.schema, 2);
        assert_eq!(prep.config.checks.len(), 1);
        assert_eq!(prep.config.checks[0].id, "maintenance-helpers");
        assert_eq!(prep.config.checks[0].task, "test-maintenance-helpers");
        assert_sample_input_owners(&prep)?;
        let execution = prep
            .config
            .execution
            .as_ref()
            .ok_or("schema 2 sample has no execution")?;
        assert_eq!(execution.default_profile, "hosted");
        assert_eq!(
            prep.config.workflow.policy,
            WorkflowPolicy::VelnorRepositoryV1
        );
        assert_eq!(
            prep.config.workflow.pull_request_cache_policy,
            PullRequestCachePolicy::SameRepositoryScoped
        );
        assert_eq!(
            prep.config.discovery.exclude,
            vec!["fixtures/**", "crates/**/tests/fixtures/**"]
        );
        // The live sample pins the branch: config wins over origin/HEAD so CI
        // checkouts (which create no origin/HEAD) still resolve the branch.
        assert_eq!(prep.default_branch, "main");
        Ok(())
    })
}

/// Keep each sample input rule attached to its declared workspace package.
fn assert_sample_input_owners(
    prep: &velnor_actions_orchestrator::GenerationPreparation,
) -> TestResult {
    let rust = prep
        .config
        .stacks
        .rust
        .as_ref()
        .ok_or("sample Rust policy")?;
    let cli = rust
        .inputs
        .iter()
        .find(|rule| rule.packages == ["velnor-actions-cli"])
        .ok_or("live CLI repository input rule disappeared during typed admission")?;
    assert_eq!(
        cli.globs,
        [
            "crates/velnor-actions-*/**",
            "crates/velnor-archive-guard/**",
            "crates/velnor-runner/Cargo.toml",
            "crates/velnor-runner/crates/*/**",
        ]
    );
    assert!(cli.complete, "CLI repository inputs remain complete");
    assert_package_owner(prep, "velnor-actions-cli");
    Ok(())
}

/// Require a declared sample owner to resolve inside its workspace.
fn assert_package_owner(prep: &velnor_actions_orchestrator::GenerationPreparation, name: &str) {
    assert!(
        prep.discovery
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.record.packages)
            .any(|package| package.name == name && package.in_workspace && !package.external),
        "fixture is missing declared input owner {name}"
    );
}

#[test]
fn repo_sample_excludes_nested_cargo_test_fixtures_before_admission() -> TestResult {
    without_ambient_identity(
        "repo_sample_excludes_nested_cargo_test_fixtures_before_admission",
        || {
            let sample = fixture_config_sample(&repo_sample_text()?)?;
            let mise = repo_mise_text()?;
            let repo = make_sample_repo(&sample, &mise)?;
            let root = repo.path();
            let fixture =
                "crates/velnor-actions-mise/tests/fixtures/mbx-synchronous/registry-fixture";
            let manifest = format!("{fixture}/Cargo.toml");
            let source = root.join(fixture).join("src");
            fs::create_dir_all(&source)?;
            fs::write(
                root.join(&manifest),
                "[package]\nname = \"mbx-synchronous-registry-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            )?;
            fs::write(source.join("lib.rs"), "pub fn fixture() {}\n")?;

            let prep = prepare(root)?;
            assert!(
                prep.discovery.statuses.iter().all(|status| match status {
                    DetectionStatus::Selected(project)
                    | DetectionStatus::Ignored { project, .. } => project.manifest != manifest,
                }),
                "nested fixture entered detection status"
            );
            assert!(
                prep.discovery
                    .workspaces
                    .iter()
                    .flat_map(|workspace| &workspace.record.packages)
                    .all(|package| package.manifest != manifest),
                "nested fixture entered package inventory"
            );
            assert!(
                prep.discovery
                    .proposals
                    .iter()
                    .all(|task| task.display_name != "mbx-synchronous-registry-fixture"),
                "nested fixture entered task proposals"
            );
            Ok(())
        },
    )
}

/// Pinned `uses:` ref fixture.
pub(crate) fn uses(name: &str) -> String {
    format!("{name}@{}", "a".repeat(40))
}
