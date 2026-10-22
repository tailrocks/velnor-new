//! Playwright browser provisioning for `[stacks.bun]` members (hard decision 6).

use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_bun::{LOCK, MISE, bun_config, write_workspace};
use crate::impl_common::{TestResult, git, make_repo};
use crate::impl_schema2_routing::{job_body, required_file};

pub(crate) fn core_integrity() -> String {
    format!("sha512-{}==", "A".repeat(86))
}

fn playwright_repo(
    lock_integrity: Option<&str>,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    playwright_repo_with(
        "[stacks.bun.members.\"chainargos-docs\"]\nbrowsers = [\"chromium\"]\n",
        lock_integrity,
    )
}

/// The Playwright workspace with `extra` `[stacks.bun]` configuration.
pub(crate) fn playwright_repo_with(
    extra: &str,
    lock_integrity: Option<&str>,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let config = bun_config(extra);
    let repo = make_repo(&config)?;
    write_workspace(repo.path(), MISE)?;
    let lock = LOCK
        .replace(
            r#""docs": {"name": "chainargos-docs", "dependencies": {"@chainargos/ui": "workspace:*"}},"#,
            r#""docs": {"name": "chainargos-docs", "dependencies": {"@chainargos/ui": "workspace:*"}, "devDependencies": {"@playwright/test": "1.64.1"}},"#,
        )
        .replace(
            r#""packages": {},"#,
            &format!(
                r#""packages": {{"@playwright/test": ["@playwright/test@1.64.1", "", {{}}, "sha512-x"], "playwright-core": ["playwright-core@1.64.1", "", {{}}, "{}"]}},"#,
                core_integrity()
            ),
        );
    fs::write(repo.path().join("frontend/bun.lock"), lock)?;
    if let Some(integrity) = lock_integrity {
        let archive = |directory: &str, digit: char, browsers: &str| {
            format!(
                "[[archives]]\nversion = \"1.64.1\"\ncore_integrity = \"{integrity}\"\nbrowsers = {browsers}\n\
directory = \"{directory}\"\nurl = \"https://cdn.playwright.dev/builds/{directory}.zip\"\n\
sha256 = \"{}\"\nbytes = 1000\n",
                digit.to_string().repeat(64)
            )
        };
        fs::write(
            repo.path().join(".velnor/playwright.lock"),
            format!(
                "schema = 1\nplatform = \"ubuntu26.04-x64\"\n{}{}",
                archive("chromium-1248", 'a', "[\"chromium\"]"),
                archive("ffmpeg-1013", 'b', "[\"chromium\"]")
            ),
        )?;
    }
    git(&["add", "."], repo.path())?;
    Ok(repo)
}

#[test]
fn browsers_install_from_the_lock_before_the_scripts_with_a_closed_environment() -> TestResult {
    let repo = playwright_repo(Some(&core_integrity()))?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let docs = job_body(ci, "bun-chainargos-docs")?;
    let install = docs
        .find("name: Install Playwright browsers")
        .ok_or("playwright step")?;
    assert!(
        install > docs.find("name: Install dependencies").ok_or("deps")?,
        "{docs}"
    );
    assert!(
        install < docs.find("name: Typecheck").ok_or("script")?,
        "{docs}"
    );
    let step = docs[install..].split("- name:").next().ok_or("body")?;
    let step = &step.replace('"', "");
    assert!(step.contains("VELNOR_PLAYWRIGHT_VERSION: 1.64.1"), "{step}");
    assert!(step.contains("VELNOR_PLAYWRIGHT_ROOT: frontend"), "{step}");
    assert!(
        step.contains(&format!(
            "VELNOR_PLAYWRIGHT_PINS: chromium-1248:{},ffmpeg-1013:{}",
            "a".repeat(64),
            "b".repeat(64)
        )),
        "{step}"
    );
    assert!(
        !step.contains("github.token") && !step.contains("secrets."),
        "{step}"
    );
    let typecheck = docs.split("name: Typecheck").nth(1).ok_or("typecheck")?;
    let typecheck = &typecheck.replace('"', "");
    for expected in [
        "PLAYWRIGHT_BROWSERS_PATH: ${{ runner.temp }}/velnor/ms-playwright",
        "PLAYWRIGHT_HOST_PLATFORM_OVERRIDE: ubuntu26.04-x64",
        "PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD: 1",
    ] {
        assert!(typecheck.contains(expected), "{expected}: {typecheck}");
    }
    let ui = job_body(ci, "bun-chainargos-ui")?;
    assert!(
        !ui.contains("Playwright") && !ui.contains("PLAYWRIGHT"),
        "{ui}"
    );
    Ok(())
}

