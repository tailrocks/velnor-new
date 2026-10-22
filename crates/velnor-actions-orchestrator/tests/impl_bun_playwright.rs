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
