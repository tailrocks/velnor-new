//! I/O hardening cases: config sample, MBX transport.

use crate::impl_common::git_fixture;

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::DetectionStatus;
use velnor_actions_contract::{PullRequestCachePolicy, WorkflowPolicy};
use velnor_actions_orchestrator::prepare;

use crate::impl_common::without_ambient_identity;

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
    let checks = table
        .get_mut("checks")
        .and_then(toml::Value::as_array_mut)
        .ok_or("live sample has no checks array")?;
    let check = checks
        .first_mut()
        .and_then(toml::Value::as_table_mut)
        .ok_or("live sample has no named check")?;
    check.insert(
        "inputs".into(),
        toml::Value::Array(vec![toml::Value::String("mise.toml".into())]),
    );
    Ok(toml::to_string(&table)?)
}

/// Git fixture carrying the live sample plus the Velnor identity.
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
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\n")?;
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

#[test]
fn repo_config_sample_covers_schema_keys() -> TestResult {
    let sample = repo_sample_text()?;
    let table: toml::Table = toml::from_str(&sample)?;
    for key in table.keys() {
        assert!(
            [
                "schema",
                "workflow",
                "resources",
                "test_sharding",
                "stacks",
                "discovery",
                "actions",
                "execution",
                "checks",
                "qualified_tools",
            ]
            .contains(&key.as_str()),
            "sample key outside schema: {key}"
        );
    }
    let section_keys = |section: &str| -> Vec<String> {
        table
            .get(section)
            .and_then(toml::Value::as_table)
            .map(|values| values.keys().cloned().collect())
            .unwrap_or_default()
    };
    for key in section_keys("workflow") {
        assert!(
            [
                "name",
                "policy",
                "default_branch",
                "generator_validation",
                "max_parallel_jobs",
                "pull_request_cache_policy",
                "runner_label",
            ]
            .contains(&key.as_str()),
            "workflow key outside schema: {key}"
        );
    }
    for key in section_keys("discovery") {
        assert_eq!(key, "exclude", "discovery key outside schema: {key}");
    }
    for key in section_keys("execution") {
        assert!(
            [
                "default_profile",
                "mode",
                "hosted_profile",
                "scale_set_profile",
                "profiles",
                "parity",
                "overrides",
                "workflows",
            ]
            .contains(&key.as_str()),
            "execution key outside schema: {key}"
        );
    }
    assert_sample_mentions(&sample);
    Ok(())
}

/// Tokens the live schema-2 sample must still spell.
fn assert_sample_mentions(sample: &str) {
    for token in [
        "schema",
        "[workflow]",
        "name",
        "policy",
        "default_branch",
        "generator_validation",
        "max_parallel_jobs",
        "pull_request_cache_policy",
        "[resources]",
        "compiler_process_budget",
        "test_process_budget",
        "[test_sharding]",
        "default_shards",
        "by_manifest",
        "[stacks]",
        "ignore",
        "[stacks.rust]",
        "configurations",
        "[discovery]",
        "exclude",
        "[actions.overrides]",
        "[execution]",
        "default_profile",
        "hosted_profile",
        "scale_set_profile",
        "workflows",
    ] {
        assert!(
            sample.contains(token),
            "schema key missing from sample: {token}"
        );
    }
}

/// Pinned `uses:` ref fixture.
pub(crate) fn uses(name: &str) -> String {
    format!("{name}@{}", "a".repeat(40))
}
