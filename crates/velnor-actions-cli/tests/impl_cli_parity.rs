//! Plan parity: job IDs match the workflow; plan writes nothing.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use crate::impl_cli_tmp::{
    add_crate_pair, cleanup, code, fresh_tempdir, ignore_rust, init_repo, plan_stdout, spawn,
};

/// Cache-query verbs and internal-artifact names plan must never print.
///
/// Planned `Cache layers:` are reported (see the field-set test); querying
/// cache contents, naming `plan.json`/baseline artifacts, or leaking the
/// event-time protocol would contradict par §10.
const NO_QUERY: [&str; 23] = [
    "query",
    "Query",
    "lookup",
    "Lookup",
    "fetch",
    "Fetch",
    "download",
    "Download",
    "restore",
    "Restore",
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "baseline.json",
    "plan.json",
    "matrix.json",
    "task-result",
    "cache hit",
    "Cache hit",
    "plan-v1",
    "merge-v1",
    "response.json",
    "artifact",
    "Artifact",
];

/// Assert `text` carries no cache-query or internal-exposure language.
///
/// The `Repository:` echo is an OS path, not generator content, so it is
/// dropped before the scan: temp segments could match tokens spuriously.
/// The `Action pins:` list is dropped too: pinned `uses:` refs name the
/// static actions `generate` embeds (including `download-artifact` and
/// `cache/restore`), which declares content rather than querying caches
/// or exposing the event-time protocol.
fn assert_no_query_or_exposure(text: &str) {
    let mut in_pins = false;
    let body: Vec<&str> = text
        .lines()
        .filter(|line| {
            if line.starts_with("  Action pins:") {
                in_pins = true;
                return false;
            }
            if in_pins {
                if line.starts_with("    - ") {
                    return false;
                }
                in_pins = false;
            }
            !line.starts_with("Repository:")
        })
        .collect();
    let body = body.join("\n");
    for token in NO_QUERY {
        assert!(!body.contains(token), "plan leaks {token:?}:\n{text}");
    }
}

/// Crate-job count from the `- N Rust crate job(s)` plan line.
fn crate_count(stdout: &str) -> Result<usize, Box<dyn Error>> {
    for line in stdout.lines() {
        let Some(rest) = line.trim().strip_prefix("- ") else {
            continue;
        };
        let Some(count) = rest
            .strip_suffix(" Rust crate jobs")
            .or_else(|| rest.strip_suffix(" Rust crate job"))
        else {
            continue;
        };
        return Ok(count.parse()?);
    }
    Err("crate count line missing".into())
}

/// Snapshot every file under `dir` except `.git`, as relative path to bytes.
fn snapshot(dir: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, Box<dyn Error>> {
    let mut out = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(&path)? {
            let entry = entry?;
            let relative = entry.path().strip_prefix(dir)?.to_path_buf();
            if relative.starts_with(".git") {
                continue;
            }
            if entry.file_type()?.is_dir() {
                pending.push(entry.path());
            } else {
                out.insert(relative, std::fs::read(entry.path())?);
            }
        }
    }
    Ok(out)
}

