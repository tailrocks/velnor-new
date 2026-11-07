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
    seed_stale_workflow(root)?;

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
    assert!(!rendered.contains("stale generated workflow"));
    assert!(!rendered.contains("stale user-owned workflow"));
    assert_manual_native_validator_step(rendered)?;

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

fn seed_stale_workflow(root: &Path) -> Result<(), Box<dyn Error>> {
    let source_workflow = root.join(WORKFLOW_PATH);
    fs::create_dir_all(
        source_workflow
            .parent()
            .ok_or("workflow path has no parent")?,
    )?;
    fs::write(
        &source_workflow,
        format!("{MARKER_PREFIX} 0.1.0\nname: stale generated workflow\n"),
    )?;
    fs::create_dir_all(
        root.join(UNRELATED_PATH)
            .parent()
            .ok_or("unrelated path has no parent")?,
    )?;
    fs::write(root.join(UNRELATED_PATH), UNRELATED_CONTENT)?;

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

fn assert_manual_native_validator_step(rendered: &str) -> Result<(), Box<dyn Error>> {
    let build_step = rendered
        .find("- name: Build source CLI and archive guard concurrently")
        .ok_or("source CLI build step was not rendered")?;
    let qualification_step = rendered
        .find("- name: Author Linux x64 native validator candidates")
        .ok_or("manual native validator qualification step was not rendered")?;
    let capture_step = rendered
        .find("- name: Capture all five external goldens")
        .ok_or("external golden capture step was not rendered")?;
    assert!(build_step < capture_step && capture_step < qualification_step);
    assert_eq!(
        rendered
            .matches("- name: Author Linux x64 native validator candidates")
            .count(),
        1
    );
    let end = rendered[qualification_step + 1..]
        .find("\n      - name:")
        .map_or(rendered.len(), |offset| qualification_step + 1 + offset);
    let manual_qualification = &rendered[qualification_step..end];
    assert!(
        manual_qualification.contains("if: success() && github.event_name == 'workflow_dispatch'")
    );
    assert!(manual_qualification.contains(
        "run: bash scripts/generator-release/source-qualification.sh qualify-linux-x64-native-validator-tools"
    ));
    let probe = rendered
        .find("- name: Observe hosted Docker identity")
        .ok_or("manual Docker observation was not rendered")?;
    let end = rendered[probe + 1..]
        .find("\n      - name:")
        .map_or(rendered.len(), |offset| probe + 1 + offset);
    let manual_probe = &rendered[probe..end];
    assert!(manual_probe.contains("if: success() && github.event_name == 'workflow_dispatch'"));
    assert!(manual_probe.contains(
        "run: bash scripts/generator-release/source-qualification.sh observe-hosted-docker-identity"
    ));
    Ok(())
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

#[test]
fn handwritten_workflow_blocks_generation_without_modifying_existing_files()
-> Result<(), Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let root = repo.path();
    let workflow = root.join(".github/workflows/custom.yml");
    fs::create_dir_all(workflow.parent().ok_or("parent")?)?;
    fs::write(&workflow, "name: custom\n")?;
    let other = root.join(".github/notes.txt");
    fs::write(&other, "preserve me\n")?;
    let prepared = prepare(root)?;
    assert!(generate(&prepared, &GenerateOptions::default()).is_err());
    assert_eq!(fs::read_to_string(&workflow)?, "name: custom\n");
    assert_eq!(fs::read_to_string(other)?, "preserve me\n");
    let config = root.join(".velnor/config.toml");
    fs::write(
        &config,
        format!(
            "{}\n[github]\nallow = [\".github/workflows/custom.yml\"]\n",
            generator_config(true)
        ),
    )?;
    assert!(prepare(root).is_err());
    fs::write(&config, generator_config(true).replace("default_branch = \"testmain\"", "default_branch = \"testmain\"\npreserve_workflows = [\".github/workflows/custom.yml\"]"))?;
    assert!(prepare(root).is_err());
    fs::write(&config, generator_config(true))?;
    fs::write(&workflow, format!("{MARKER_PREFIX} 0.1.0\nname: stale\n"))?;
    let prepared = prepare(root)?;
    let preview = tempfile::tempdir()?;
    let destination = preview.path().join("preview");
    generate(
        &prepared,
        &GenerateOptions {
            output_dir: Some(destination.clone()),
        },
    )?;
    assert!(!destination.join(".github/workflows/custom.yml").exists());
    assert!(workflow.exists());
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir()?;
        let outside_file = outside.path().join("keep.yml");
        fs::write(&outside_file, "outside\n")?;
        fs::remove_dir_all(root.join(".github/workflows"))?;
        std::os::unix::fs::symlink(outside.path(), root.join(".github/workflows"))?;
        assert!(generate(&prepared, &GenerateOptions::default()).is_err());
        assert_eq!(fs::read_to_string(&outside_file)?, "outside\n");
    }
    Ok(())
}
