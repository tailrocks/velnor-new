//! Schema 2 generation on temp fixtures. Do not generate into the repository tree.

use std::fs;

use velnor_actions_orchestrator::{
    ExecutionMode, migrate_config, prepare, render_staged_tree, render_staged_tree_with,
};
use velnor_actions_workflow_renderer::RenderedTree;

use crate::impl_common::{TestResult, config_with_branch, git, make_repo};
use monitoring_fixture::MONITORING;
use schema2_feature_snapshots::marked;

#[path = "impl_schema2_binary_release.rs"]
mod binary_release_tests;
#[path = "schema2_monitoring_fixture.rs"]
mod monitoring_fixture;
#[path = "schema2_named_check_lanes_tests.rs"]
mod named_check_lanes_tests;
#[path = "schema2_release_producer_assertions.rs"]
mod producer_assertions;
#[path = "schema2_feature_snapshots.rs"]
mod schema2_feature_snapshots;
#[path = "schema2_fixture_owner_export.rs"]
mod schema2_fixture_owner_export;
#[path = "schema2_product_release_snapshots.rs"]
mod schema2_product_release_snapshots;
use producer_assertions::{
    assert_image_producer as assert_image_producer_contract,
    assert_macos_producer as assert_macos_producer_contract,
};
#[path = "schema2_snapshot_capture.rs"]
mod schema2_snapshot_capture;
#[path = "schema2_workflow_snapshot_tests.rs"]
mod schema2_workflow_snapshot_tests;
#[path = "impl_schema2_routing_shell.rs"]
mod shell_tests;

const HOSTED_RUNS: &str = "runs-on: ubuntu-26.04";
const SCALE_RUNS: &str = "runs-on: [velnor, ubuntu-26.04-scale-set]";
const SCALE_REVERSED: &str = "runs-on: [ubuntu-26.04-scale-set, velnor]";

#[test]
fn schema1_omits_scale_set_selector() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let joined = join_files(&tree);
    assert!(
        !joined.contains("ubuntu-26.04-scale-set"),
        "schema 1 emitted a scale-set label"
    );
    assert!(
        !joined.contains("runs-on: [velnor"),
        "schema 1 emitted a scale-set selector"
    );
    Ok(())
}

#[test]
fn dependency_seed_uses_scale_set_without_hosted_cache_job() -> TestResult {
    let config = format!("{}\nmode = \"scale-set\"\n{}", execution_head(), profiles());
    let repo = make_repo(&config)?;
    fs::write(
        repo.path().join("mise.toml"),
        "wrappers = { cargo = { command = \"mbx\", env = { MBX_CARGO_SHIM_MODE = \"1\" } } }\n",
    )?;

    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let seed = job_body(ci, "dependency-seed")?;

    assert!(seed.contains(SCALE_RUNS), "{seed}");
    assert!(
        seed.contains("name: Compile test executables (seed)"),
        "{seed}"
    );
    assert!(seed.contains("name: Compile Clippy lane (seed)"), "{seed}");
    assert!(
        seed.contains("name: Compile documentation lane (seed)"),
        "{seed}"
    );
    assert!(
        !ci.contains("mbx-cache-write-hosted")
            && !ci.contains("mbx-cache-read-hosted")
            && !ci.contains("dependency-seed-hosted"),
        "MBX generation must not add a hosted cache job when the mode is scale-set. Output: {ci}"
    );
    assert!(
        job_ids(ci).iter().all(|id| {
            !id.starts_with("rust-") || job_body(ci, id).is_ok_and(|body| body.contains(SCALE_RUNS))
        }),
        "All Rust jobs must use the configured runner when the mode is scale-set. Output: {ci}"
    );
    Ok(())
}

