//! Leg blob verification, leg artifacts and the merged report job of a
//! sharded Playwright suite.

use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_bun_playwright::{core_integrity, playwright_repo_with};
use crate::impl_common::{TestResult, git};
use crate::impl_schema2_routing::{job_body, required_file};

const SUITE: &str = "[stacks.bun.members.\"chainargos-docs\".suites.browser]\n\
scripts = [\"test:browser\"]\nbrowsers = [\"chromium\"]\ntimeout_minutes = 25\n\
[stacks.bun.members.\"chainargos-docs\".suites.browser.playwright]\n";

fn ci(playwright: &str) -> Result<String, Box<dyn std::error::Error>> {
    let repo = playwright_repo_with(&format!("{SUITE}{playwright}"), Some(&core_integrity()))?;
    fs::write(
        repo.path().join("frontend/docs/package.json"),
        r#"{"name":"chainargos-docs","scripts":{"typecheck":"tsc","test:browser":"playwright test"}}"#,
    )?;
    git(&["add", "."], repo.path())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
}

fn ci_with_private_assets(playwright: &str) -> Result<String, Box<dyn std::error::Error>> {
    let extra = format!(
        "{SUITE}{playwright}\n[private_sources]\nowner = \"acme\"\napp_id_variable = \"VELNOR_DEPS_APP_ID\"\n\
app_private_key_secret = \"VELNOR_DEPS_APP_PRIVATE_KEY\"\nnpm_scopes = [\"@chainargos\"]\n\
[[private_sources.release_assets]]\nid = \"ui-assets\"\nrepository = \"acme/ui-assets\"\n\
tag = \"v1.0.0\"\nasset = \"assets.tar.gz\"\nextract_to = \"ui-assets\"\n"
    );
    let repo = playwright_repo_with(&extra, Some(&core_integrity()))?;
    fs::write(
        repo.path().join("frontend/docs/package.json"),
        r#"{"name":"chainargos-docs","scripts":{"typecheck":"tsc","test:browser":"playwright test"}}"#,
    )?;
    fs::write(
        repo.path().join(".velnor/deps.lock"),
        "schema = 1\n[[release_assets]]\nid = \"ui-assets\"\nrepository = \"acme/ui-assets\"\n\
tag = \"v1.0.0\"\ncommit = \"0123456789abcdef0123456789abcdef01234567\"\n\
asset = \"assets.tar.gz\"\nsha256 = \"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\"\n\
extract_to = \"ui-assets\"\n",
    )?;
    git(&["add", "."], repo.path())?;
    let tree = render_staged_tree(&prepare(repo.path())?)?;
    Ok(required_file(&tree, ".github/workflows/ci.yml")?.to_owned())
}

fn after<'a>(body: &'a str, name: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    Ok(body
        .split(&format!("- name: {name}\n"))
        .nth(1)
        .ok_or(format!("missing step {name}: {body}"))?)
}

#[test]
fn every_leg_verifies_and_uploads_its_blob_and_failure_results() -> TestResult {
    let ci = ci("projects = [\"chromium\", \"mobile\"]\nshards = 2\n")?;
    let job = job_body(&ci, "bun-chainargos-docs--browser")?;
    let wait = job.find("wait-all").ok_or("join")?;
    let verify = job.find("- name: Verify Playwright blob").ok_or("verify")?;
    let blob = job.find("- name: Upload Playwright blob").ok_or("blob")?;
    let results = job
        .find("- name: Upload Playwright results")
        .ok_or("results")?;
    assert!(wait < verify && verify < blob && blob < results, "{job}");
    let verify = after(job, "Verify Playwright blob")?;
    assert!(verify.contains("if: \"!cancelled()\""), "{verify}");
    assert!(
        verify.contains("VELNOR_PW_LEG: ${{ matrix.leg }}"),
        "{verify}"
    );
    assert!(
        verify.contains("VELNOR_PW_PROJECT: ${{ matrix.project }}"),
        "{verify}"
    );
    assert!(verify.contains("unzip -p"), "{verify}");
    assert!(
        verify.contains("playwright_blob_shard_mismatch"),
        "{verify}"
    );
    assert!(
        verify.contains("playwright_blob_project_mismatch"),
        "{verify}"
    );
    assert!(verify.contains("dependencies"), "{verify}");
    assert!(verify.contains("teardown"), "{verify}");
    assert!(verify.contains("empty_unsharded_blob"), "{verify}");
    let blob = after(job, "Upload Playwright blob")?;
    assert!(blob.contains("if: \"!cancelled()\""), "{blob}");
    assert!(
        blob.contains("name: velnor-pw-blob-bun-chainargos-docs--browser-leg-${{ matrix.leg }}"),
        "{blob}"
    );
    assert!(blob.contains("overwrite: \"true\""), "{blob}");
    assert!(blob.contains("retention-days: \"1\""), "{blob}");
    assert!(blob.contains("if-no-files-found: error"), "{blob}");
    let results = after(job, "Upload Playwright results")?;
    assert!(results.contains("if: failure()"), "{results}");
    assert!(
        results.contains("path: frontend/docs/test-results"),
        "{results}"
    );
    assert!(results.contains("retention-days: \"7\""), "{results}");
    assert!(results.contains("if-no-files-found: warn"), "{results}");
    assert!(
        !ci.contains("velnor-pw-report-"),
        "no report job without report = merge"
    );
    Ok(())
}

