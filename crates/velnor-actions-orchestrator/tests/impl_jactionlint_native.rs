//! Every generated lint step runs pinned jactionlint natively: one shared
//! argv over the hk file set, with no ignore pattern or label bridge, and
//! velnor's own tree uses the step concurrency that needed the old bridge.

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::TestResult;

#[test]
fn every_generated_lint_step_runs_jactionlint_natively_over_the_hk_file_set() -> TestResult {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = render_staged_tree(&prepare(&root)?)?;
    let mut steps = 0;
    let mut background = 0;
    for file in &tree.files {
        background += file.bytes.matches("background: true").count();
        for line in file
            .bytes
            .lines()
            .filter(|line| line.contains("-- jactionlint"))
        {
            steps += 1;
            for marker in [
                r#"find .github/workflows -maxdepth 1 -type f -name \"*.y*ml\"; find .github/actions -type f -name \"action.y*ml\"; find .github -maxdepth 1 -type f -name \"dependabot.y*ml\";"#,
                "--config-file .github/jactionlint.yaml --format github --strict-exit --no-online --shellcheck=shellcheck --pyflakes= --no-color",
                "unset ACTIONS_ID_TOKEN_REQUEST_TOKEN",
            ] {
                assert!(
                    line.contains(marker),
                    "{}: {marker} missing: {line}",
                    file.path
                );
            }
            for forbidden in ["-ignore", "--ignore", "JACTIONLINT_NO_HINTS", "--online "] {
                assert!(
                    !line.contains(forbidden),
                    "{}: {forbidden}: {line}",
                    file.path
                );
            }
        }
        assert!(
            !file.bytes.contains("-- actionlint"),
            "{} still runs actionlint",
            file.path
        );
    }
    assert!(
        steps >= 3,
        "the CI job and both release eligibility gates lint the committed tree"
    );
    assert!(
        background > 0,
        "velnor's own CI emits native background steps that jactionlint validates"
    );
    assert!(tree.get(".github/actionlint.yaml").is_none());
    let config = tree
        .get(".github/jactionlint.yaml")
        .ok_or("missing jactionlint config")?;
    for key in ["ignores:", "paths:", "rules:", "online"] {
        assert!(!config.contains(key), "{key} in {config}");
    }
    Ok(())
}