#[test]
fn browsers_fail_closed_without_a_matching_lock() -> TestResult {
    let fails = |repo: tempfile::TempDir, code: &str| -> TestResult {
        let error = prepare(repo.path()).and_then(|plan| render_staged_tree(&plan));
        let text = error.err().ok_or("expected failure")?.to_string();
        assert!(text.contains(code), "{code}: {text}");
        Ok(())
    };
    fails(playwright_repo(None)?, "playwright_lock_missing")?;
    let other = format!("sha512-{}==", "B".repeat(86));
    fails(
        playwright_repo(Some(&other))?,
        "playwright_lock_drift:1.64.1",
    )?;
    // A member that declares browsers without Playwright in the lock.
    let repo = make_repo(&bun_config(
        "[stacks.bun.members.\"chainargos-docs\"]\nbrowsers = [\"chromium\"]\n",
    ))?;
    write_workspace(repo.path(), MISE)?;
    fails(repo, "bun_playwright_missing:chainargos-docs")?;
    Ok(())
}

#[test]
fn only_the_suite_that_declares_browsers_installs_them() -> TestResult {
    let repo = playwright_repo_with(
        "[stacks.bun.members.\"chainargos-docs\".suites.browser]\nscripts = [\"test\"]\nbrowsers = [\"chromium\"]\n",
        Some(&core_integrity()),
    )?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    let ci = required_file(&tree, ".github/workflows/ci.yml")?;
    let member = job_body(ci, "bun-chainargos-docs")?;
    assert!(!member.contains("Install Playwright browsers"), "{member}");
    assert!(!member.contains("PLAYWRIGHT_BROWSERS_PATH"), "{member}");
    let suite = job_body(ci, "bun-chainargos-docs--browser")?;
    let install = suite
        .find("name: Install Playwright browsers")
        .ok_or("install")?;
    let test = suite.find("name: Test\n").ok_or("test")?;
    assert!(install < test, "{suite}");
    assert!(
        suite.contains("PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD"),
        "{suite}"
    );
    assert!(suite.contains("chromium-1248:"), "{suite}");
    Ok(())
}

const BROWSER_SUITE: &str = "[stacks.bun.members.\"chainargos-docs\".suites.browser]\n\
scripts = [\"test:browser\"]\nbrowsers = [\"chromium\"]\ntimeout_minutes = 25\n";

/// The Playwright workspace with a `test:browser` script and a sharded suite.
fn sharded_repo(
    playwright: &str,
    script: &str,
) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let repo = playwright_repo_with(
        &format!(
            "{BROWSER_SUITE}[stacks.bun.members.\"chainargos-docs\".suites.browser.playwright]\n{playwright}"
        ),
        Some(&core_integrity()),
    )?;
    fs::write(
        repo.path().join("frontend/docs/package.json"),
        format!(
            r#"{{"name":"chainargos-docs","scripts":{{"typecheck":"tsc","test":"vitest run","test:browser":"{script}"}}}}"#
        ),
    )?;
    git(&["add", "."], repo.path())?;
    Ok(repo)
}

fn sharded_ci(playwright: &str, script: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = sharded_repo(playwright, script)?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
}