#[test]
fn migrate_preview_is_write_and_stays_hosted() -> TestResult {
    let body = "schema = 1\n[workflow]\nname = \"Kept\"\ndefault_branch = \"testmain\"\nrunner_label = \"ubuntu-24.04\"\n[resources]\ncompiler_process_budget = 4\ntest_process_budget = 5\n[test_sharding]\ndefault_shards = 1\n[test_sharding.by_manifest]\n\"crates/demo/Cargo.toml\" = 2\n[stacks]\nignore = [\"tofu\"]\n[discovery]\nexclude = [\"vendor/**\"]\n[actions.overrides.\"actions/checkout\"]\nsha = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nversion = \"v1.2.3\"\n";
    let repo = make_repo(body)?;
    let root = repo.path();
    let path = root.join(".velnor/config.toml");
    let before = std::fs::read_to_string(&path)?;
    let preview = migrate_config(root, 2, false)?;
    assert_eq!(std::fs::read_to_string(&path)?, before);
    match migrate_config(root, 1, true) {
        Ok(_) => return Err("to 1 must fail".into()),
        Err(err) => {
            assert!(
                err.to_string().contains("unsupported_migration_target"),
                "{err}"
            );
        }
    }
    assert_eq!(std::fs::read_to_string(&path)?, before);
    let written = migrate_config(root, 2, true)?;
    assert_eq!(written, preview);
    assert_eq!(std::fs::read_to_string(&path)?, preview);
    assert!(preview.contains("schema = 2"), "{preview}");
    assert!(
        preview.contains("runner_label = \"ubuntu-24.04\""),
        "{preview}"
    );
    assert!(
        preview.contains("default_profile = \"hosted\""),
        "{preview}"
    );
    assert!(!preview.contains("mode ="), "{preview}");
    assert!(!preview.contains("profiles.ubuntu-24.04"), "{preview}");
    assert!(preview.contains("Kept"), "{preview}");
    assert!(preview.contains("vendor/**"), "{preview}");
    assert!(preview.contains("crates/demo/Cargo.toml"), "{preview}");
    assert!(
        preview.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        "{preview}"
    );
    let tree = render_staged_tree(&prepare(root)?)?;
    let joined = join_files(&tree);
    assert!(!joined.contains("ubuntu-26.04-scale-set"), "{joined}");
    assert!(!joined.contains("runs-on: [velnor"), "{joined}");
    assert!(joined.contains("runs-on: ubuntu-24.04"), "{joined}");
    Ok(())
}

#[test]
fn both_mode_splits_verification_and_keeps_release_hosted() -> TestResult {
    let repo = release_repo(&both_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let release = required_file(&tree, ".github/workflows/release.yml")?;
    let rust_ids: Vec<_> = job_ids(ci)
        .into_iter()
        .filter(|id| id.starts_with("rust-demo"))
        .collect();
    assert_eq!(rust_ids, ["rust-demo__hosted", "rust-demo__local"]);
    assert!(!tree.files.iter().any(|file| {
        file.path.starts_with(".github/actions/rust-demo-")
            && ["-lint/", "-tests/", "-docs/"]
                .iter()
                .any(|suffix| file.path.contains(suffix))
    }));
    assert!(
        tree.files
            .iter()
            .all(|file| { !file.path.starts_with(".github/actions/rust-demo") })
    );
    let shared = required_file(&tree, ".github/actions/declared-task-0/action.yml")?;
    assert!(shared.contains("shell: bash"), "{shared}");
    assert!(shared.contains("using: composite"), "{shared}");
    assert!(
        tree.get(".github/actions/rust-demo/action.yml").is_none(),
        "parallel Rust steps stay at workflow job scope"
    );
    assert_both_ci(ci)?;
    assert_hosted_release(release);
    Ok(())
}

#[test]
fn dispatch_mode_overrides_configured_mode() -> TestResult {
    let repo = make_repo(&hosted_schema2())?;
    let prep = prepare(repo.path())?;
    let hosted = render_staged_tree(&prep)?;
    let forced = render_staged_tree_with(&prep, Some(ExecutionMode::Both))?;
    let hosted_ci = required_file(&hosted, ".github/workflows/ci.yml")?;
    let forced_ci = required_file(&forced, ".github/workflows/ci.yml")?;
    assert!(job_ids(hosted_ci).contains(&"rust-demo"), "{hosted_ci}");
    assert!(!hosted_ci.contains("rust-demo__hosted"), "{hosted_ci}");
    assert!(!hosted_ci.contains("rust-demo__local"), "{hosted_ci}");
    assert!(!hosted_ci.contains("rust-demo-lint"), "{hosted_ci}");
    assert!(!hosted_ci.contains("rust-demo-tests"), "{hosted_ci}");
    assert!(!hosted_ci.contains("rust-demo-docs"), "{hosted_ci}");
    assert_both_ci(forced_ci)?;
    assert!(
        forced.get(".github/actions/rust-demo/action.yml").is_none(),
        "parallel Rust steps stay at workflow job scope"
    );
    Ok(())
}

fn assert_both_ci(ci: &str) -> TestResult {
    let ids = job_ids(ci);
    assert!(ids.contains(&"plan"), "{ids:?}");
    assert!(ids.contains(&"required"), "{ids:?}");
    assert!(!ids.contains(&"rust-demo"), "{ids:?}");
    let logical = "rust-demo";
    let hosted_id = format!("{logical}__hosted");
    let local_id = format!("{logical}__local");
    assert!(ids.contains(&hosted_id.as_str()), "{ids:?}");
    assert!(ids.contains(&local_id.as_str()), "{ids:?}");
    assert!(!ids.contains(&logical), "{ids:?}");
    assert!(!ids.contains(&"plan__hosted"), "{ids:?}");
    assert!(!ids.contains(&"required__local"), "{ids:?}");
    let required = job_body(ci, "required")?;
    let hosted = job_body(ci, &hosted_id)?;
    let local = job_body(ci, &local_id)?;
    for job in [hosted, local] {
        assert!(job.contains("./.github/actions/declared-task-0"), "{job}");
        assert!(job.contains("Wait for parallel checks"), "{job}");
        assert!(job.contains("wait-all:"), "{job}");
    }
    assert!(hosted.contains(HOSTED_RUNS), "{hosted}");
    assert!(local.contains(SCALE_RUNS), "{local}");
    assert!(!local.contains(SCALE_REVERSED), "{local}");
    assert!(!local.contains("runs-on: ubuntu-26.04\n"), "{local}");
    shell_tests::assert_scale_set_shell_and_same_steps(hosted, local)?;
    assert!(required.contains(&hosted_id), "{required}");
    assert!(required.contains(&local_id), "{required}");
    assert!(
        !ids.iter().any(|id| ["-lint", "-tests", "-docs"]
            .iter()
            .any(|suffix| id.contains(suffix))),
        "{ids:?}"
    );
    let plan = job_body(ci, "plan")?;
    let plan_runners = plan
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("runs-on:"))
        .collect::<Vec<_>>();
    assert_eq!(plan_runners, [HOSTED_RUNS], "plan job placement: {plan}");
    Ok(())
}

