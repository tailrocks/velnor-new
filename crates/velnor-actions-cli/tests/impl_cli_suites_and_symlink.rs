//! End-to-end CLI tests for default byte parity, `[[stacks.rust.suites]]`,
//! `[workflow.dispatch]`, `preserve_workflows`, and `[agents] claude_md = "symlink"`.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::impl_cli_tmp::{
    MiseDiagnosticContext, cleanup, code, commit_all, fresh_tempdir, git_init, init_repo,
    install_consumer_manifest, mise_failure_diagnostics, mise_path_snapshot, spawn, tool_context,
    write_workspace,
};

const FULL_SUITES_CONFIG: &str = r#"schema = 1

[workflow]
name = "CI"
default_branch = "main"
preserve_workflows = [".github/workflows/custom-audit.yml"]

[workflow.dispatch.inputs.tier]
type = "choice"
options = ["smoke", "full", "narrow", "capture", "all"]
default = "smoke"

[workflow.dispatch.inputs.filter]
type = "string"
default = ""

[agents]
claude_md = "symlink"

[test_sharding]
default_shards = 2

[stacks.rust]
test_runner = "cargo_nextest"

[[stacks.rust.suites]]
id = "smoke"
profile = "smoke"
run_ignored = "only"
partitions = 2
test_threads = 4
triggers = ["pull_request", "push"]
required = true
artifacts = [
  { name = "smoke-diff-{shard}", paths = ["target/snapshots/actual", "target/snapshots/diff"], when = "failure", retention_days = 14 },
]

[[stacks.rust.suites]]
id = "full"
profile = "full"
run_ignored = "only"
partitions = 12
build = "archive"
timeout_minutes = 5
test_threads = 4
triggers = ["schedule", "workflow_dispatch"]
schedule = ["0 2 * * *"]
dispatch_inputs = { tier = ["full", "all"] }
filter_input = "filter"
artifacts = [
  { name = "full-diff-{shard}", paths = ["target/snapshots/actual"], when = "failure", retention_days = 14 },
]

[[stacks.rust.suites]]
id = "capture"
profile = "full"
run_ignored = "only"
partitions = 12
build = "archive"
timeout_minutes = 5
triggers = ["workflow_dispatch"]
allow_failure = true
dispatch_inputs = { tier = ["capture"] }
filter_input = "filter"
artifacts = [
  { name = "capture-{shard}", paths = ["target/snapshots/actual"], when = "always", retention_days = 14 },
]
"#;

const NEXTEST_CONFIG: &str = r"[profile.ci]
fail-fast = false

[profile.smoke]
fail-fast = false

[profile.full]
fail-fast = false
retries = 2
";

const CUSTOM_AUDIT_WORKFLOW: &[u8] = b"name: Custom Audit\non:\n  workflow_dispatch:\njobs:\n  audit:\n    runs-on: ubuntu-24.04\n    timeout-minutes: 5\n    steps:\n      - run: echo ok\n";

#[test]
fn default_schema1_config_emits_byte_identical_github_tree() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("suites-default-parity")?;
    init_repo(&tmp)?;
    write_workspace(&tmp, 1)?;
    commit_all(&tmp)?;

    let tool_state_before = mise_path_snapshot(&tmp, MiseDiagnosticContext::Inherited);
    let before_first = tool_context::diagnostic();
    let in_place = spawn(&["generate"], &[], &tmp)?;
    let after_first = tool_context::diagnostic();
    assert_eq!(
        code(&in_place),
        0,
        "in-place generate failed (stderr={})\n{before_first}\n{after_first}\n{}",
        String::from_utf8_lossy(&in_place.stderr),
        mise_failure_diagnostics(
            &tmp,
            MiseDiagnosticContext::Inherited,
            &[],
            Some(&tool_state_before)
        )
    );

    let claude = tmp.join(".github/CLAUDE.md");
    let meta = fs::symlink_metadata(&claude)?;
    assert!(
        !meta.is_symlink(),
        "default CLAUDE.md must be a regular file"
    );
    assert!(meta.is_file());
    assert_eq!(fs::read(&claude)?, b"@AGENTS.md\n");
    assert!(!tmp.join(".github/workflows/suites.yml").exists());
    let default_ci = fs::read_to_string(tmp.join(".github/workflows/ci.yml"))?;
    assert!(
        !default_ci.contains("--partition"),
        "default_shards = 1 must not emit --partition in ci.yml"
    );
    let default_manifest =
        fs::read_to_string(tmp.join(".github/velnor/task-execution-manifest-v1.json"))?;
    assert!(
        !default_manifest.contains("--partition"),
        "default_shards = 1 must not emit --partition in task-execution-manifest-v1.json"
    );

    let outer = fresh_tempdir("suites-default-preview")?;
    let preview = outer.join("preview");
    let preview_str = preview.to_str().ok_or("non-utf8 preview path")?;
    let gen_preview = spawn(&["generate", "--output-dir", preview_str], &[], &tmp)?;
    let on_preview_result = tool_context::diagnostic();
    assert_eq!(
        code(&gen_preview),
        0,
        "preview generate failed (stderr={})\n{before_first}\n{after_first}\n{on_preview_result}\n{}",
        String::from_utf8_lossy(&gen_preview.stderr),
        mise_failure_diagnostics(
            &tmp,
            MiseDiagnosticContext::Inherited,
            &[],
            Some(&tool_state_before)
        )
    );

    let diff = Command::new("diff")
        .args(["-r", "--brief"])
        .arg(tmp.join(".github"))
        .arg(preview.join(".github"))
        .output()?;
    assert!(
        diff.status.success(),
        "default preview diverged from committed .github: {}",
        String::from_utf8_lossy(&diff.stdout)
    );

    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

