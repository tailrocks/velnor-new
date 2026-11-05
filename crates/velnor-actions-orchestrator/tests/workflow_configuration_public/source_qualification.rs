// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use velnor_actions_contract::{
    RESERVED_SUITE_WORKFLOW_SLUGS, RustSuiteConfig, validate_rust_suites,
};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::MARKER_PREFIX;

use super::workflow_configuration_fixture;

pub(super) const WORKFLOW_PATH: &str = ".github/workflows/source-qualification.yml";
const UNRELATED_PATH: &str = ".github/ISSUE_TEMPLATE/config.yml";
const UNRELATED_CONTENT: &str = "name: preserved user content\n";

#[test]
fn generator_release_owns_source_qualification_lifecycle() -> Result<(), Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let root = repo.path();
    let source_workflow = root.join(WORKFLOW_PATH);
    fs::create_dir_all(
        source_workflow
            .parent()
            .ok_or("workflow path has no parent")?,
    )?;
    fs::write(&source_workflow, "name: stale user-owned workflow\n")?;
    fs::create_dir_all(
        root.join(UNRELATED_PATH)
            .parent()
            .ok_or("unrelated path has no parent")?,
    )?;
    fs::write(root.join(UNRELATED_PATH), UNRELATED_CONTENT)?;

    let prepared = prepare(root)?;
    let tree = render_staged_tree(&prepared)?;
    let generated = tree
        .get(WORKFLOW_PATH)
        .ok_or("GeneratorRelease did not render its source qualification companion")?;
    assert!(generated.starts_with(MARKER_PREFIX));
    let rendered = generated;
    assert!(rendered.contains("pull_request: {}"));
    assert!(rendered.contains("workflow_dispatch: {}"));
    assert!(!rendered.contains("push:"));
    assert!(!rendered.contains("schedule:"));
    assert!(!rendered.contains("workflow_run:"));
    assert!(rendered.contains("permissions:\n  contents: read"));
    assert!(!rendered.contains("contents: write"));
    assert!(!rendered.contains("stale user-owned workflow"));

    let previews = tempfile::tempdir()?;
    let first = preview_generation(&prepared, previews.path().join("enabled-first"))?;
    let second = preview_generation(&prepared, previews.path().join("enabled-second"))?;
    let first_workflow = fs::read(
        first.join(".github").join(
            WORKFLOW_PATH
                .strip_prefix(".github/")
                .ok_or("workflow path prefix")?,
        ),
    )?;
    let second_workflow = fs::read(
        second.join(".github").join(
            WORKFLOW_PATH
                .strip_prefix(".github/")
                .ok_or("workflow path prefix")?,
        ),
    )?;
    assert_eq!(first_workflow, second_workflow);
    let unrelated = Path::new(".github").join(
        UNRELATED_PATH
            .strip_prefix(".github/")
            .ok_or("unrelated path prefix")?,
    );
    assert_eq!(
        fs::read(first.join(&unrelated))?,
        UNRELATED_CONTENT.as_bytes()
    );
    assert_eq!(
        fs::read(second.join(&unrelated))?,
        UNRELATED_CONTENT.as_bytes()
    );

    fs::write(root.join(".velnor/config.toml"), generator_config(false))?;
    let disabled = prepare(root)?;
    let retired = preview_generation(&disabled, previews.path().join("disabled"))?;
    assert!(
        !retired
            .join(".github")
            .join(
                WORKFLOW_PATH
                    .strip_prefix(".github/")
                    .ok_or("workflow path prefix")?
            )
            .exists()
    );
    assert_eq!(
        fs::read(retired.join(&unrelated))?,
        UNRELATED_CONTENT.as_bytes()
    );
    Ok(())
}

#[test]
fn source_qualification_reservations_reject_collisions_and_allow_unrelated_entries() {
    assert!(RESERVED_SUITE_WORKFLOW_SLUGS.contains(&"source-qualification"));
    let mut suite = RustSuiteConfig::new("project-check", "default");
    suite.workflow = Some("source-qualification".to_owned());
    let suite_error = validate_rust_suites(&[suite], ".velnor/config.toml")
        .expect_err("generator-owned workflow slug must reject suite collision");
    assert!(
        suite_error
            .to_string()
            .contains("reserved_suite_workflow:source-qualification")
    );

    let mut unrelated = RustSuiteConfig::new("project-check", "default");
    unrelated.workflow = Some("project-checks".to_owned());
    validate_rust_suites(&[unrelated], ".velnor/config.toml")
        .expect("unrelated suite workflow slug must remain valid");
}

pub(super) fn generator_repo(enabled: bool) -> Result<TempDir, Box<dyn Error>> {
    let repo = workflow_configuration_fixture::make_repo(&generator_config(enabled))?;
    Ok(repo)
}

fn generator_config(enabled: bool) -> String {
    let workflows = if enabled {
        "workflows = [\"generator_release\"]\n"
    } else {
        "workflows = []\n"
    };
    format!(
        "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n{workflows}\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n"
    )
}

fn preview_generation(
    preparation: &velnor_actions_orchestrator::GenerationPreparation,
    destination: PathBuf,
) -> Result<PathBuf, Box<dyn Error>> {
    generate(
        preparation,
        &GenerateOptions {
            output_dir: Some(destination.clone()),
        },
    )?;
    Ok(destination)
}