fn assert_hosted_release(release: &str) {
    assert_eq!(release.matches("release-publish:").count(), 1, "{release}");
    assert!(release.contains(HOSTED_RUNS), "{release}");
    assert!(!release.contains("__hosted"), "{release}");
    assert!(!release.contains("__local"), "{release}");
    assert!(!release.contains("ubuntu-26.04-scale-set"), "{release}");
    assert!(!release.contains("runs-on: [velnor"), "{release}");
}

fn job_ids(yaml: &str) -> Vec<&str> {
    jobs_section(yaml)
        .lines()
        .filter(|line| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        .map(|line| line[2..].trim_end_matches(':'))
        .collect()
}

pub(super) fn job_body<'a>(yaml: &'a str, id: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let section = jobs_section(yaml);
    let header = format!("  {id}:");
    let mut offset = 0;
    let mut body_start = None;
    for line in section.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let is_job = bare.starts_with("  ") && !bare.starts_with("   ");
        if let Some(start) = body_start {
            if is_job {
                return Ok(&section[start..offset]);
            }
        } else if bare == header {
            body_start = Some(offset + line.len());
        }
        offset += line.len();
    }
    body_start
        .map(|start| &section[start..])
        .ok_or_else(|| format!("missing job {id}").into())
}

fn jobs_section(yaml: &str) -> &str {
    yaml.split_once("jobs:\n").map_or("", |(_, rest)| rest)
}

fn join_files(tree: &RenderedTree) -> String {
    tree.files
        .iter()
        .map(|file| file.bytes.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn required_file<'a>(
    tree: &'a RenderedTree,
    path: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    tree.get(path)
        .ok_or_else(|| format!("missing {path}").into())
}

fn release_repo(config: &str) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
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
    git(&["commit", "-m", "fixture"], root)?;
    Ok(repo)
}

fn both_config() -> String {
    format!(
        "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = true\nregistry = \"crates-io\"\npackages = [\"demo\"]\n{}\nmode = \"both\"\n{}",
        execution_ids(),
        profiles()
    )
}

fn hosted_schema2() -> String {
    format!("{}\nmode = \"hosted\"\n{}", execution_head(), profiles())
}

pub(super) fn workflow_config() -> String {
    format!(
        "{}\nmode = \"hosted\"\nworkflows = [\"qualification\", \"image_release\", \"macos_binary_release\", \"generator_release\", \"monitoring\"]\n{}",
        execution_head(),
        profiles()
    )
}

fn execution_head() -> String {
    format!(
        "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{}",
        execution_ids()
    )
}

fn execution_ids() -> &'static str {
    "[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\""
}

fn profiles() -> &'static str {
    "[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\"]\nplatform = \"linux/amd64\""
}

fn assert_image_producer(body: &str) -> TestResult {
    assert_image_producer_contract(body)?;
    schema2_product_release_snapshots::assert_full_workflow_verifier_environment(
        body,
        &["Verify Rust channel manifest"],
        &[
            (
                "Install pinned Rust for the resource probe",
                "GITHUB_TOKEN",
                "${{ github.token }}",
            ),
            ("Publish GitHub release", "GH_TOKEN", "${{ github.token }}"),
        ],
    )?;
    Ok(())
}

fn assert_macos_producer(body: &str) -> TestResult {
    assert_macos_producer_contract(body)?;
    schema2_product_release_snapshots::assert_full_workflow_verifier_environment(
        body,
        &["Verify Rust channel manifest"],
        &[("Publish GitHub release", "GH_TOKEN", "${{ github.token }}")],
    )?;
    Ok(())
}