fn setup_full_suites_repo(repo: &Path) -> Result<(), Box<dyn Error>> {
    git_init(repo)?;
    write_workspace(repo, 1)?;
    fs::create_dir_all(repo.join(".velnor"))?;
    fs::write(repo.join(".velnor/config.toml"), FULL_SUITES_CONFIG)?;
    install_consumer_manifest(repo)?;
    fs::create_dir_all(repo.join(".config"))?;
    fs::write(repo.join(".config/nextest.toml"), NEXTEST_CONFIG)?;
    let wf_dir = repo.join(".github/workflows");
    fs::create_dir_all(&wf_dir)?;
    fs::write(wf_dir.join("custom-audit.yml"), CUSTOM_AUDIT_WORKFLOW)?;
    fs::write(wf_dir.join("obsolete.yml"), b"name: Obsolete\n")?;
    commit_all(repo)?;
    Ok(())
}

fn verify_ci_yaml(ci_yaml: &str, manifest_json: &str) -> Result<(), Box<dyn Error>> {
    assert!(
        manifest_json.contains("\"--no-tests\",\"warn\",\"--partition\",\"hash:1/2\"")
            && manifest_json.contains("\"--no-tests\",\"warn\",\"--partition\",\"hash:2/2\""),
        "default_shards = 2 must emit --no-tests warn --partition hash:1/2 and hash:2/2:\n{manifest_json}"
    );
    for needle in [
        "  suite-smoke:\n",
        "    strategy:\n      fail-fast: false\n      matrix:\n        shard:\n          - 1\n          - 2\n",
        "aqua:nextest-rs/nextest/cargo-nextest@",
        "--no-tests warn --partition \\\"hash:$VELNOR_SUITE_SHARD/2\\\" --profile smoke --run-ignored only",
        "VELNOR_SUITE_SHARD: ${{ matrix.shard }}",
        "NEXTEST_TEST_THREADS: \"4\"",
        "name: velnor-suite-r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}-smoke-diff-${{ matrix.shard }}",
        "test -L .github/CLAUDE.md",
        "readlink .github/CLAUDE.md | grep -qxF AGENTS.md",
    ] {
        assert!(
            ci_yaml.contains(needle),
            "missing {needle:?} in ci.yml:\n{ci_yaml}"
        );
    }
    let required_block = ci_yaml
        .split("  required:\n")
        .nth(1)
        .ok_or("required job block in ci.yml")?;
    assert!(
        required_block.contains("      - suite-smoke\n"),
        "Required job must need suite-smoke:\n{required_block}"
    );
    Ok(())
}

const EXPECTED_SUITES_HEADER: &str = "\
name: Suites
\"on\":
  workflow_dispatch:
    inputs:
      filter:
        required: false
        type: string
        default: \"\"
      tier:
        required: false
        type: choice
        default: smoke
        options:
          - smoke
          - full
          - narrow
          - capture
          - all
  schedule:
    - cron: 0 2 * * *
permissions:
  contents: read
";

