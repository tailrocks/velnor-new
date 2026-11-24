// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{RepoView, WORKFLOW_PATH};
use velnor_rust::DetectError;

use crate::facts::TestResult;

const MISE: &str = "[tools]\njactionlint = \"2.4.0\"\n\"cargo:ripgrep\" = \"14\"\n\n[tasks.check]\nrun = \"true\"\n";
const LOCK: &str = "[[tools.jactionlint]]\nversion = \"2.4.0\"\nspecifiers = [\"2.4.0\"]\n[tools.jactionlint.\"platforms.linux-x64\"]\nurl = \"https://example.invalid/jactionlint\"\n\n[[tools.\"cargo:ripgrep\"]]\nversion = \"14.0.0\"\n";

fn view(config: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = RepoView::default();
    crate::stacks::put(&mut view, "mise.toml", MISE.as_bytes())?;
    crate::stacks::put(&mut view, "mise.lock", LOCK.as_bytes())?;
    crate::stacks::put(&mut view, ".velnor/rust-ci.toml", config.as_bytes())?;
    crate::pipeline::pin_helper(&mut view, "0.1.89")?;
    Ok(view)
}

fn refused(config: &str) -> Result<GenerateError, Box<dyn std::error::Error>> {
    match expected_with_notes(&view(config)?) {
        Err(error) => Ok(error),
        Ok(_) => Err("generation succeeded".into()),
    }
}

#[test]
fn a_lint_checks_prebuilt_tools_only_when_it_installs_them() -> TestResult {
    let named = "[[lint]]\nkind = 'mise-task'\ntask = 'check'\ntools = ['jactionlint']\n";
    let files = expected_with_notes(&view(named)?)?.0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    assert!(
        workflow.lines().any(|line| line == "  lint-check:"),
        "{workflow}"
    );
    let missing = DetectError::ToolNotPrebuilt {
        tool: "cargo:ripgrep".to_owned(),
        platform: "linux-x64".to_owned(),
    };
    for config in [
        "[[lint]]\nkind = 'mise-task'\ntask = 'check'\ntools = ['cargo:ripgrep']\n",
        "[[lint]]\nkind = 'mise-task'\ntask = 'check'\ntools = []\n",
    ] {
        assert_eq!(refused(config)?, GenerateError::Detect(missing.clone()));
    }
    Ok(())
}
