//! Schema 2 generation on temp fixtures. Do not generate into the repository tree.

use velnor_actions_contract_config::ExecutionMode;
use velnor_actions_orchestrator_generation::generate::{
    render_staged_tree, render_staged_tree_with,
};
use velnor_actions_orchestrator_generation::prepare::prepare;
use velnor_actions_orchestrator_generation::routing::migrate_config;
use velnor_actions_workflow_tree::RenderedTree;

use crate::impl_common::{TestResult, config_with_branch, git, make_repo};
use monitoring_fixture::MONITORING;

#[path = "schema2_monitoring_fixture.rs"]
mod monitoring_fixture;
#[path = "schema2_named_check_lanes_tests.rs"]
mod named_check_lanes_tests;
#[path = "schema2_feature_snapshots.rs"]
mod schema2_feature_snapshots;
#[path = "schema2_generator_release_snapshots.rs"]
mod schema2_generator_release_snapshots;
#[path = "schema2_release_snapshots.rs"]
mod schema2_release_snapshots;
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
        joined.contains("runs-on: ubuntu-26.04"),
        "schema 1 default hosted jobs must use the supported Ubuntu 26 profile"
    );
    assert!(
        !joined.contains("ubuntu-24.04"),
        "schema 1 emitted an Ubuntu 24 selector"
    );
    assert!(
        !joined.contains("ubuntu-26.04-scale-set"),
        "schema 1 emitted a scale-set label"
    );
    assert!(
        !joined.contains("runs-on: [velnor"),
        "schema 1 emitted a scale-set selector"
    );
    assert!(
        !joined.contains("artifact-build"),
        "schema 1 emitted a schema-2 artifact task lane"
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
    let shared = required_file(&tree, ".github/actions/rust-demo/action.yml")?;
    assert!(shared.contains("shell: bash"), "{shared}");
    assert!(shared.contains("using: composite"), "{shared}");
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
    assert_both_ci(forced_ci)?;
    Ok(())
}

#[test]
fn schema2_workflows_match_expected_bytes() -> TestResult {
    let repo = make_repo(&workflow_config())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let qualification = required_file(&tree, ".github/workflows/qualification.yml")?;
    assert_eq!(
        qualification,
        &marked(schema2_feature_snapshots::QUALIFICATION)
    );
    let image = required_file(&tree, ".github/workflows/image-release.yml")?;
    let macos = required_file(&tree, ".github/workflows/macos-binary-release.yml")?;
    assert_eq!(image, &marked(schema2_release_snapshots::IMAGE_RELEASE));
    assert_eq!(macos, &marked(schema2_release_snapshots::MACOS_RELEASE));
    assert_image_producer(image)?;
    assert_macos_producer(macos)?;
    assert_eq!(
        required_file(&tree, ".github/workflows/monitoring.yml")?,
        &marked(MONITORING)
    );
    schema2_generator_release_snapshots::assert_rendered(&tree)?;
    Ok(())
}

#[test]
fn committed_release_files_match_schema2_bytes() -> TestResult {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    for (name, expected) in [
        (
            "image-release.yml",
            schema2_release_snapshots::IMAGE_RELEASE,
        ),
        (
            "macos-binary-release.yml",
            schema2_release_snapshots::MACOS_RELEASE,
        ),
    ] {
        let path = root.join(".github/workflows").join(name);
        let body = std::fs::read_to_string(&path)?;
        assert_eq!(body, marked(expected), "{}", path.display());
    }
    schema2_generator_release_snapshots::assert_committed(&root)?;
    Ok(())
}