fn verify_suites_yaml(suites_yaml: &str) {
    assert!(
        suites_yaml.contains(EXPECTED_SUITES_HEADER),
        "suites.yml header/triggers mismatch:\n{suites_yaml}"
    );
    assert!(
        !suites_yaml.contains("      suite:\n"),
        "auto suite input must be omitted when all suites declare dispatch_inputs:\n{suites_yaml}"
    );
    for needle in [
        "  cancel-in-progress: false\n",
        "  suite-build-full:\n",
        "  suite-full:\n",
        "  suite-build-capture:\n",
        "  suite-capture:\n",
        "    if: (github.event_name == 'workflow_dispatch' && (inputs.tier == 'full' || inputs.tier == 'all')) || (github.event_name == 'schedule' && github.event.schedule == '0 2 * * *')\n",
        "    if: github.event_name == 'workflow_dispatch' && inputs.tier == 'capture'\n    continue-on-error: true\n",
        "--archive-file \\\"$RUNNER_TEMP/velnor/suite-archive-full/archive.tar.zst\\\" --workspace-remap . --extract-to . --extract-overwrite --no-tests warn --partition \\\"hash:$VELNOR_SUITE_SHARD/12\\\" --profile full --run-ignored only",
        "VELNOR_SUITE_FILTERSET: ${{ inputs.filter }}",
        "VELNOR_SUITE_SHARD: ${{ matrix.shard }}",
        "name: velnor-suite-r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}-full-diff-${{ matrix.shard }}",
        "name: velnor-suite-r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}-capture-${{ matrix.shard }}",
    ] {
        assert!(
            suites_yaml.contains(needle),
            "missing {needle:?} in suites.yml:\n{suites_yaml}"
        );
    }
}

#[cfg(unix)]
#[test]
fn full_suites_dispatch_symlink_and_preserve_workflows_end_to_end() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("suites-e2e-repo")?;
    setup_full_suites_repo(&tmp)?;

    let tool_state_before = mise_path_snapshot(&tmp, MiseDiagnosticContext::Inherited);
    let before_first = tool_context::diagnostic();
    let generated = spawn(&["generate"], &[], &tmp)?;
    let after_first = tool_context::diagnostic();
    assert_eq!(
        code(&generated),
        0,
        "in-place generate failed (stderr={})\n{before_first}\n{after_first}\n{}",
        String::from_utf8_lossy(&generated.stderr),
        mise_failure_diagnostics(
            &tmp,
            MiseDiagnosticContext::Inherited,
            &[],
            Some(&tool_state_before)
        )
    );

    let claude = tmp.join(".github/CLAUDE.md");
    assert!(
        fs::symlink_metadata(&claude)?.is_symlink(),
        ".github/CLAUDE.md must be a symlink"
    );
    assert_eq!(fs::read_link(&claude)?, Path::new("AGENTS.md"));
    assert_eq!(
        fs::read(tmp.join(".github/workflows/custom-audit.yml"))?,
        CUSTOM_AUDIT_WORKFLOW
    );
    assert!(!tmp.join(".github/workflows/obsolete.yml").exists());

    let ci_yaml = fs::read_to_string(tmp.join(".github/workflows/ci.yml"))?;
    let manifest_json =
        fs::read_to_string(tmp.join(".github/velnor/task-execution-manifest-v1.json"))?;
    verify_ci_yaml(&ci_yaml, &manifest_json)?;

    let suites_yaml = fs::read_to_string(tmp.join(".github/workflows/suites.yml"))?;
    verify_suites_yaml(&suites_yaml);

    let outer = fresh_tempdir("suites-e2e-preview")?;
    let preview = outer.join("preview");
    let preview_str = preview.to_str().ok_or("non-utf8 preview path")?;
    let binary = env!("CARGO_BIN_EXE_velnor-actions");
    let check_script = format!(
        "\"{binary}\" generate --output-dir \"{preview_str}\" && test -L .github/CLAUDE.md && test -L \"{preview_str}/.github/CLAUDE.md\" && readlink .github/CLAUDE.md | grep -qxF AGENTS.md && readlink \"{preview_str}/.github/CLAUDE.md\" | grep -qxF AGENTS.md && diff -r --brief .github \"{preview_str}/.github\""
    );
    let check_out = Command::new("sh")
        .args(["-c", &check_script])
        .current_dir(&tmp)
        .output()?;
    let on_preview_result = tool_context::diagnostic();
    assert!(
        check_out.status.success(),
        "preview workflow check failed (stdout={}, stderr={})\n{before_first}\n{after_first}\n{on_preview_result}\n{}",
        String::from_utf8_lossy(&check_out.stdout),
        String::from_utf8_lossy(&check_out.stderr),
        mise_failure_diagnostics(
            &tmp,
            MiseDiagnosticContext::Inherited,
            &[],
            Some(&tool_state_before)
        )
    );

    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}
