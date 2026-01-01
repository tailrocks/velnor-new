//! Generated-output inventory cases separated from prepare behavior.

use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, make_repo, plan_for};

#[test]
fn ignored_rust_plans_no_work() -> TestResult {
    let repo = make_repo(
        "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\n[stacks]\nignore = [\"rust\"]\n",
    )?;
    let prep = prepare(repo.path())?;
    assert!(prep.discovery.proposals.is_empty(), "no tasks when ignored");
    let text = plan_for(&prep)?;
    assert!(text.contains("Rust: ignored"), "ignored:\n{text}");
    assert!(text.contains("no-work workflow"), "no-work:\n{text}");
    let report = generate(&prep, &GenerateOptions { output_dir: None })?;
    assert_eq!(report.files_written.len(), 9);
    Ok(())
}
