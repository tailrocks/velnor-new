// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::gh_expr::{Value, evaluate};
use crate::golden::{load, samples};

type TestResult = Result<(), Box<dyn Error>>;

// The sample is the repository itself. Its Documentation steps must run on a
// pull request, so a rustdoc break is caught before the merge (not only after
// it, as happened with the unresolved `[tools]` link of #2223).
fn rust_ci_workflow() -> Result<String, Box<dyn Error>> {
    let files = expected_files(&load(&samples().join("velnor-new"))?)?;
    let workflow = files
        .get(&RepoPath::new(".github/workflows/rust-ci.yml")?)
        .ok_or("no generated workflow")?;
    Ok(workflow.clone())
}

// The step condition as a pull request sees it: every plan output says the
// docs run and every outcome succeeded.
fn resolve(event: &'static str) -> impl Fn(&str) -> Result<Value, String> {
    move |path: &str| {
        Ok(Value::Str(match path {
            "github.event_name" => event.to_owned(),
            _ if path.ends_with(".outcome") => "success".to_owned(),
            _ if path.ends_with("rust_root_docs") => "run".to_owned(),
            _ if path.ends_with("rust_root_mode") => "some".to_owned(),
            _ if path.ends_with("rust_root_package_args") => "--workspace".to_owned(),
            _ => String::new(),
        }))
    }
}

#[test]
fn every_documentation_step_runs_on_a_pull_request() -> TestResult {
    let tree = crate::yaml_tree::document(&rust_ci_workflow()?)?;
    let mut checked = 0;
    for job in tree["jobs"].as_object().ok_or("no jobs")?.values() {
        for step in job["steps"].as_array().into_iter().flatten() {
            let name = step["name"].as_str().unwrap_or_default();
            if !name.starts_with("Documentation") {
                continue;
            }
            checked += 1;
            let condition = step["if"].as_str().ok_or("no if")?;
            let runs = evaluate(condition, &resolve("pull_request"))?.truthy();
            assert!(runs, "{name} on pull_request: {condition}");
        }
    }
    assert_ne!(
        checked, 0,
        "no Documentation step in the generated workflow"
    );
    Ok(())
}
