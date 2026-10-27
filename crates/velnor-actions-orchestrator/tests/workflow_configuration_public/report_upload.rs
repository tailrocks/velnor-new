// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::{Job, REQUIRED_JOB_ID, StepKind};
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::parallel_owners::{assert_package_tasks_are_not_planned, assert_rust_owner_jobs};
use super::support::{
    discovered_rust_packages, logical_owner_id, rendered_job_block, rust_owner_jobs,
    task_execution_steps,
};
use super::workflow_configuration_fixture;

#[test]
fn prepare_and_render_upload_owner_reports_after_parallel_checks() -> Result<(), Box<dyn Error>> {
    let config = workflow_configuration_fixture::config_with_verification_branch(None);
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    workflow_configuration_fixture::add_independent_mbx_package(root, "crates/nested", "nested")?;
    workflow_configuration_fixture::write_nextest_task(root)?;
    workflow_configuration_fixture::write_nextest_task(&root.join("crates/nested"))?;

    let prepared = prepare(root)?;
    let rendered = render_staged_tree(&prepared)?;
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;
    let root_workspace = prepared
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root.is_empty())
        .ok_or("root Cargo workspace is missing")?;
    assert_eq!(root_workspace.profile.compile_driver.as_str(), "cargo");
    assert_eq!(root_workspace.profile.test_runner.as_str(), "cargo_nextest");
    let nested = prepared
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root == "crates/nested")
        .ok_or("nested MBX workspace is missing")?;
    assert_eq!(nested.profile.compile_driver.as_str(), "mbx");
    assert_eq!(nested.profile.test_runner.as_str(), "cargo_nextest");

    let (unit_paths, expected_packages) = discovered_rust_packages(&prepared);
    assert_package_tasks_are_not_planned(&prepared, &unit_paths)?;
    let owner_jobs = rust_owner_jobs(&prepared);
    let required = prepared
        .workflow
        .ir
        .jobs
        .get(REQUIRED_JOB_ID)
        .ok_or("missing Required job")?;
    assert_rust_owner_jobs(
        &owner_jobs,
        required,
        &unit_paths,
        &expected_packages,
        workflow,
    )?;

    // Both branches upload under their logical owner's stable name: the
    // branches run on mutually exclusive events and artifact names are
    // run-scoped, so reader and writer never collide.
    let all: BTreeSet<&str> = owner_jobs.iter().map(|(id, _)| *id).collect();
    let mut artifact_names = BTreeMap::new();
    for (owner_id, job) in &owner_jobs {
        let owner = logical_owner_id(&all, owner_id);
        let artifact_name = assert_crate_report_upload_ir(owner, job)?;
        match artifact_names.entry(owner) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(artifact_name.clone());
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                assert_eq!(entry.get(), &artifact_name);
            }
        }
        assert_rendered_crate_report_upload(workflow, owner_id, &artifact_name, job)?;
    }
    assert_eq!(artifact_names.len(), expected_packages.len());
    Ok(())
}

fn assert_crate_report_upload_ir(owner_id: &str, job: &Job) -> Result<String, Box<dyn Error>> {
    let uploads: Vec<_> = job
        .steps
        .iter()
        .filter(|step| step.name == velnor_actions_workflow_renderer::CRATE_REPORT_UPLOAD_NAME)
        .collect();
    assert_eq!(uploads.len(), 1, "one report upload for {owner_id}");
    let upload = uploads[0];
    let StepKind::Action { uses, with, .. } = &upload.kind else {
        return Err(format!("{owner_id} report upload is not an action").into());
    };
    assert_eq!(
        uses,
        "actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9"
    );
    assert!(!upload.parallel);
    assert_eq!(upload.condition, None);

    let expected_name = format!(
        "velnor-crate-r${{{{ github.run_id }}}}-a${{{{ env.VELNOR_RUN_KEY_ATTEMPT }}}}-{owner_id}"
    );
    assert_eq!(
        with.get("name").map(String::as_str),
        Some(expected_name.as_str())
    );
    assert_eq!(
        with.get("path").map(String::as_str),
        Some("${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}")
    );
    assert_eq!(
        with.get("if-no-files-found").map(String::as_str),
        Some("error")
    );
    assert!(
        !job.steps.iter().any(|step| {
            step.name == velnor_actions_workflow_renderer::MATRIX_REPORT_UPLOAD_NAME
        })
    );

    let executions = task_execution_steps(job);
    assert!(!executions.is_empty(), "{owner_id} has task executions");
    assert!(executions.iter().filter(|step| step.3).count() >= 2);
    let upload_index = job
        .steps
        .iter()
        .position(|step| step.name == velnor_actions_workflow_renderer::CRATE_REPORT_UPLOAD_NAME)
        .ok_or("report upload is missing")?;
    assert!(executions.iter().all(|(index, ..)| *index < upload_index));
    Ok(expected_name)
}

fn assert_rendered_crate_report_upload(
    workflow: &str,
    owner_id: &str,
    expected_name: &str,
    job: &Job,
) -> Result<(), Box<dyn Error>> {
    let job_yaml = rendered_job_block(workflow, owner_id)?;
    let upload_marker = "      - name: Upload crate reports\n";
    assert_eq!(job_yaml.matches(upload_marker).count(), 1);
    assert!(!job_yaml.contains("      - name: Upload matrix report\n"));

    let upload_start = job_yaml
        .find(upload_marker)
        .ok_or("report upload is missing")?;
    let upload_tail = &job_yaml[upload_start..];
    let upload_end = upload_tail
        .find("\n      - name: ")
        .unwrap_or(upload_tail.len());
    let upload_yaml = &upload_tail[..upload_end];
    assert!(upload_yaml.contains("        if: always()\n"));
    assert!(upload_yaml.contains(
        "        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9\n"
    ));
    assert!(upload_yaml.contains(&format!("          name: {expected_name}\n")));
    assert!(upload_yaml.contains(
        "          path: ${{ runner.temp }}/velnor/r${{ github.run_id }}-a${{ env.VELNOR_RUN_KEY_ATTEMPT }}\n"
    ));
    assert!(upload_yaml.contains("          if-no-files-found: error\n"));

    let parallel_count = task_execution_steps(job)
        .iter()
        .filter(|step| step.3)
        .count();
    assert!(parallel_count >= 2);
    assert_eq!(job_yaml.matches("background: true").count(), parallel_count);
    let last_background = job_yaml
        .rfind("background: true")
        .ok_or("parallel task steps are missing")?;
    let wait_all = job_yaml
        .find("- name: Wait for parallel checks\n        wait-all:")
        .ok_or("parallel wait-all step is missing")?;
    assert!(last_background < wait_all && wait_all < upload_start);
    Ok(())
}
