//! End-to-end CLI tests for default byte parity, `[[stacks.rust.suites]]`,
//! `[workflow.dispatch]`, `preserve_workflows`, and `[agents] claude_md = "symlink"`.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::impl_cli_tmp::{
    cleanup, code, commit_all, fresh_tempdir, git_init, init_repo, install_consumer_manifest,
    spawn, write_workspace,
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
timeout_minutes = 60
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
timeout_minutes = 60
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

const CUSTOM_AUDIT_WORKFLOW: &[u8] = b"name: Custom Audit\non:\n  workflow_dispatch:\njobs:\n  audit:\n    runs-on: ubuntu-24.04\n    timeout-minutes: 10\n    steps:\n      - run: echo ok\n";

#[test]
fn default_schema1_config_emits_byte_identical_github_tree() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("suites-default-parity")?;
    init_repo(&tmp)?;
    write_workspace(&tmp, 1)?;
    commit_all(&tmp)?;

    let in_place = spawn(&["generate"], &[], &tmp)?;
    assert_eq!(
        code(&in_place),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&in_place.stderr)
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
    assert!(tmp.join(".github/workflows/ci.yml").is_file());

    let outer = fresh_tempdir("suites-default-preview")?;
    let preview = outer.join("preview");
    let preview_str = preview.to_str().ok_or("non-utf8 preview path")?;
    let gen_preview = spawn(&["generate", "--output-dir", preview_str], &[], &tmp)?;
    assert_eq!(
        code(&gen_preview),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&gen_preview.stderr)
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

fn verify_ci_yaml(ci_yaml: &str) -> Result<(), Box<dyn Error>> {
    assert!(
        ci_yaml.contains("  suite-smoke:\n"),
        "missing suite-smoke in ci.yml:\n{ci_yaml}"
    );
    assert!(
        ci_yaml.contains(
            "    strategy:\n      fail-fast: false\n      matrix:\n        shard:\n          - 1\n          - 2\n"
        ),
        "missing 2-shard matrix in ci.yml:\n{ci_yaml}"
    );
    assert!(
        ci_yaml.contains("--partition \\\"hash:${{ matrix.shard }}/2\\\""),
        "missing nextest partition arg in ci.yml:\n{ci_yaml}"
    );
    assert!(
        ci_yaml.contains("--profile smoke --run-ignored only"),
        "missing profile/run-ignored in ci.yml:\n{ci_yaml}"
    );
    assert!(
        ci_yaml.contains("NEXTEST_TEST_THREADS: \"4\""),
        "missing NEXTEST_TEST_THREADS in ci.yml:\n{ci_yaml}"
    );
    assert!(
        ci_yaml.contains("smoke-diff-${{ matrix.shard }}"),
        "missing smoke-diff shard artifact in ci.yml:\n{ci_yaml}"
    );
    let required_block = ci_yaml
        .split("  required:\n")
        .nth(1)
        .ok_or("required job block in ci.yml")?;
    assert!(
        required_block.contains("      - suite-smoke\n"),
        "Required job must need suite-smoke:\n{required_block}"
    );
    assert!(
        ci_yaml.contains("test -L .github/CLAUDE.md"),
        "Check generated files must verify symlink in ci.yml:\n{ci_yaml}"
    );
    assert!(
        ci_yaml.contains("readlink .github/CLAUDE.md | grep -qxF AGENTS.md"),
        "Check generated files must verify symlink target in ci.yml:\n{ci_yaml}"
    );
    Ok(())
}

fn verify_suites_yaml(suites_yaml: &str) {
    assert!(
        suites_yaml.contains("name: Suites\n"),
        "missing Suites workflow name:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("  schedule:\n    - cron: 0 2 * * *\n"),
        "missing schedule 0 2 * * * in suites.yml:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("  workflow_dispatch:\n    inputs:\n      suite:\n"),
        "missing workflow_dispatch suite input in suites.yml:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("      tier:\n"),
        "missing tier dispatch input in suites.yml:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("      filter:\n"),
        "missing filter dispatch input in suites.yml:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("  cancel-in-progress: false\n"),
        "missing cancel-in-progress: false in suites.yml:\n{suites_yaml}"
    );
    for job_key in [
        "  suite-build-full:\n",
        "  suite-full:\n",
        "  suite-build-capture:\n",
        "  suite-capture:\n",
    ] {
        assert!(
            suites_yaml.contains(job_key),
            "missing {job_key:?} in suites.yml:\n{suites_yaml}"
        );
    }
    assert!(
        suites_yaml.contains("(inputs.tier == 'full' || inputs.tier == 'all')"),
        "missing tier condition for full suite:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("inputs.tier == 'capture'"),
        "missing tier condition for capture suite:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("continue-on-error: true"),
        "missing continue-on-error: true on capture suite:\n{suites_yaml}"
    );
    assert!(
        suites_yaml.contains("VELNOR_SUITE_FILTERSET: ${{ inputs.filter }}"),
        "missing VELNOR_SUITE_FILTERSET binding:\n{suites_yaml}"
    );
}

#[cfg(unix)]
#[test]
fn full_suites_dispatch_symlink_and_preserve_workflows_end_to_end() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("suites-e2e-repo")?;
    setup_full_suites_repo(&tmp)?;

    let generated = spawn(&["generate"], &[], &tmp)?;
    assert_eq!(
        code(&generated),
        0,
        "velnor-actions generate failed:\n{}",
        String::from_utf8_lossy(&generated.stderr)
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
    verify_ci_yaml(&ci_yaml)?;

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
    assert!(
        check_out.status.success(),
        "Check generated files command failed (stdout={}, stderr={})",
        String::from_utf8_lossy(&check_out.stdout),
        String::from_utf8_lossy(&check_out.stderr)
    );

    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}
