//! P12 probe/advisory/invariant cases plus static completeness checks.

use std::error::Error;

use super::p12_harness as harness;

const INVENTORY: &str = ".velnor/freshness-inventory.json";
const POLICY: &str = ".velnor/version-policy.toml";
const CATALOG: &str = "crates/velnor-actions-mise/src/catalog.rs";

/// One probe row: inventory source URL, canned file, canned body.
type ProbeRow = (&'static str, &'static str, &'static str);

/// (inventory source URL, canned file, canned body) for every probe row.
fn probe_rows() -> Vec<ProbeRow> {
    let mut rows = probe_tool_rows();
    rows.extend(probe_action_rows());
    rows
}

/// Canned upstream bodies for tool inventory rows.
fn probe_tool_rows() -> Vec<ProbeRow> {
    vec![
        (
            "https://api.github.com/repos/jdx/mise/releases/latest",
            "mise.json",
            "{\"tag_name\": \"v2026.9.16\"}",
        ),
        (
            "https://static.rust-lang.org/dist/channel-rust-stable.toml",
            "rust.toml",
            "[pkg.rust]\nversion = \"1.98.1 (48a229cea 2026-09-01)\"\n",
        ),
        (
            "https://api.github.com/repos/jdx/mr-boxington/releases/latest",
            "mbx.json",
            "[{\"tag_name\": \"v1.20.0-beta\", \"prerelease\": true}, {\"tag_name\": \"v1.19.0\"}]",
        ),
        (
            "https://api.github.com/repos/cli/cli/releases/latest",
            "gh.json",
            "{\"tag_name\": \"v2.101.0\"}",
        ),
        (
            "https://api.github.com/repos/rhysd/actionlint/releases/latest",
            "actionlint.json",
            "{\"tag_name\": \"v1.7.12\"}",
        ),
        (
            "https://api.github.com/repos/koalaman/shellcheck/releases/latest",
            "shellcheck.json",
            "{\"tag_name\": \"v0.11.0\"}",
        ),
        (
            "https://api.github.com/repos/zizmorcore/zizmor/releases/latest",
            "zizmor.json",
            "{\"tag_name\": \"v1.30.1\"}",
        ),
        (
            "https://crates.io/api/v1/crates/cargo-nextest",
            "nextest.json",
            "{\"crate\": {\"max_version\": \"0.9.146\"}}",
        ),
        (
            "https://api.github.com/repos/opentofu/opentofu/releases/latest",
            "opentofu.json",
            "{\"tag_name\": \"v1.13.1\"}",
        ),
    ]
}

/// Canned upstream bodies for action inventory rows.
fn probe_action_rows() -> Vec<ProbeRow> {
    vec![
        (
            "https://api.github.com/repos/jdx/mise-action/releases/latest",
            "mise-action.json",
            "{\"tag_name\": \"v4.3.0\"}",
        ),
        (
            "https://api.github.com/repos/actions/checkout/releases/latest",
            "checkout.json",
            "{\"tag_name\": \"v7.0.1\"}",
        ),
        (
            "https://api.github.com/repos/actions/download-artifact/releases/latest",
            "download.json",
            "{\"tag_name\": \"v8.0.1\"}",
        ),
        (
            "https://api.github.com/repos/actions/upload-artifact/releases/latest",
            "upload.json",
            "{\"tag_name\": \"v7.0.1\"}",
        ),
        (
            "https://api.github.com/repos/actions/cache/releases/latest",
            "cache.json",
            "{\"tag_name\": \"v6.1.0\"}",
        ),
        (
            "https://api.github.com/repos/jdx/mr-boxington-action/releases",
            "mbx-action.json",
            "[{\"tag_name\": \"v1.5.0\"}]",
        ),
        (
            "https://api.github.com/repos/asamarts/alint/releases/latest",
            "alint.json",
            "{\"tag_name\": \"v0.16.1\"}",
        ),
    ]
}

/// Passing fixture with every probe source rewritten to canned `file://` URLs.
fn probe_fixture(prefix: &str) -> Result<harness::Fixture, Box<dyn Error>> {
    let fixture = harness::passing(prefix)?;
    let upstream = fixture.dir.join("upstream");
    std::fs::create_dir_all(&upstream)?;
    let path = fixture.dir.join(INVENTORY);
    let mut body = std::fs::read_to_string(&path)?;
    for (url, file, canned) in probe_rows() {
        harness::write(&fixture.dir, &format!("upstream/{file}"), canned)?;
        let file_url = format!("file://{}", upstream.join(file).display());
        assert!(body.contains(url), "anchor {url}");
        body = body.replace(url, &file_url);
    }
    std::fs::write(path, body)?;
    Ok(fixture)
}

#[test]
fn upstream_probe_passes_on_canned_evidence() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-probe-pass")?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_clean(&run);
    assert!(
        run.stdout.contains("\"check\":\"upstream-probe\""),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("pinned==latest"), "{}", run.stdout);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn upstream_probe_stale_pin_fails() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-probe-stale")?;
    harness::write(
        &fixture.dir,
        "upstream/mise.json",
        "{\"tag_name\": \"v2026.9.99\"}",
    )?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_fail(&run, "upstream-probe");
    harness::assert_fail(&run, "stale pin");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn upstream_probe_unreachable_fails() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-probe-down")?;
    std::fs::remove_file(fixture.dir.join("upstream/gh.json"))?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_fail(&run, "lookup_failed");
    assert!(
        !run.stdout
            .contains("\"subject\":\"gh\",\"status\":\"pass\""),
        "{}",
        run.stdout
    );
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn deny_ignore_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-deny")?;
    harness::mutate(
        &fixture.dir,
        "deny.toml",
        "ignore = []",
        "ignore = [\"RUSTSEC-2026-0001\"]",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "must be a policy exception instead");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn missing_deny_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-deny-missing")?;
    std::fs::remove_file(fixture.dir.join("deny.toml"))?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "advisories");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn every_fail_row_is_nonzero() -> Result<(), Box<dyn Error>> {
    let today = harness::days_iso(0)?;
    let old = harness::days_iso(-30)?;
    let granted = harness::days_iso(-10)?;
    let expired = harness::days_iso(-1)?;
    let hold = format!(
        "\"temporary_holds\":[{{\"key\":\"gh\",\"held_version\":\"9.9.9\",\"owner\":\"t\",\
         \"issue\":\"#1\",\"reason\":\"r\",\"granted\":\"{granted}\",\"expires\":\"{expired}\"}}]"
    );
    let table: Vec<(&str, &str, String, String, &str)> = vec![
        (
            "t-lock",
            "crates/aaa/Cargo.toml",
            "serde_json = \"=1.0.100\"".to_owned(),
            "serde_json = \"1.0.100\"".to_owned(),
            "lock-staleness",
        ),
        (
            "t-mirror",
            POLICY,
            "rust = \"1.98.1\"".to_owned(),
            "rust = \"1.99.0\"".to_owned(),
            "policy-mirror",
        ),
        (
            "t-pin",
            CATALOG,
            "GH_VERSION: &str = \"2.101.0\"".to_owned(),
            "GH_VERSION: &str = \"9.9.9\"".to_owned(),
            "local-pin",
        ),
        (
            "t-evidence",
            INVENTORY,
            format!("\"checked_at\":\"{today}\""),
            format!("\"checked_at\":\"{old}\""),
            "upstream-freshness",
        ),
        (
            "t-hold",
            INVENTORY,
            "\"temporary_holds\":[]".to_owned(),
            hold,
            "exception-expiry",
        ),
        (
            "t-deny",
            "deny.toml",
            "ignore = []".to_owned(),
            "ignore = [\"RUSTSEC-2026-0001\"]".to_owned(),
            "advisories",
        ),
    ];
    for (prefix, rel, before, after, check) in &table {
        let fixture = harness::passing(prefix)?;
        harness::mutate(&fixture.dir, rel, before, after)?;
        let run = harness::run_script(&fixture.dir, &[])?;
        let saw_fail = run.stdout.lines().any(|line| {
            line.starts_with("row: ")
                && line.contains("\"status\":\"fail\"")
                && line.contains(check)
        });
        assert!(saw_fail, "no {check} fail row:\n{}", run.stdout);
        assert_ne!(run.code, 0, "{check} fail row exited zero");
        harness::cleanup(&fixture);
    }
    Ok(())
}

