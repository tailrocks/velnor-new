// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_with_notes;
use velnor_model::WORKFLOW_PATH;

use crate::stacks::ansible_repository;

type TestResult = Result<(), Box<dyn Error>>;

const IMPORT: &str =
    "PYTHONPATH=\"$RUNNER_TEMP/ansible-clean:$RUNNER_TEMP/ansible-tools:$RUNNER_TEMP/ansible\"";

fn runs(workflow: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let tree = crate::yaml_tree::document(workflow)?;
    let job = tree
        .get("jobs")
        .and_then(|jobs| jobs.get("ansible"))
        .ok_or("no ansible job")?;
    Ok(crate::lint_lane::runs(job))
}

fn position(steps: &[String], marker: &str) -> Result<usize, Box<dyn Error>> {
    steps
        .iter()
        .position(|run| run.contains(marker))
        .ok_or_else(|| format!("no step with {marker}").into())
}

#[test]
fn each_tool_is_installed_once_into_its_own_directory_that_the_imports_put_first() -> TestResult {
    let files = expected_with_notes(&ansible_repository(
        "---\n- hosts: all\n  tasks: []\n",
        "ansible-configs/site.yml",
    )?)?
    .0;
    let workflow = files
        .get(&WORKFLOW_PATH)
        .ok_or("generate did not write .github/workflows/ci.yml")?;
    let steps = runs(workflow)?;
    let pip: Vec<_> = steps
        .iter()
        .filter(|run| run.contains("pip install"))
        .collect();
    assert_eq!(pip.len(), 2, "{steps:?}");
    for run in &pip {
        assert!(run.contains("--only-binary=:all:"), "{run}");
        assert!(run.contains("--require-hashes"), "{run}");
        assert!(!run.contains("--upgrade"), "{run}");
    }
    assert!(steps.iter().all(|run| !run.contains("rm -rf")), "{steps:?}");
    let others = position(&steps, "-r \"$RUNNER_TEMP/ansible-requirements.txt\"")?;
    let tools = position(&steps, "-r \"$RUNNER_TEMP/ansible-tools.txt\"")?;
    assert!(
        steps[others].contains("--target \"$RUNNER_TEMP/ansible\" "),
        "{}",
        steps[others]
    );
    assert!(
        steps[tools].contains("--target \"$RUNNER_TEMP/ansible-tools\" "),
        "{}",
        steps[tools]
    );
    let pins = position(&steps, "ansible-core==2.16.14")?;
    assert_eq!(
        steps
            .iter()
            .filter(|run| run.contains("ansible-core==2.16.14"))
            .count(),
        1,
        "{steps:?}"
    );
    let filter = position(&steps, "ansible-lint==(?:")?;
    assert!(
        steps[filter].contains("ansible-configs/requirements.txt"),
        "{}",
        steps[filter]
    );
    let hook = steps
        .iter()
        .position(|run| {
            run.contains("sitecustomize.py\").write_text(\"\")")
                && run.contains("usercustomize.py\").write_text(\"\")")
                && run.contains("ansible-clean")
        })
        .ok_or("no empty site hooks")?;
    let galaxy = position(&steps, "ansible.cli.galaxy")?;
    assert!(!steps[hook].contains("pip install"), "{}", steps[hook]);
    assert!(pins < tools && filter < others, "{steps:?}");
    assert!(others < hook && tools < hook && hook < galaxy, "{steps:?}");
    assert!(steps[galaxy].starts_with(IMPORT), "{}", steps[galaxy]);
    Ok(())
}
