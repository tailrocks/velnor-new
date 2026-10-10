//! P12 probe/advisory/invariant cases plus static completeness checks.

use std::error::Error;

use super::p12_harness as harness;

const INVENTORY: &str = ".velnor/freshness-inventory.json";
const POLICY: &str = ".velnor/version-policy.toml";
const CATALOG: &str = "crates/velnor-actions-mise/src/catalog.rs";

/// Passing fixture with every probe source rewritten to canned `file://` URLs.
fn probe_fixture(prefix: &str) -> Result<harness::Fixture, Box<dyn Error>> {
    let fixture = harness::passing(prefix)?;
    let upstream = fixture.dir.join("upstream");
    std::fs::create_dir_all(&upstream)?;
    let path = fixture.dir.join(INVENTORY);
    let mut body = std::fs::read_to_string(&path)?;
    for (url, file, canned) in harness::probe_rows() {
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
fn nested_deny_ignore_fails() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-nested-deny")?;
    harness::add_nested_workspace(&fixture)?;
    harness::mutate(
        &fixture.dir,
        "crates/runner/deny.toml",
        "ignore = []",
        "ignore = [\"RUSTSEC-2026-0001\"]",
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "crates/runner/deny.toml");
    assert!(
        run.stdout
            .contains("ignored advisory must be a policy exception"),
        "{}",
        run.stdout
    );
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
    let old = harness::days_iso(-30)?;
    let granted = harness::days_iso(-10)?;
    let expired = harness::days_iso(-1)?;
    let hold = format!(
        "\"temporary_holds\":[{{\"key\":\"gh\",\"held_version\":\"9.9.9\",\"owner\":\"t\",\
         \"issue\":\"#1\",\"reason\":\"r\",\"granted\":\"{granted}\",\"expires\":\"{expired}\"}}]"
    );
    let table = [
        (
            "t-lock",
            "crates/aaa/Cargo.toml",
            Some(("serde_json = \"=1.0.100\"", "serde_json = \"1.0.100\"")),
            "lock-staleness",
        ),
        (
            "t-mirror",
            POLICY,
            Some(("rust = \"1.98.1\"", "rust = \"1.99.0\"")),
            "policy-mirror",
        ),
        (
            "t-pin",
            CATALOG,
            Some((
                "GH_VERSION: &str = \"2.101.0\"",
                "GH_VERSION: &str = \"9.9.9\"",
            )),
            "local-pin",
        ),
        ("t-evidence", INVENTORY, None, "upstream-freshness"),
        (
            "t-hold",
            INVENTORY,
            Some(("\"temporary_holds\":[]", &hold)),
            "exception-expiry",
        ),
        (
            "t-deny",
            "deny.toml",
            Some(("ignore = []", "ignore = [\"RUSTSEC-2026-0001\"]")),
            "advisories",
        ),
    ];
    for (prefix, rel, mutation, check) in table {
        let fixture = harness::passing(prefix)?;
        if let Some((before, after)) = mutation {
            harness::mutate(&fixture.dir, rel, before, after)?;
        } else {
            let current = format!("\"checked_at\":\"{}\"", fixture.checked_at);
            let stale = format!("\"checked_at\":\"{old}\"");
            harness::mutate(&fixture.dir, rel, &current, &stale)?;
        }
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
    let mut owner = String::new();
    for path in crate::impl_repo_policy::tree_files("crates/velnor-actions-freshness/src", "rs")? {
        owner.push_str(&std::fs::read_to_string(path)?);
    }
    for marker in [
        "build-dependencies",
        "dev-dependencies",
        "target.",
        "workspace",
        "package",
        "ambiguous identity",
        "unreachable locked package",
        "local-pin",
        "policy-mirror",
        "upstream-freshness",
        "upstream-probe",
        "exception-expiry",
        "standing-exception",
        "advisories",
        "fn fail_row",
    ] {
        assert!(
            owner.contains(marker),
            "Rust freshness owner misses {marker}"
        );
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
        "mise = \"2026.10.7\"",
        "rust = \"1.98.1\"",
        "mr-boxington = \"1.21.1\"",
        "gh = \"2.102.0\"",
        "actionlint = \"1.7.12\"",
        "shellcheck = \"0.11.0\"",
        "zizmor = \"1.30.1\"",
        "nextest = \"0.9.148\"",
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
