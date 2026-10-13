//! Schema 2 generation on temp fixtures. Do not generate into the repository tree.

use velnor_actions_orchestrator::{
    ExecutionMode, migrate_config, prepare, render_staged_tree, render_staged_tree_with,
};
use velnor_actions_workflow_renderer::RenderedTree;

use crate::impl_common::{TestResult, config_with_branch, git, make_repo};

#[path = "schema2_feature_snapshots.rs"]
mod schema2_feature_snapshots;

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
    let product_release = required_file(&tree, ".github/workflows/product-release.yml")?;
    assert_product_release(product_release)?;
    for obsolete in [
        ".github/workflows/image-release.yml",
        ".github/workflows/macos-binary-release.yml",
        ".github/workflows/generator-release.yml",
    ] {
        assert!(
            tree.get(obsolete).is_none(),
            "obsolete workflow emitted: {obsolete}"
        );
    }
    assert_eq!(
        required_file(&tree, ".github/workflows/monitoring.yml")?,
        &marked(MONITORING)
    );
    Ok(())
}

fn assert_product_release(body: &str) -> TestResult {
    assert!(body.contains("name: Velnor product releases\n"), "{body}");
    assert!(body.contains("cancel-in-progress: false"), "{body}");
    assert!(body.contains("workflow_dispatch: {}"), "{body}");
    assert!(body.contains("cron: 17 * * * *"), "{body}");
    let ids = job_ids(body);
    for required in [
        "release-eligibility",
        "prepare-images",
        "build-images",
        "attest-images",
        "publish-images",
        "prepare-binary",
        "build-binary",
        "attest-binary",
        "publish-binary",
        "prepare-generator",
        "build-linux",
        "attest-linux",
        "build-macos",
        "attest-macos",
        "publish-generator",
    ] {
        assert!(ids.contains(&required), "missing job {required}: {ids:?}");
    }
    assert!(body.contains("needs.release-eligibility.outputs.source_sha"));
    assert!(body.contains("needs.release-eligibility.outputs.ci_attempt"));
    assert!(body.contains("needs.prepare-generator.outputs.action == 'build'"));

    let images = job_body(body, "build-images")?;
    assert!(images.contains("docker build --platform linux/amd64"));
    assert!(images.contains("images/runner/ubuntu-26.04"));
    assert!(images.contains("images/dind"));
    assert!(images.contains("docker save"));
    assert!(images.contains("sha256sum"));

    let binary = job_body(body, "build-binary")?;
    assert!(binary.contains("runs-on: macos-15"), "{binary}");
    assert!(binary.contains("cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --release -p velnor-runner-cli"));
    assert!(binary.contains("Mach-O"));
    assert!(binary.contains("arm64"));

    let linux = job_body(body, "build-linux")?;
    assert!(linux.contains("ELF"), "{linux}");
    assert!(linux.contains("x86-64"), "{linux}");
    let macos = job_body(body, "build-macos")?;
    assert!(macos.contains("Mach-O"), "{macos}");
    assert!(macos.contains("arm64"), "{macos}");

    let publisher = job_body(body, "publish-generator")?;
    assert!(
        publisher.contains("release_tag='generator-'") && publisher.contains("$release_source_sha"),
        "{publisher}"
    );
    assert!(publisher.contains("Setup pinned Mise"), "{publisher}");
    assert!(
        publisher.contains("Install pinned GitHub CLI"),
        "{publisher}"
    );
    assert!(publisher.contains("git/refs"), "{publisher}");
    assert!(publisher.contains("--signer-digest"), "{publisher}");
    assert!(publisher.contains("--source-digest"), "{publisher}");
    assert!(!publisher.contains("--clobber"), "{publisher}");
    assert!(!body.contains("pull_request"), "{body}");
    assert!(!body.contains("packages: write"), "{body}");
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
    assert!(
        local.contains("defaults:\n      run:\n        shell: bash -e {0}"),
        "{local}"
    );
    assert!(!hosted.contains("defaults:"), "{hosted}");
    assert_eq!(tool_lines(hosted), tool_lines(local));
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

fn tool_lines(body: &str) -> Vec<&str> {
    let mut in_steps = false;
    body.lines()
        .filter(|line| {
            let indent = line.bytes().take_while(|byte| *byte == b' ').count();
            let content = line.trim();
            if indent == 4 {
                in_steps = content == "steps:";
                return false;
            }
            in_steps && indent == 8 && (content.starts_with("run:") || content.starts_with("uses:"))
        })
        .collect()
}

#[test]
fn tool_lines_ignores_defaults_but_detects_step_differences() {
    let hosted = "    steps:\n      - name: run\n        run: cargo test\n";
    let scaled = "    defaults:\n      run:\n        shell: bash -e {0}\n    steps:\n      - name: run\n        run: cargo test\n";
    let changed = scaled.replace("cargo test", "cargo check");
    assert_eq!(tool_lines(hosted), tool_lines(scaled));
    assert_ne!(tool_lines(hosted), tool_lines(&changed));
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

const MONITORING: &str = r#"name: Scale set monitoring
"on":
  workflow_dispatch: {}
permissions:
  contents: read
jobs:
  scale-set-lane:
    name: Scale set lane
    runs-on: [velnor, ubuntu-26.04-scale-set]
    timeout-minutes: 30
    defaults:
      run:
        shell: bash -e {0}
    steps:
      - name: Run scale-set lane
        run: echo scale-set-lane
  queue-monitor:
    name: Queue monitor
    runs-on: ubuntu-26.04
    timeout-minutes: 10
    steps:
      - name: Watch admission
        run: echo queue-monitor
"#;