fn assert_both_ci(ci: &str) -> TestResult {
    let ids = job_ids(ci);
    assert!(ids.contains(&"plan"), "{ids:?}");
    assert!(ids.contains(&"required"), "{ids:?}");
    assert!(ids.contains(&"rust-demo__hosted"), "{ids:?}");
    assert!(ids.contains(&"rust-demo__local"), "{ids:?}");
    assert!(!ids.contains(&"rust-demo"), "{ids:?}");
    assert!(!ids.contains(&"plan__hosted"), "{ids:?}");
    assert!(!ids.contains(&"required__local"), "{ids:?}");
    let hosted = job_body(ci, "rust-demo__hosted")?;
    let local = job_body(ci, "rust-demo__local")?;
    assert!(hosted.contains(HOSTED_RUNS), "{hosted}");
    assert!(local.contains(SCALE_RUNS), "{local}");
    assert!(!local.contains(SCALE_REVERSED), "{local}");
    assert!(!local.contains("runs-on: ubuntu-26.04\n"), "{local}");
    shell_tests::assert_scale_set_shell_and_same_steps(hosted, local);
    let plan = job_body(ci, "plan")?;
    assert!(plan.contains(HOSTED_RUNS), "{plan}");
    assert!(!plan.contains("ubuntu-26.04-scale-set"), "{plan}");
    let required = job_body(ci, "required")?;
    assert!(required.contains("rust-demo__hosted"), "{required}");
    assert!(required.contains("rust-demo__local"), "{required}");
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
        "schema = 2\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n{}\nmode = \"both\"\n{}",
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

fn marked(body: &str) -> String {
    format!(
        "# Generated by Velnor Actions {}; edit .velnor/config.toml and regenerate.\n{body}",
        env!("CARGO_PKG_VERSION")
    )
}

fn assert_image_producer(body: &str) -> TestResult {
    assert!(body.contains("docker build --platform linux/amd64"));
    assert!(body.contains("images/runner/ubuntu-26.04"));
    assert!(body.contains("images/dind"));
    assert!(body.contains("{{.Architecture}}"));
    assert!(body.contains("= amd64"));
    assert!(body.contains("docker save"));
    assert!(body.contains("sha256sum"));
    assert!(
        body.contains("actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8")
    );
    assert!(body.contains("gh release create"));
    assert!(body.contains("runner-${GITHUB_SHA}"));
    assert!(body.contains(HOSTED_RUNS));
    assert_release_permissions(body, "attest-images", "publish-images")?;
    assert!(!body.contains("echo release"));
    assert!(!body.contains("inputs:"));
    assert!(!body.contains("packages:"));
    assert!(!body.contains("CARGO_REGISTRY_TOKEN"));
    assert!(!body.contains("secrets:"));
    assert!(!body.contains("pull_request"));
    Ok(())
}

fn assert_macos_producer(body: &str) -> TestResult {
    let build = job_body(body, "build-binary")?;
    assert!(build.contains("runs-on: macos-15"), "{build}");
    assert!(!build.contains("ubuntu"), "{build}");
    assert!(build.contains("jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5"));
    assert!(build.contains("rust@1.98.1"), "{build}");
    assert!(!body.contains("ubuntu"));
    assert!(body.contains(
        "cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --release -p velnor-runner-cli"
    ));
    assert!(body.contains("Mach-O"));
    assert!(body.contains("arm64"));
    assert!(body.contains("shasum -a 256"));
    assert!(
        body.contains("actions/attest-build-provenance@4d101475d8b20a2381f78447822ac1eab6504dd8")
    );
    assert!(body.contains("gh release create"));
    assert!(body.contains("binary-${GITHUB_SHA}"));
    assert_release_permissions(body, "attest-binary", "publish-binary")?;
    assert!(!body.contains("echo release"));
    assert!(!body.contains("inputs:"));
    assert!(!body.contains("packages:"));
    assert!(!body.contains("CARGO_REGISTRY_TOKEN"));
    Ok(())
}

fn assert_release_permissions(body: &str, attest: &str, publish: &str) -> TestResult {
    assert_eq!(body.matches("id-token: write").count(), 1, "{body}");
    assert_eq!(body.matches("contents: write").count(), 1, "{body}");
    assert!(body.contains("workflow_dispatch: {}"), "{body}");
    let attest_body = job_body(body, attest)?;
    let publish_body = job_body(body, publish)?;
    assert!(attest_body.contains("id-token: write"), "{attest_body}");
    assert!(!attest_body.contains("contents: write"), "{attest_body}");
    assert!(!attest_body.contains("packages:"), "{attest_body}");
    assert!(publish_body.contains("contents: write"), "{publish_body}");
    assert!(
        publish_body.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish_body}"
    );
    assert!(publish_body.contains("actions/checkout@"), "{publish_body}");
    assert!(
        publish_body.contains(r#"-R \"${GITHUB_REPOSITORY}\""#),
        "{publish_body}"
    );
    assert!(!attest_body.contains("GH_TOKEN"), "{attest_body}");
    assert!(!publish_body.contains("id-token:"), "{publish_body}");
    Ok(())
}
