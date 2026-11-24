// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_with_notes;
use velnor_model::WORKFLOW_PATH;

use crate::stacks::ansible_repository;

type TestResult = Result<(), Box<dyn Error>>;

const IMPORT: &str = "PYTHONPATH=\"$RUNNER_TEMP/ansible-clean:$RUNNER_TEMP/ansible\"";
const REMOVE: &str = "rm -rf \"$RUNNER_TEMP/ansible/ansible\" \"$RUNNER_TEMP/ansible/ansiblelint\"";

fn runs(workflow: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let tree = crate::yaml_tree::document(workflow)?;
    let job = tree
        .get("jobs")
        .and_then(|jobs| jobs.get("ansible"))
        .ok_or("no ansible job")?;
    Ok(crate::lint_lane::runs(job))
}

#[test]
fn the_repair_install_unlinks_planted_tool_directories_and_imports_through_empty_site_hooks()
-> TestResult {
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
        .enumerate()
        .filter(|(_, run)| run.contains("pip install"))
        .map(|(index, _)| index)
        .collect();
    assert_eq!(pip.len(), 2, "{steps:?}");
    for index in &pip {
        assert!(
            steps[*index].contains("--only-binary=:all:"),
            "{}",
            steps[*index]
        );
    }
    let remove_at = steps
        .iter()
        .position(|run| run == REMOVE)
        .ok_or("no rm of the planted tool paths")?;
    let upgrade_at = pip
        .iter()
        .copied()
        .find(|index| steps[*index].contains("--upgrade"))
        .ok_or("no upgrade install")?;
    let hook_at = steps
        .iter()
        .position(|run| {
            run.contains("sitecustomize.py\").write_text(\"\")")
                && run.contains("usercustomize.py\").write_text(\"\")")
                && run.contains("ansible-clean")
        })
        .ok_or("no empty site hooks")?;
    let galaxy_at = steps
        .iter()
        .position(|run| run.contains("ansible.cli.galaxy"))
        .ok_or("no galaxy step")?;
    assert!(
        !steps[hook_at].contains("pip install"),
        "{}",
        steps[hook_at]
    );
    assert!(pip[0] < remove_at && remove_at < upgrade_at);
    assert!(upgrade_at < hook_at && hook_at < galaxy_at);
    assert!(remove_at < galaxy_at);
    assert!(steps[galaxy_at].starts_with(IMPORT), "{}", steps[galaxy_at]);
    Ok(())
}
