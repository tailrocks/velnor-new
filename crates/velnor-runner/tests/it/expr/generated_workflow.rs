// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use velnor_runner::expr::{ExprEnv, evaluate_condition, evaluate_template};

const RENDER_ONLY_EXPRESSIONS: [&str; 3] = [
    "failure()",
    "!cancelled() && vars.VELNOR_CACHE_DEBUG == '1' && steps.restored.outcome == 'success'",
    "vars.VELNOR_CACHE_DEBUG == '1' && 'fingerprint.log' || ''",
];

const WORKFLOWS: [&str; 3] = [
    ".github/workflows/ci.yml",
    "generator/samples/image-ghcr/expected/.github/workflows/ci.yml",
    "generator/samples/image-ghcr/expected/.github/workflows/image.yml",
];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workflow_files() -> Vec<PathBuf> {
    let root = repository_root();
    WORKFLOWS.iter().map(|path| root.join(path)).collect()
}

fn collect(value: &serde_json::Value, key: &str, sources: &mut Vec<(bool, String)>) {
    match value {
        serde_json::Value::String(text) if text.contains("${{") => {
            sources.push((false, text.clone()));
        }
        serde_json::Value::String(text) if key == "if" => sources.push((true, text.clone())),
        serde_json::Value::Array(items) => {
            for item in items {
                collect(item, key, sources);
            }
        }
        serde_json::Value::Object(entries) => {
            for (name, item) in entries {
                collect(item, name, sources);
            }
        }
        _ => {}
    }
}

fn assert_evaluates(is_condition: bool, source: &str) -> Result<()> {
    let env = ExprEnv::new(repository_root());
    if is_condition {
        evaluate_condition(source, &env).map(drop)
    } else {
        evaluate_template(source, &env).map(drop)
    }
    .with_context(|| source.to_owned())
}

#[test]
fn expr_generated_workflow_expressions_evaluate() -> Result<()> {
    let mut sources = Vec::new();
    for path in workflow_files() {
        let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        let document: serde_json::Value =
            serde_saphyr::from_str(&text).with_context(|| path.display().to_string())?;
        collect(&document, "", &mut sources);
    }
    for (is_condition, source) in &sources {
        assert_evaluates(*is_condition, source)?;
    }
    for gap in ["endsWith(", "vars."] {
        assert!(
            sources.iter().any(|(_, source)| source.contains(gap)),
            "no checked-in workflow uses {gap}"
        );
    }
    for source in RENDER_ONLY_EXPRESSIONS {
        assert_evaluates(true, source)?;
    }
    Ok(())
}