#[test]
fn a_sharded_suite_emits_a_project_by_shard_include_matrix() -> TestResult {
    let ci = sharded_ci(
        "projects = [\"chromium-desktop\", \"chromium-mobile\"]\nshards = 2\nmax_parallel = 3\n",
        "playwright test",
    )?;
    let job = job_body(&ci, "bun-chainargos-docs--browser")?;
    let strategy = "    strategy:\n      fail-fast: false\n      max-parallel: 3\n      matrix:\n        include:\n";
    assert!(job.contains(strategy), "{job}");
    for (leg, project, shard) in [
        (1, "chromium-desktop", 1),
        (2, "chromium-desktop", 2),
        (3, "chromium-mobile", 1),
        (4, "chromium-mobile", 2),
    ] {
        let entry = format!(
            "          - leg: {leg}\n            project: {project}\n            shard: {shard}\n            total: 2\n"
        );
        assert!(job.contains(&entry), "{entry}: {job}");
    }
    assert!(job.contains("timeout-minutes: 25"), "{job}");
    assert!(
        job.contains(r#"\"--project=$VELNOR_PW_PROJECT\" \"--shard=$VELNOR_PW_SHARD\""#),
        "{job}"
    );
    assert!(
        job.contains("VELNOR_PW_PROJECT: ${{ matrix.project }}"),
        "{job}"
    );
    assert!(
        job.contains("VELNOR_PW_SHARD: ${{ matrix.shard }}/${{ matrix.total }}"),
        "{job}"
    );
    assert!(
        job.contains("PLAYWRIGHT_BLOB_OUTPUT_FILE: ${{ runner.temp }}/velnor/pw-blob/leg-${{ matrix.leg }}.zip"),
        "{job}"
    );
    for line in job
        .lines()
        .filter(|line| line.contains("bun run") || line.contains("'run'"))
    {
        assert!(
            !line.contains("${{"),
            "matrix values only through env: {line}"
        );
    }
    assert!(!ci.contains("VELNOR_BUN_LEGS"), "marker scrubbed");
    assert!(!ci.contains("VELNOR_BUN_MAX_PARALLEL"), "marker scrubbed");
    assert!(!job.contains("--fail-on-flaky-tests"), "{job}");
    assert!(!job.contains("--reporter"), "{job}");
    assert!(!job.contains("--update-snapshots"), "{job}");
    let required = job_body(&ci, "required")?;
    assert!(
        required.contains("- bun-chainargos-docs--browser\n"),
        "one id covers every leg: {required}"
    );
    let member = job_body(&ci, "bun-chainargos-docs")?;
    assert!(!member.contains("strategy:"), "{member}");
    Ok(())
}

#[test]
fn flags_appear_only_where_they_apply() -> TestResult {
    let shards_only = sharded_ci("shards = 3\nfail_on_flaky = true\n", "playwright test")?;
    let job = job_body(&shards_only, "bun-chainargos-docs--browser")?;
    assert!(job.contains(r#"\"--shard=$VELNOR_PW_SHARD\""#), "{job}");
    assert!(!job.contains("--project"), "{job}");
    assert!(!job.contains("VELNOR_PW_PROJECT: "), "{job}");
    assert!(job.contains("--fail-on-flaky-tests"), "{job}");
    assert!(!job.contains("max-parallel"), "{job}");
    assert!(
        job.contains("          - leg: 3\n            shard: 3\n            total: 3\n"),
        "{job}"
    );
    let projects_only = sharded_ci("projects = [\"chromium\"]\n", "playwright test")?;
    let job = job_body(&projects_only, "bun-chainargos-docs--browser")?;
    assert!(job.contains(r#"\"--project=$VELNOR_PW_PROJECT\""#), "{job}");
    assert!(!job.contains("--shard"), "{job}");
    assert!(!job.contains("VELNOR_PW_SHARD: "), "{job}");
    Ok(())
}

#[test]
fn inadmissible_shard_scripts_stop_generation() -> TestResult {
    for (script, code) in [
        (
            "playwright test && playwright test -c dev.config.ts",
            "bun_shard_script_compound:test:browser",
        ),
        ("vitest run", "bun_shard_script_not_playwright:test:browser"),
        (
            "playwright test --shard=1/2",
            "bun_shard_script_sets_shard:test:browser",
        ),
        (
            "playwright test -u",
            "bun_shard_script_updates_snapshots:test:browser",
        ),
    ] {
        let repo = sharded_repo("shards = 2\n", script)?;
        let error = prepare(repo.path()).expect_err(code);
        assert!(error.to_string().contains(code), "{code}: {error}");
    }
    Ok(())
}

#[test]
fn sharded_generation_is_deterministic() -> TestResult {
    let config = "projects = [\"a\", \"b\"]\nshards = 2\n";
    assert_eq!(
        sharded_ci(config, "playwright test")?,
        sharded_ci(config, "playwright test")?
    );
    Ok(())
}
