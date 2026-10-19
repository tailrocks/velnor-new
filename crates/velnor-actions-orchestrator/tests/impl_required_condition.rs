//! `Required` job-level condition: skipped on cancellation, run otherwise.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{TestResult, make_repo};

/// Rendered `ci.yml` text for the default consumer fixture.
fn rendered_workflow(root: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let tree = render_staged_tree(&prepare(root)?)?;
    tree.get(".github/workflows/ci.yml")
        .map(str::to_owned)
        .ok_or_else(|| "ci.yml not rendered".into())
}

/// Lines of the top-level `jobs:` entry named `job_id`, up to the next job.
fn job_block<'a>(yaml: &'a str, job_id: &str) -> Vec<&'a str> {
    let header = format!("  {job_id}:");
    let mut lines = yaml.lines().skip_while(|line| *line != header).peekable();
    let mut block = Vec::new();
    if let Some(first) = lines.next() {
        block.push(first);
    }
    for line in lines {
        let next_job = line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':');
        if next_job {
            break;
        }
        block.push(line);
    }
    block
}

/// `Required` skips on cancellation instead of holding the concurrency
/// group, yet still runs (and judges) when needed jobs fail or are skipped.
#[test]
fn required_job_condition_is_not_cancelled_not_always() -> TestResult {
    let repo = make_repo("schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n")?;
    let yaml = rendered_workflow(repo.path())?;
    let required = job_block(&yaml, "required");
    assert!(!required.is_empty(), "required job rendered");
    let job_level: Vec<&&str> = required
        .iter()
        .filter(|line| line.starts_with("    if:"))
        .collect();
    assert_eq!(
        job_level,
        [&"    if: ${{ !cancelled() }}"],
        "job-level condition"
    );
    // Report uploads keep running regardless of the job outcome.
    assert!(
        required.iter().any(|line| line.trim() == "if: always()"),
        "upload step stays always-on inside the job"
    );
    let plan = job_block(&yaml, "plan");
    assert!(
        plan.iter().all(|line| !line.starts_with("    if:")),
        "only the gate carries a job-level condition"
    );
    Ok(())
}
