// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::TempDir;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use velnor_runner::expr::{ExprEnv, ExprError, evaluate_condition, evaluate_template};

const MATRIX: &str = "unsupported expression context matrix";

const OUT_OF_SCOPE: [(&str, &str); 3] = [
    (
        "generator/samples/declared-workflows/expected/.github/workflows/build-assets.yml",
        MATRIX,
    ),
    (
        "generator/samples/declared-workflows/expected/.github/workflows/tools-build.yml",
        MATRIX,
    ),
    (
        "generator/samples/reusable-workflows/expected/.github/workflows/native-checks.yml",
        MATRIX,
    ),
];

const SERVICE_JOB_KEYS: [&str; 12] = [
    "runs-on",
    "if",
    "name",
    "needs",
    "strategy",
    "concurrency",
    "timeout-minutes",
    "continue-on-error",
    "permissions",
    "uses",
    "with",
    "secrets",
];

const SERVICE_ROOT_KEYS: [&str; 5] = ["on", "name", "run-name", "concurrency", "permissions"];

const COVERED: [&str; 4] = ["endsWith(", "vars.", "failure()", "VELNOR_CACHE_DEBUG"];

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workflow_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![root.join(".github/workflows")];
    for sample in fs::read_dir(root.join("generator/samples"))? {
        let expected = sample?.path().join("expected");
        if expected.is_dir() {
            pending.push(expected);
        }
    }
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).with_context(|| directory.display().to_string())? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "yml" || ext == "yaml")
                && path.parent().and_then(Path::file_name) == Some("workflows".as_ref())
            {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn service_evaluated(path: &[&str], value: &serde_json::Value) -> bool {
    match path {
        ["jobs", _, "environment"] => !value.is_object(),
        ["jobs", _, "environment", "url", ..] | [] => false,
        ["jobs", _, "environment", ..] => true,
        ["jobs", _, key, ..] => SERVICE_JOB_KEYS.contains(key),
        [root, ..] => SERVICE_ROOT_KEYS.contains(root),
    }
}

fn collect(value: &serde_json::Value, path: &[&str], sources: &mut BTreeSet<(bool, String)>) {
    match value {
        _ if service_evaluated(path, value) => {}
        serde_json::Value::String(text) if text.contains("${{") => {
            sources.insert((false, text.clone()));
        }
        serde_json::Value::String(text) if path.last() == Some(&"if") => {
            sources.insert((true, text.clone()));
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect(item, path, sources);
            }
        }
        serde_json::Value::Object(entries) => {
            for (name, item) in entries {
                let mut child = path.to_vec();
                child.push(name);
                collect(item, &child, sources);
            }
        }
        _ => {}
    }
}

fn parse_error(is_condition: bool, source: &str, env: &ExprEnv) -> Option<String> {
    let result = if is_condition {
        evaluate_condition(source, env).map(drop)
    } else {
        evaluate_template(source, env).map(drop)
    };
    match result {
        Err(ExprError::Parse(error)) => Some(error.to_string()),
        Ok(()) | Err(ExprError::Evaluation(_)) => None,
    }
}

struct Report {
    corpus: BTreeSet<String>,
    known_hit: BTreeSet<usize>,
    unexpected: Vec<String>,
}

fn report(root: &Path) -> Result<Report> {
    let env = ExprEnv::new(root.to_path_buf());
    let mut report = Report {
        corpus: BTreeSet::new(),
        known_hit: BTreeSet::new(),
        unexpected: Vec::new(),
    };
    for path in workflow_files(root)? {
        let text = fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        let document: serde_json::Value =
            serde_saphyr::from_str(&text).with_context(|| path.display().to_string())?;
        let relative = path.strip_prefix(root)?.to_string_lossy().into_owned();
        let mut sources = BTreeSet::new();
        collect(&document, &[], &mut sources);
        for (is_condition, source) in sources {
            if let Some(error) = parse_error(is_condition, &source, &env) {
                let known = OUT_OF_SCOPE
                    .iter()
                    .position(|(file, message)| relative == *file && error == *message);
                match known {
                    Some(index) => {
                        report.known_hit.insert(index);
                    }
                    None => report
                        .unexpected
                        .push(format!("{relative}: {source}: {error}")),
                }
            }
            report.corpus.insert(source);
        }
    }
    Ok(report)
}

#[test]
fn expr_generated_workflow_expressions_evaluate() -> Result<()> {
    let report = report(&repository_root())?;
    assert!(
        report.unexpected.is_empty(),
        "{}",
        report.unexpected.join("\n")
    );
    assert_eq!(
        report.known_hit.len(),
        OUT_OF_SCOPE.len(),
        "an out-of-scope entry no longer fails to parse"
    );
    for gap in COVERED {
        assert!(
            report.corpus.iter().any(|source| source.contains(gap)),
            "no generated workflow uses {gap}"
        );
    }
    Ok(())
}

#[test]
fn expr_generated_workflow_reads_environment_url_and_yaml() -> Result<()> {
    let root = TempDir::new()?;
    fs::create_dir_all(root.path().join("generator/samples"))?;
    root.write(
        ".github/workflows/deploy.yaml",
        "jobs:\n  deploy:\n    environment:\n      name: ${{ toYAML(1) }}\n      url: ${{ toYAML(2) }}\n  other:\n    environment: ${{ toYAML(3) }}\n",
    )?;

    let report = report(root.path())?;

    assert_eq!(
        report.unexpected.len(),
        1,
        "{}",
        report.unexpected.join("\n")
    );
    assert!(report.unexpected[0].starts_with(".github/workflows/deploy.yaml: ${{ toYAML(2) }}: "));
    Ok(())
}
