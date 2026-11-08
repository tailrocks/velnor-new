// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn both_release_families_and_preview_bound_every_publication_script() -> Result<(), Box<dyn Error>>
{
    let mut request = super::super::request()?;
    request
        .workflows
        .insert(velnor_actions_contract::RoutingWorkflow::MacosBinaryRelease);
    request
        .workflows
        .insert(velnor_actions_contract::RoutingWorkflow::ImageRelease);
    let pins = request.product_release.as_mut().ok_or("missing pins")?;
    pins.install_runner_build_tools_argv = vec!["true".to_owned()];
    pins.runner_build_argv = vec!["true".to_owned()];
    pins.attestation_helper_build_argv = vec!["true".to_owned()];
    pins.install_resource_probe_target_argv = vec!["true".to_owned()];
    pins.resource_probe_build_argv = vec!["true".to_owned()];
    pins.install_resource_probe_release_tools_argv = vec!["true".to_owned()];
    pins.resource_probe_release_build_argv = vec!["true".to_owned()];
    pins.resource_probe_release_test_argv = vec!["true".to_owned()];
    let files = velnor_actions_workflow_renderer::render_schema2_workflows(&request)?;
    for file in &files {
        if file.path.starts_with(".github/workflows/") {
            for line in file
                .bytes
                .lines()
                .filter_map(|line| line.trim_start().strip_prefix("run: "))
            {
                if line.starts_with('"') {
                    let run: String = serde::Deserialize::deserialize(
                        &mut serde_json::Deserializer::from_str(line),
                    )?;
                    assert!(run.chars().count() <= 21_000, "{}", file.path);
                }
            }
        }
    }
    for (file, id, name) in CASES {
        let workflow = find_file(&files, &format!(".github/workflows/{file}"))?;
        let section = job(workflow, id)?;
        let source = materialized_script(section)?;
        assert!(source.chars().count() > 21_000, "{file} {id}");
        let run = step_script(section, name)?;
        assert!(run.chars().count() <= 21_000, "{file} {id}");
        assert!(
            run.contains("VELNOR_FIXED_SCRIPT_DIR/script.sh"),
            "{file} {id}"
        );
    }
    Ok(())
}

const CASES: [(&str, &str, &str); 10] = [
    (
        "job-image-candidate.yml",
        "attest-image-candidate",
        "Materialize fixed image authority implementation",
    ),
    (
        "job-image-candidate.yml",
        "publish-job-image",
        "Materialize fixed image authority implementation",
    ),
    (
        "product-release-images.yml",
        "publish-images",
        "Publish GitHub release",
    ),
    (
        "product-release.yml",
        "prepare-generator",
        "Reconcile immutable family release",
    ),
    (
        "product-release-generator.yml",
        "publish-generator",
        "Publish GitHub release",
    ),
    (
        "product-release-binary.yml",
        "publish-binary",
        "Publish GitHub release",
    ),
    (
        "source-preview-publication.yml",
        "source-preview-source",
        "Admit annotated SDK preview tag",
    ),
    (
        "source-preview-publication.yml",
        "source-preview-draft",
        "Create exact preview draft",
    ),
    (
        "source-preview-publication.yml",
        "source-preview-attest",
        "Select or finalize signed candidate",
    ),
    (
        "source-preview-publication.yml",
        "source-preview-publish",
        "Verify and publish preview",
    ),
];
