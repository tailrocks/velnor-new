//! P12-4 scheduled-probe contract: offline gate, evidence, read-only.
//!
//! The `--check-upstream` probe is the bounded read-only tooling a future
//! scheduled job runs. These cases pin the contract that job relies on:
//! the default gate never fetches, every probe row carries its source and
//! check time, the probe writes nothing, and every lookup failure fails
//! closed instead of reporting current.

use std::error::Error;
use std::path::{Path, PathBuf};

use super::p12_harness as harness;

const INVENTORY: &str = ".velnor/freshness-inventory.json";

/// One probe row: inventory source URL, canned file, canned body.
type ProbeRow = (&'static str, &'static str, &'static str);

/// (inventory source URL, canned file, canned body) for every probe row.
fn probe_rows() -> Vec<ProbeRow> {
    harness::probe_rows()
}

#[test]
fn community_latest_reads_jdk_assets_instead_of_release_tag() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-community-cohorts")?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_clean(&run);
    assert!(has_pass(&run, "upstream-probe", "java"), "{}", run.stdout);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn community_selected_qualified_pin_stays_stale_against_newer_assets() -> Result<(), Box<dyn Error>>
{
    let fixture = probe_fixture("p12-community-stale")?;
    let path = fixture.dir.join("upstream/java.json");
    let body = std::fs::read_to_string(&path)?
        .replace("25.0.4.1.1", "25.0.4.1.2")
        .replace("graal-25.4.4.1.1", "graal-25.4.4.1.2");
    std::fs::write(path, body)?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_fail(&run, "stale pin");
    assert!(run.stdout.contains("latest='25.0.4.1.2'"), "{}", run.stdout);
    assert!(!has_pass(&run, "upstream-probe", "java"), "{}", run.stdout);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn node_latest_selects_only_the_24_lts_release_line() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-node-lts")?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_clean(&run);
    assert!(has_pass(&run, "upstream-probe", "node"), "{}", run.stdout);
    harness::cleanup(&fixture);
    Ok(())
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

/// True when some pass row for `check` names `subject`.
fn has_pass(run: &harness::Run, check: &str, subject: &str) -> bool {
    let want_check = format!("\"check\":\"{check}\"");
    let want_subject = format!("\"subject\":\"{subject}\"");
    harness::rows(run).iter().any(|line| {
        line.contains("\"status\":\"pass\"")
            && line.contains(&want_check)
            && line.contains(&want_subject)
    })
}

/// Every regular file's bytes under `dir`: relative path plus content.
type Snapshot = Vec<(PathBuf, Vec<u8>)>;

/// Every regular file's bytes under `dir`, sorted by relative path.
fn snapshot(dir: &Path) -> Result<Snapshot, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        let mut entries: Vec<PathBuf> = Vec::new();
        for entry in std::fs::read_dir(&path)? {
            entries.push(entry?.path());
        }
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                pending.push(entry);
            } else {
                let rel = entry.strip_prefix(dir)?.to_path_buf();
                out.push((rel, std::fs::read(&entry)?));
            }
        }
    }
    out.sort();
    Ok(out)
}

#[test]
fn default_gate_never_fetches() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-offline")?;
    let path = fixture.dir.join(INVENTORY);
    let mut body = std::fs::read_to_string(&path)?;
    for (url, file, _) in probe_rows() {
        assert!(body.contains(url), "anchor {url}");
        body = body.replace(url, &format!("file:///missing-upstream/{file}"));
    }
    std::fs::write(path, body)?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_clean(&run);
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn probe_rows_carry_source_and_check_time() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-evidence")?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_clean(&run);
    let all = harness::rows(&run);
    let probe: Vec<&&str> = all
        .iter()
        .filter(|line| line.contains("\"check\":\"upstream-probe\""))
        .collect();
    assert_eq!(probe.len(), 48, "47 rows + runner note:\n{}", run.stdout);
    for line in probe {
        if line.contains("\"subject\":\"runner\"") {
            assert!(
                line.contains("platform-qualification"),
                "runner note changed: {line}"
            );
            continue;
        }
        assert!(line.contains("source "), "row lacks source: {line}");
        assert!(line.contains("checked 2"), "row lacks check time: {line}");
    }
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn probe_writes_nothing() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-readonly")?;
    let before = snapshot(&fixture.dir)?;
    assert!(!before.is_empty(), "fixture snapshot is empty");
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_clean(&run);
    assert_eq!(snapshot(&fixture.dir)?, before, "probe mutated the tree");
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn prerelease_only_is_lookup_failure_never_current() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-prerelease")?;
    harness::write(
        &fixture.dir,
        "upstream/mbx.json",
        "[{\"tag_name\": \"v9.9.9-beta\", \"prerelease\": true}, \
         {\"tag_name\": \"v9.9.8\", \"draft\": true}]",
    )?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_fail(&run, "lookup_failed");
    assert!(
        !has_pass(&run, "upstream-probe", "mr-boxington"),
        "prerelease-only reported current:\n{}",
        run.stdout
    );
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn unparseable_body_is_lookup_failure_never_current() -> Result<(), Box<dyn Error>> {
    let fixture = probe_fixture("p12-garbage")?;
    harness::write(&fixture.dir, "upstream/gh.json", "not json at all {{{")?;
    let run = harness::run_script(&fixture.dir, &["--check-upstream"])?;
    harness::assert_fail(&run, "lookup_failed");
    assert!(
        !has_pass(&run, "upstream-probe", "gh"),
        "unparseable body reported current:\n{}",
        run.stdout
    );
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn stale_evidence_names_source_and_timestamp() -> Result<(), Box<dyn Error>> {
    let fixture = harness::passing("p12-stale")?;
    let today = harness::days_iso(0)?;
    let old = harness::days_iso(-30)?;
    harness::mutate(
        &fixture.dir,
        INVENTORY,
        &format!("\"checked_at\":\"{today}\""),
        &format!("\"checked_at\":\"{old}\""),
    )?;
    let run = harness::run_script(&fixture.dir, &[])?;
    harness::assert_fail(&run, "upstream-freshness");
    let all = harness::rows(&run);
    let stale: Vec<&&str> = all
        .iter()
        .filter(|line| {
            line.contains("\"check\":\"upstream-freshness\"")
                && line.contains("\"status\":\"fail\"")
        })
        .collect();
    assert!(!stale.is_empty(), "no stale rows:\n{}", run.stdout);
    for line in stale {
        assert!(line.contains("checked "), "row lacks timestamp: {line}");
    }
    harness::cleanup(&fixture);
    Ok(())
}

#[test]
fn procedure_states_scheduled_producer() -> Result<(), Box<dyn Error>> {
    let procedure = crate::impl_repo_policy::read("docs/implemented/update-procedure.md")?;
    assert!(
        !procedure.contains("The scheduled job refreshes"),
        "procedure asserts automation that is not wired"
    );
    for marker in [
        "The scheduled producer is",
        ".github/workflows/freshness.yml",
        "cron: 0 6 * * 1",
        "ScheduleTrigger",
        "P05",
        "--check-upstream",
        "probe-only",
    ] {
        assert!(procedure.contains(marker), "procedure misses {marker}");
    }
    Ok(())
}
