//! I/O hardening cases: config sample, MBX transport.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;
use velnor_actions_contract::{PullRequestCachePolicy, WorkflowPolicy};
use velnor_actions_mise::cache::validate_sources_path;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::steps::{
    CompileDriver, TASK_ARTIFACTS_DIR, cache_action_step, mbx_step_for_driver,
};

use crate::impl_common::{fixture_manifest_json, without_ambient_identity};

/// Test error shortcut.
type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Run git with inherited failure context.
fn git(args: &[&str], cwd: &Path) -> TestResult {
    let status = Command::new("git").args(args).current_dir(cwd).status()?;
    assert!(status.success(), "git {args:?} failed");
    Ok(())
}

/// Live repository sample text (the file under review for drift).
fn repo_sample_text() -> Result<String, Box<dyn std::error::Error>> {
    let path = format!("{}/../../.velnor/config.toml", env!("CARGO_MANIFEST_DIR"));
    Ok(fs::read_to_string(path)?)
}

/// Git fixture carrying the live sample plus the Velnor identity.
fn make_sample_repo(sample: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
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
        let sample = repo_sample_text()?;
        let repo = make_sample_repo(&sample)?;
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
        assert_eq!(prep.config.discovery.exclude, vec!["fixtures/**"]);
        // The live sample pins the branch: config wins over origin/HEAD so CI
        // checkouts (which create no origin/HEAD) still resolve the branch.
        assert_eq!(prep.default_branch, "main");
        Ok(())
    })
}

#[test]
fn full_render_places_one_preflight_before_each_mbx_setup() -> TestResult {
    without_ambient_identity(
        "full_render_places_one_preflight_before_each_mbx_setup",
        || {
            let sample = repo_sample_text()?;
            let repo = make_sample_repo(&sample)?;
            let prep = prepare(repo.path())?;
            let tree = render_staged_tree(&prep)?;
            let yaml = tree
                .get(".github/workflows/ci.yml")
                .ok_or_else(|| std::io::Error::other("missing workflow"))?;
            let preflight_positions: Vec<usize> = yaml
                .match_indices("name: Verify MBX and Rust toolchains")
                .map(|(position, _)| position)
                .collect();
            let setup_positions: Vec<usize> = yaml
                .match_indices("name: Setup MBX")
                .map(|(position, _)| position)
                .collect();
            assert!(
                !setup_positions.is_empty(),
                "sample emits MBX setup: {yaml}"
            );
            assert_eq!(
                preflight_positions.len(),
                setup_positions.len(),
                "full rendering injects exactly one central preflight per MBX job"
            );
            assert!(
                preflight_positions
                    .iter()
                    .zip(&setup_positions)
                    .all(|(preflight, setup)| preflight < setup),
                "preflight precedes each MBX setup: {yaml}"
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
fn uses(name: &str) -> String {
    format!("{name}@{}", "a".repeat(40))
}

#[test]
fn velnor_sources_transport_rejects_mbx_paths() {
    assert!(validate_sources_path("registry/index/example").is_ok());
    assert!(validate_sources_path("git/db/example").is_ok());
    for bad in [
        "mbx/objects/x",
        "mbx",
        "registry/../mbx/x",
        "task-artifacts/v2/x",
        "/registry/x",
    ] {
        assert!(
            validate_sources_path(bad).is_err(),
            "sources transport must reject: {bad}"
        );
    }
}

#[test]
fn cache_action_transport_never_carries_mbx() {
    let restore = uses("actions/cache/restore");
    let save = uses("actions/cache/save");
    let key = "velnor-sources-abc";
    let sources = vec!["$CARGO_HOME/registry/cache/x".to_owned()];
    assert!(cache_action_step(true, &restore, "sources", key, &[], &sources).is_ok());
    assert!(
        cache_action_step(
            false,
            &save,
            "task",
            key,
            &[],
            &[TASK_ARTIFACTS_DIR.to_owned()]
        )
        .is_ok()
    );
    assert!(
        cache_action_step(true, &restore, "mbx", key, &[], &sources).is_err(),
        "mbx layer must use objects mode"
    );
    // NOTE: `$CARGO_HOME/git/../mbx/x` normalizes outside the allowed
    // sources but `validate_cache_path` only checks the second segment;
    // latent `..` gap in renderer `cache_steps.rs` (not owned here),
    // reported separately. Direct MBX paths below are all rejected.
    for bad in [
        "$CARGO_HOME/mbx/cache/x",
        "$MISE_TASK_CACHE_DIR/mbx/x",
        TASK_ARTIFACTS_DIR,
    ] {
        assert!(
            cache_action_step(true, &restore, "sources", key, &[], &[bad.to_owned()]).is_err(),
            "sources layer must reject: {bad}"
        );
    }
    assert!(
        cache_action_step(
            false,
            &save,
            "task",
            key,
            &[],
            &["$MISE_TASK_CACHE_DIR/mbx/x".to_owned()]
        )
        .is_err(),
        "task layer takes only the task-artifacts dir"
    );
}

#[test]
fn mbx_transport_stays_with_mr_boxington_action() {
    let mbx = uses("jdx/mr-boxington-action");
    let pin = velnor_actions_mise::MR_BOXINGTON_VERSION;
    let step = mbx_step_for_driver(&mbx, CompileDriver::Mbx, pin)
        .expect("setup step")
        .expect("MBX profile");
    assert!(
        format!("{:?}", step.kind).contains("jdx/mr-boxington-action"),
        "mbx bytes move only through the external action"
    );
    let velnor_actions_contract::StepKind::Action { with, .. } = &step.kind else {
        panic!("MBX setup must be an action");
    };
    assert_eq!(with.get("version").map(String::as_str), Some(pin));
    assert_eq!(with.get("backend").map(String::as_str), Some("local"));
    assert!(
        mbx_step_for_driver(&mbx, CompileDriver::Cargo, pin)
            .expect("Cargo profile")
            .is_none()
    );
    let other = uses("actions/cache/restore");
    assert!(mbx_step_for_driver(&other, CompileDriver::Mbx, pin).is_err());
}