#[test]
fn script_covers_all_forms_scopes_and_namespaces() -> Result<(), Box<dyn Error>> {
    let script = crate::impl_repo_policy::read("scripts/check-freshness.sh")?;
    for marker in [
        "build-dependencies",
        "dev-dependencies",
        "target.",
        "workspace",
        "package",
        "name+version+source",
        "ambiguous identity",
        "unreachable locked package",
        "local-pin",
        "policy-mirror",
        "upstream-freshness",
        "upstream-probe",
        "exception-expiry",
        "standing-exception",
        "advisories",
        "def fail_row",
        "sys.exit(1)",
    ] {
        assert!(script.contains(marker), "script misses {marker}");
    }
    Ok(())
}

#[test]
fn policy_file_is_complete() -> Result<(), Box<dyn Error>> {
    let policy = crate::impl_repo_policy::read(".velnor/version-policy.toml")?;
    for marker in [
        "registry = \"https://github.com/rust-lang/crates.io-index\"",
        "[github_runner_images.linux_x64]",
        "supported = [\"ubuntu-26.04\", \"ubuntu-24.04\", \"ubuntu-22.04\"]",
        "[validation-tools]",
        "cargo-mutants = \"27.1.0\"",
        "mise = \"2026.9.18\"",
        "rust = \"1.98.1\"",
        "mr-boxington = \"1.21.0\"",
        "gh = \"2.102.0\"",
        "actionlint = \"1.7.12\"",
        "shellcheck = \"0.11.0\"",
        "zizmor = \"1.30.1\"",
        "nextest = \"0.9.146\"",
    ] {
        assert!(policy.contains(marker), "policy misses {marker}");
    }
    let tables = policy
        .lines()
        .filter(|line| line.trim() == "[[actions]]")
        .count();
    assert_eq!(tables, 9, "nine action mirrors");
    let mutants = crate::impl_repo_policy::read(".cargo/mutants.toml")?;
    assert!(
        mutants.contains("# pinned: cargo-mutants = \"27.1.0\""),
        "pin line"
    );
    Ok(())
}

#[test]
fn procedure_is_reconciled() -> Result<(), Box<dyn Error>> {
    let procedure = crate::impl_repo_policy::read("docs/implemented/update-procedure.md")?;
    for marker in [
        "local-pin",
        "policy-mirror",
        "upstream-freshness",
        "upstream-probe",
        "--check-upstream",
        "--with-advisories",
        "10 s",
        "512 KiB",
        "MUST NOT be reported as current",
        "name+version+source",
        "cargo-mutants",
        "27.1.0",
    ] {
        assert!(procedure.contains(marker), "procedure misses {marker}");
    }
    Ok(())
}
