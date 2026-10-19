//! Every generated repository-wide actionlint step carries the step-concurrency bridge.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::TestResult;

#[test]
fn every_generated_actionlint_step_carries_the_step_parallelism_bridge() -> TestResult {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = render_staged_tree(&prepare(&root)?)?;
    let mut steps = 0;
    for file in &tree.files {
        for line in file
            .bytes
            .lines()
            .filter(|line| line.contains("-- actionlint"))
        {
            steps += 1;
            for marker in [
                "-ignore '^unexpected key ",
                "-ignore '^step must run script ",
            ] {
                assert!(
                    line.contains(marker),
                    "{} lints without the bridge: {line}",
                    file.path
                );
            }
        }
    }
    assert!(
        steps >= 3,
        "the CI job and both release eligibility gates lint workflows"
    );
    Ok(())
}
