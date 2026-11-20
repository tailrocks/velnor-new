// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use velnor_model::{InputsCli, RepoPath, StepKind, WORKFLOW_PATH};
use velnor_render::render;

use crate::rules::{TestResult, plan};

#[test]
fn one_command_steps_render_a_single_line_each() -> TestResult {
    let manifest = RepoPath::new("Cargo.toml")?;
    let target = RepoPath::new("target")?;
    let files = render(&plan(vec![
        StepKind::InputsSave {
            target: target.clone(),
            cli: InputsCli::Batched,
        },
        StepKind::InputsRestore {
            roots: vec![(manifest, target)],
            cli: InputsCli::Batched,
        },
        StepKind::GeneratorCheck,
    ])?)
    .map_err(|found| format!("{found:?}"))?;
    let text = files.get(&WORKFLOW_PATH).ok_or("no workflow")?;
    for line in [
        "run: 'velnor-ci inputs save --target-dir target --scope-cache \"${RUNNER_TEMP}/velnor-input-scopes-${GITHUB_JOB}.json\"'",
        "run: 'velnor-ci inputs restore-all --root-target \"${GITHUB_WORKSPACE}\"/. \"${GITHUB_WORKSPACE}\"/target --scope-cache \"${RUNNER_TEMP}/velnor-input-scopes-${GITHUB_JOB}.json\"'",
        "run: velnor-gen check",
    ] {
        assert!(text.contains(line), "{line} missing in {text}");
    }
    Ok(())
}

fn multi_line_runs(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(value) = line.trim_start().strip_prefix("run:") else {
            continue;
        };
        let value = value.trim();
        if value.is_empty()
            && lines
                .get(index + 1)
                .is_some_and(|next| next.trim_start().starts_with("shell:"))
        {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let quoted_open = ["'", "\""]
            .iter()
            .any(|quote| value.starts_with(quote) && (value.len() == 1 || !value.ends_with(quote)));
        if value.starts_with(['|', '>']) {
            let block: Vec<&str> = lines[index + 1..]
                .iter()
                .take_while(|next| next.starts_with(&" ".repeat(indent + 1)))
                .map(|next| next.trim_start())
                .collect();
            found.push(block.join("\n"));
        } else if value.is_empty() || quoted_open {
            found.push(value.to_owned());
        }
    }
    found
}

#[test]
fn no_golden_workflow_has_a_multi_line_run() -> TestResult {
    let rejected = [
        "run: |\n  echo one\n  echo two\n",
        "run: >\n  echo one\n",
        "run: 'echo one\n  echo two'\n",
        "run:\n  echo one\n",
    ];
    assert_eq!(
        multi_line_runs("run: cargo-fmt --all --check\n"),
        Vec::<String>::new()
    );
    for text in rejected {
        assert_eq!(multi_line_runs(text).len(), 1, "{text}");
    }

    let samples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../generator/samples");
    let mut checked = 0;
    for sample in fs::read_dir(samples)? {
        let workflows = sample?.path().join("expected/.github/workflows");
        for workflow in fs::read_dir(workflows)? {
            let path = workflow?.path();
            let found = multi_line_runs(&fs::read_to_string(&path)?);
            assert!(found.is_empty(), "{}: {found:?}", path.display());
            checked += 1;
        }
    }
    assert!(checked > 0);
    Ok(())
}