/// Job IDs from plan job lines shaped `- <id> (<n> steps)`.
fn plan_job_ids(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("- ")?;
            let (id, steps) = rest.split_once(" (")?;
            if !id.is_empty() && !id.contains(' ') && steps.ends_with(" steps)") {
                Some(id.to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Job IDs from top-level `  <id>:` keys under the jobs block.
fn workflow_job_ids(yaml: &str) -> Vec<String> {
    let jobs = yaml.split_once("jobs:").map_or("", |(_, tail)| tail);
    jobs.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            if rest.starts_with(' ') {
                return None;
            }
            let id = rest.strip_suffix(':')?;
            if !id.is_empty() && !id.contains(' ') {
                Some(id.to_owned())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn plan_job_ids_match_generated_workflow() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("parity-jobs")?;
    init_repo(&tmp)?;
    add_crate_pair(&tmp)?;
    let stdout = plan_stdout(&tmp)?;
    let planned = plan_job_ids(&stdout);
    assert!(!planned.is_empty(), "no jobs in plan:\n{stdout}");
    let outer = fresh_tempdir("parity-preview")?;
    let preview = outer.join("preview");
    let output = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    let yaml = std::fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
    let rendered = workflow_job_ids(&yaml);
    assert!(!rendered.is_empty(), "no jobs in workflow:\n{yaml}");
    for id in &planned {
        assert!(rendered.contains(id), "{id} missing from workflow");
    }
    for id in &rendered {
        assert!(planned.contains(id), "{id} missing from plan");
    }
    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

#[test]
fn plan_reports_full_configured_crates_in_agreement_with_yaml() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("parity-matrix")?;
    init_repo(&tmp)?;
    add_crate_pair(&tmp)?;
    let stdout = plan_stdout(&tmp)?;
    // PAR-10.1 cli half: the full configured count (never narrowed here),
    // the per-obligation kind chain, and the event-time narrowing explanation.
    let count = crate_count(&stdout)?;
    assert_eq!(count, 2, "both crates planned:\n{stdout}");
    assert!(stdout.contains("Each: "), "kind chain missing:\n{stdout}");
    assert!(
        stdout.contains(
            "Pull-request execution narrows crate obligations through its event-time affected-work plan."
        ),
        "{stdout}"
    );
    assert_eq!(plan_stdout(&tmp)?, stdout, "crate print not deterministic");
    let outer = fresh_tempdir("parity-matrix-preview")?;
    let preview = outer.join("preview");
    let output = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    let yaml = std::fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
    for marker in ["  rust-apple:", "  rust-zebra:", "name: Rust / apple"] {
        assert!(yaml.contains(marker), "yaml lacks {marker}:\n{yaml}");
    }
    for marker in ["strategy:", "fail-fast: false", "fromJSON"] {
        assert!(!yaml.contains(marker), "yaml keeps {marker}:\n{yaml}");
    }
    // Ignored work: no crates in plan and none in YAML (both directions).
    ignore_rust(&tmp)?;
    let ignored = plan_stdout(&tmp)?;
    assert!(!ignored.contains("Rust crate job"), "{ignored}");
    assert!(ignored.contains("no matrix entries"), "{ignored}");
    let second = outer.join("second");
    let output = spawn(
        &["generate", "--output-dir", second.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&output), 0, "stderr: {:?}", output.stderr);
    let yaml = std::fs::read_to_string(second.join(".github/workflows/ci.yml"))?;
    assert!(!yaml.contains("strategy:"), "static yaml:\n{yaml}");
    assert!(!yaml.contains("fromJSON"), "static yaml:\n{yaml}");
    cleanup(&tmp);
    cleanup(&outer);
    Ok(())
}

#[test]
fn plan_queries_no_caches_and_exposes_no_plan_json() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("parity-noquery")?;
    init_repo(&tmp)?;
    add_crate_pair(&tmp)?;
    let before = snapshot(&tmp)?;
    assert!(!before.is_empty());
    let stdout = plan_stdout(&tmp)?;
    // PAR-10.2: inventory only — no cache query materializes files, and no
    // internal plan.json/baseline/matrix artifact is written or named.
    assert_eq!(snapshot(&tmp)?, before, "plan modified the repo");
    for path in before.keys() {
        assert!(
            path.extension().is_none_or(|ext| ext != "json"),
            "json artifact present: {}",
            path.display()
        );
    }
    assert!(
        stdout.contains("Cache layers:"),
        "planned layers:\n{stdout}"
    );
    assert_no_query_or_exposure(&stdout);
    ignore_rust(&tmp)?;
    assert_no_query_or_exposure(&plan_stdout(&tmp)?);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn plan_writes_nothing_and_exposes_no_plan_json() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("parity-clean")?;
    init_repo(&tmp)?;
    add_crate_pair(&tmp)?;
    let before = snapshot(&tmp)?;
    assert!(!before.is_empty());
    let stdout = plan_stdout(&tmp)?;
    assert_eq!(snapshot(&tmp)?, before, "plan modified the repo");
    assert!(
        !stdout.contains("plan.json"),
        "plan leaks plan.json:\n{stdout}"
    );
    cleanup(&tmp);
    Ok(())
}
