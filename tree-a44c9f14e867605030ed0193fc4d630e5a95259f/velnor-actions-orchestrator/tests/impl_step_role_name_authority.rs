//! End-to-end proof that step display names do not select workflow behavior.

use std::fs;
use std::path::Path;

use velnor_actions_contract_workflow::{Job, StepKind, StepRole};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::closure::download_plan_step;
use velnor_actions_workflow_renderer::render::{FINAL_JOB_ID, WORKFLOW_PATH};
use velnor_actions_workflow_renderer::steps::{MERGE_OPERATION, WRITE_REQUEST_OPERATION};

use super::impl_common::{TestResult, config_with_branch, make_repo};

const DEMO_LOCK: &str = "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n";

/// Consumer generation retains the full output tree when typed role names change.
#[test]
fn finalized_generation_is_invariant_to_typed_step_display_names() -> TestResult {
    let repo = authority_fixture()?;
    let mut original = prepare(repo.path())?;
    install_explicit_plan_download(&mut original.workflow.ir.jobs)?;
    assert_finalized_roles(&finalized_jobs(&original)?);
    let original_tree = render_staged_tree(&original)?;
    let mut renamed = original.clone();
    rename_authority_steps(&mut renamed.workflow.ir.jobs);
    let renamed_tree = render_staged_tree(&renamed)?;
    assert_generation_equivalent(&original_tree, &renamed_tree)
}

/// Build a schema-2 Rust, `ToFu`, and MBX fixture with both runner lanes.
fn authority_fixture() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let config = format!(
        "{}\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nmode = \"both\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"ubuntu-26.04-scale-set\", \"velnor\"]\nplatform = \"linux/amd64\"\n[stacks.tofu]\nroots = [\"stacks/a\"]\n",
        config_with_branch()
            .replace("schema = 1", "schema = 2")
            .trim()
    );
    let repo = make_repo(&config)?;
    let root = repo.path();
    fs::write(root.join("Cargo.lock"), DEMO_LOCK)?;
    fs::create_dir_all(root.join("stacks/a"))?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    fs::create_dir_all(root.join(".cargo"))?;
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\nrustc-wrapper = \"mbx\"\n",
    )?;
    Ok(repo)
}

/// Require full job finalization to exercise closures, writers, and MBX lanes.
fn assert_finalized_roles(jobs: &std::collections::BTreeMap<String, Job>) {
    let roles: Vec<StepRole> = jobs
        .values()
        .flat_map(|job| &job.steps)
        .filter_map(|step| step.role)
        .collect();
    for role in [
        StepRole::Checkout,
        StepRole::DownloadPlan,
        StepRole::CargoSourcesSave,
        StepRole::TofuProvidersSave,
        StepRole::ToolsCacheSave,
        StepRole::MbxPreflight,
        StepRole::MbxCache,
        StepRole::MbxVersionCheck,
    ] {
        assert!(
            roles.contains(&role),
            "full finalization must exercise {role:?}"
        );
    }
}

/// Rename every relevant source role and require the fixture has each target.
fn rename_authority_steps(jobs: &mut std::collections::BTreeMap<String, Job>) {
    let mut changed = Vec::new();
    for step in jobs.values_mut().flat_map(|job| &mut job.steps) {
        if let Some(role) = step.role.filter(|role| is_authority_role(*role)) {
            step.name = format!("renamed for role authority: {role:?}");
            changed.push(role);
        }
    }
    for role in [
        StepRole::AcquireVelnor,
        StepRole::DownloadPlan,
        StepRole::CargoSourcesRestore,
        StepRole::CargoSourcesSave,
        StepRole::CargoSourcesFetch,
        StepRole::TofuProvidersRestore,
        StepRole::TofuProviderUse,
        StepRole::MbxPreflight,
        StepRole::MbxCache,
        StepRole::MbxVersionCheck,
    ] {
        assert!(changed.contains(&role), "fixture must rename {role:?}");
    }
}

/// Compare the complete generated trees after removing only step labels.
fn assert_generation_equivalent(
    original: &velnor_actions_workflow_renderer::RenderedTree,
    renamed: &velnor_actions_workflow_renderer::RenderedTree,
) -> TestResult {
    let original_yaml = original
        .get(WORKFLOW_PATH)
        .ok_or("original generated tree misses ci.yml")?;
    let renamed_yaml = renamed
        .get(WORKFLOW_PATH)
        .ok_or("renamed generated tree misses ci.yml")?;
    assert_ne!(
        original_yaml, renamed_yaml,
        "renamed labels reach generated YAML"
    );
    assert_eq!(
        normalize_tree(original),
        normalize_tree(renamed),
        "workflow semantics and companion files are unchanged"
    );
    assert_eq!(original.symlinks, renamed.symlinks);
    assert!(
        original_yaml.contains("__hosted") && original_yaml.contains("__local"),
        "schema-2 both mode expands both verification lanes"
    );
    assert!(
        original
            .files
            .iter()
            .any(|file| file.path.starts_with(".github/actions/")),
        "lane sharing emits its composite action files"
    );
    Ok(())
}

/// Add the same typed `DownloadPlan` step the renderer closure normally inserts.
fn install_explicit_plan_download(
    jobs: &mut std::collections::BTreeMap<String, Job>,
) -> TestResult {
    let final_job = jobs
        .get_mut(FINAL_JOB_ID)
        .ok_or("fixture misses the final aggregation job")?;
    if final_job
        .steps
        .iter()
        .any(|step| step.role == Some(StepRole::DownloadPlan))
    {
        return Err("fixture unexpectedly has a plan download before closure".into());
    }
    let index = final_job
        .steps
        .iter()
        .position(|step| {
            matches!(
                &step.kind,
                StepKind::Internal { operation, .. }
                    if operation == WRITE_REQUEST_OPERATION || operation == MERGE_OPERATION
            )
        })
        .ok_or("fixture final job has no insertion anchor")?;
    final_job.steps.insert(index, download_plan_step()?);
    Ok(())
}

/// Roles whose names historically could have been mistaken for selectors.
fn is_authority_role(role: StepRole) -> bool {
    matches!(
        role,
        StepRole::AcquireVelnor
            | StepRole::DownloadPlan
            | StepRole::CargoSourcesRestore
            | StepRole::CargoSourcesSave
            | StepRole::CargoSourcesFetch
            | StepRole::CargoRegistryRestore
            | StepRole::ToolsCacheSave
            | StepRole::TofuProvidersRestore
            | StepRole::TofuProviderUse
            | StepRole::TofuProvidersSave
            | StepRole::MbxPreflight
            | StepRole::MbxCache
            | StepRole::MbxVersionCheck
    )
}

/// Replace only serialized step labels while retaining every semantic field.
fn normalize_tree(tree: &velnor_actions_workflow_renderer::RenderedTree) -> Vec<(String, String)> {
    tree.files
        .iter()
        .map(|file| {
            let is_yaml = Path::new(&file.path)
                .extension()
                .is_some_and(|extension| extension == "yml");
            let bytes = if is_yaml {
                file.bytes
                    .lines()
                    .map(|line| {
                        let indentation = line.len() - line.trim_start().len();
                        if line.trim_start().starts_with("- name:") {
                            format!("{}- name: <presentation>", " ".repeat(indentation))
                        } else {
                            line.to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                file.bytes.clone()
            };
            (file.path.clone(), bytes)
        })
        .collect()
}