#[test]
fn a_merge_report_job_runs_after_the_legs_outside_required() -> TestResult {
    let ci = ci(
        "shards = 3\nreport = \"merge\"\nreport_retention_days = 30\nresults_retention_days = 2\n",
    )?;
    let report = job_body(&ci, "bun-chainargos-docs--browser--report")?;
    assert!(
        report.contains("name: Bun / chainargos-docs / browser / report"),
        "{report}"
    );
    assert!(report.contains("continue-on-error: true"), "{report}");
    assert!(
        report.contains(
            "if: \"!cancelled() && needs.bun-chainargos-docs--browser.result != 'skipped'\""
        ),
        "{report}"
    );
    assert!(
        report.contains("- plan\n      - bun-chainargos-docs--browser\n"),
        "{report}"
    );
    assert!(report.contains("timeout-minutes: 25"), "{report}");
    assert!(report.contains("contents: read"), "{report}");
    assert!(!report.contains("strategy:"), "{report}");
    assert!(!report.contains("Install Playwright browsers"), "{report}");
    assert!(
        report.contains("pattern: velnor-pw-blob-bun-chainargos-docs--browser-leg-*"),
        "{report}"
    );
    assert!(report.contains("merge-multiple: \"true\""), "{report}");
    assert!(
        report.contains(
            r"'\\''playwright'\\'' '\\''merge-reports'\\'' '\\''--reporter=html,json'\\''"
        ),
        "{report}"
    );
    assert!(
        report.contains("name: velnor-pw-report-bun-chainargos-docs--browser"),
        "{report}"
    );
    assert!(report.contains("retention-days: \"30\""), "{report}");
    assert!(
        !ci.contains("VELNOR_SUITE_ALLOW_FAILURE"),
        "marker scrubbed"
    );
    let required = job_body(&ci, "required")?;
    assert!(
        required.contains("- bun-chainargos-docs--browser\n"),
        "{required}"
    );
    assert!(
        !required.contains("--report"),
        "the report never gates: {required}"
    );
    let legs = job_body(&ci, "bun-chainargos-docs--browser")?;
    assert!(legs.contains("retention-days: \"2\""), "{legs}");
    Ok(())
}

#[test]
fn report_job_hands_off_private_assets_before_tools_and_merge() -> TestResult {
    let ci = ci_with_private_assets("shards = 2\nreport = \"merge\"\n")?;
    let report = job_body(&ci, "bun-chainargos-docs--browser--report")?;
    let prepare = report
        .find("name: Prepare private asset handoff")
        .ok_or("prepare handoff")?;
    let download = report
        .find("name: Download private assets")
        .ok_or("download")?;
    let import = report
        .find("name: Import and verify private assets")
        .ok_or("import")?;
    let tools = report.find("name: Prepare pinned tools").ok_or("tools")?;
    let install = report
        .find("name: Install dependencies (registry phase)")
        .ok_or("install")?;
    let merge = report
        .find("name: Merge Playwright reports")
        .ok_or("merge")?;
    assert!(
        prepare < download && download < import && import < tools,
        "{report}"
    );
    assert!(tools < install && install < merge, "{report}");
    assert!(report.contains("bun-assets-import-v1"), "{report}");
    assert!(report.contains("VELNOR_BUN_ASSET_PINS"), "{report}");
    for secret in [
        "deps-token",
        "VELNOR_DEPS_APP_PRIVATE_KEY",
        "Mint read-only token",
    ] {
        assert!(!report.contains(secret), "{secret} in report job: {report}");
    }
    Ok(())
}

#[test]
fn a_playwright_script_name_stops_generation() -> TestResult {
    let repo = playwright_repo_with(&format!("{SUITE}shards = 2\n"), Some(&core_integrity()))?;
    fs::write(
        repo.path().join("frontend/docs/package.json"),
        r#"{"name":"chainargos-docs","scripts":{"test:browser":"playwright test","playwright":"echo"}}"#,
    )?;
    git(&["add", "."], repo.path())?;
    let error = prepare(repo.path()).expect_err("shadow");
    assert!(
        error
            .to_string()
            .contains("bun_shard_script_name_shadow:chainargos-docs"),
        "{error}"
    );
    Ok(())
}
