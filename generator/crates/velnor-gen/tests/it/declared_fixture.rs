// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::release_fixture::{jobs, release_view};
use crate::yaml_tree::{Node, parse};

pub(crate) type TestResult = Result<(), Box<dyn Error>>;

/// A one-job workflow `ci` with the given `on` table and extra workflow keys.
pub(crate) fn workflow(header: &str, on: &str, jobs: &str) -> String {
    format!("[[workflow]]\nfile = \"ci\"\nname = \"CI\"\n{header}\n{on}\n{jobs}\n")
}

/// One job `check` with one `echo` step, ready to append to a workflow.
pub(crate) const JOB: &str = r#"
[[workflow.job]]
id = "check"

[[workflow.job.step]]
name = "Check"
run = "echo ok"
"#;

pub(crate) fn rendered_file(config: &str, file: &str) -> Result<Node, Box<dyn Error>> {
    Ok(parse(&rendered_text(config, file)?)?)
}

pub(crate) fn rendered_text(config: &str, file: &str) -> Result<String, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    view.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        config.as_bytes().to_vec(),
    );
    let mut files = expected_files(&view)?;
    Ok(files
        .remove(&RepoPath::new(format!(".github/workflows/{file}.yml"))?)
        .ok_or("not rendered")?)
}

pub(crate) fn rendered(config: &str) -> Result<Node, Box<dyn Error>> {
    rendered_file(config, "ci")
}

pub(crate) fn refusal(config: &str) -> Result<String, Box<dyn Error>> {
    let mut view = release_view(&[], None)?;
    view.files.insert(
        RepoPath::new(".velnor/workflows.toml")?,
        config.as_bytes().to_vec(),
    );
    match expected_files(&view) {
        Err(error) => Ok(error.to_string()),
        Ok(_) => Err("expected a refusal".into()),
    }
}

pub(crate) fn job<'a>(tree: &'a Node, id: &str) -> Result<&'a Node, Box<dyn Error>> {
    Ok(jobs(tree)?
        .into_iter()
        .find(|(name, _)| *name == id)
        .ok_or("no such job")?
        .1)
}
