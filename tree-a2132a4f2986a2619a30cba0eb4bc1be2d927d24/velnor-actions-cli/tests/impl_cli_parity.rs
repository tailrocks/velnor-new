//! Plan parity: job IDs match the workflow; plan writes nothing.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use crate::impl_cli_tmp::{
    add_crate_pair, cleanup, code, fresh_tempdir, init_repo, plan_stdout, spawn,
};

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

/// Job IDs from plan lines shaped `    - <id> (<n> steps)`.
fn plan_job_ids(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("    - ")?;
            let (id, _) = rest.split_once(" (")?;
            if id.starts_with("velnor-") {
                Some(id.to_owned())
            } else {
                None
            }
        })
        .collect()
}

/// Job IDs from top-level `  <id>:` keys with the `velnor-` prefix.
fn workflow_job_ids(yaml: &str) -> Vec<String> {
    yaml.lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            let id = rest.strip_suffix(':')?;
            if id.starts_with("velnor-") && !id.contains(' ') {
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
    let yaml = std::fs::read_to_string(preview.join(".github/workflows/velnor.yml"))?;
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
