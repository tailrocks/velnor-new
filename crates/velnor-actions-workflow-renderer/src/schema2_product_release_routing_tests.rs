use std::collections::BTreeMap;
use std::error::Error;

use velnor_actions_contract::{ExecutionMode, RoutingWorkflow};

use crate::schema2::{
    PRODUCT_RELEASE_WORKFLOW, render_schema2_workflows,
    render_schema2_workflows_with_verified_routing_and_overrides,
};

use super::request;

const GENERATOR_ACTIONS: [&str; 12] = [
    "generator-release-attest-linux",
    "generator-release-attest-macos",
    "generator-release-attest-macos-intel",
    "generator-release-attest-manifest",
    "generator-release-build-linux",
    "generator-release-build-macos",
    "generator-release-build-macos-intel",
    "generator-release-publish",
    "generator-release-qualify-homebrew",
    "generator-release-qualify-linux",
    "generator-release-qualify-macos",
    "generator-release-qualify-macos-intel",
];

fn product_files(
    families: &[RoutingWorkflow],
    mode: ExecutionMode,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let request = request(families, mode)?;
    let rendered = render_schema2_workflows(&request)?;
    let mut files = BTreeMap::new();
    for file in rendered
        .into_iter()
        .filter(|file| file.path != crate::schema2::schema2_job_image_candidate::WORKFLOW_PATH)
    {
        assert!(
            files.insert(file.path, file.bytes).is_none(),
            "product output path was rendered more than once"
        );
    }
    Ok(files)
}

fn expected_product_paths(families: &[RoutingWorkflow]) -> Vec<String> {
    let mut paths = vec![crate::schema2::PRODUCT_RELEASE_WORKFLOW.to_owned()];
    for (family, path) in [
        (
            RoutingWorkflow::ImageRelease,
            ".github/workflows/product-release-images.yml",
        ),
        (
            RoutingWorkflow::MacosBinaryRelease,
            ".github/workflows/product-release-binary.yml",
        ),
        (
            RoutingWorkflow::GeneratorRelease,
            ".github/workflows/product-release-generator.yml",
        ),
    ] {
        if families.contains(&family) {
            paths.push(path.to_owned());
        }
    }
    if families.contains(&RoutingWorkflow::GeneratorRelease) {
        paths.extend(
            GENERATOR_ACTIONS
                .iter()
                .map(|action| format!(".github/actions/{action}/action.yml")),
        );
    }
    paths.sort();
    paths
}

fn assert_hosted_product_output(files: &BTreeMap<String, String>, families: &[RoutingWorkflow]) {
    let text = files
        .values()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("runs-on: ubuntu-26.04"), "{text}");
    if families.contains(&RoutingWorkflow::MacosBinaryRelease)
        || families.contains(&RoutingWorkflow::GeneratorRelease)
    {
        assert!(text.contains("runs-on: macos-15"), "{text}");
    }
    if families.contains(&RoutingWorkflow::GeneratorRelease) {
        assert!(text.contains("runs-on: macos-15-intel"), "{text}");
    }
    assert!(!text.contains("ubuntu-26.04-scale-set"), "{text}");
    assert!(!text.contains("runs-on: [velnor,"), "{text}");

    let coordinator = &files[crate::schema2::PRODUCT_RELEASE_WORKFLOW];
    for (family, publisher) in [
        (RoutingWorkflow::ImageRelease, "release-images:"),
        (RoutingWorkflow::MacosBinaryRelease, "release-binary:"),
        (RoutingWorkflow::GeneratorRelease, "release-generator:"),
    ] {
        assert_eq!(
            coordinator.matches(publisher).count(),
            usize::from(families.contains(&family)),
            "{coordinator}"
        );
    }
    for (path, publisher) in [
        (
            ".github/workflows/product-release-images.yml",
            "publish-images:",
        ),
        (
            ".github/workflows/product-release-binary.yml",
            "publish-binary:",
        ),
        (
            ".github/workflows/product-release-generator.yml",
            "publish-generator:",
        ),
    ] {
        if let Some(workflow) = files.get(path) {
            assert_eq!(workflow.matches(publisher).count(), 1, "{workflow}");
        }
    }
}

fn assert_mode_invariance(families: &[RoutingWorkflow]) -> Result<(), Box<dyn Error>> {
    let hosted = product_files(families, ExecutionMode::Hosted)?;
    assert_eq!(
        hosted.keys().cloned().collect::<Vec<_>>(),
        expected_product_paths(families)
    );
    assert_eq!(
        hosted,
        product_files(families, ExecutionMode::ScaleSet)?,
        "product documents must stay hosted in Scale Set mode"
    );
    assert_eq!(
        hosted,
        product_files(families, ExecutionMode::Both)?,
        "product documents must stay hosted in both mode"
    );
    assert_hosted_product_output(&hosted, families);
    Ok(())
}

#[test]
fn composed_product_documents_stay_hosted_in_every_routing_mode() -> Result<(), Box<dyn Error>> {
    let all_families = [
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ];
    assert_mode_invariance(&all_families)
}

#[test]
fn product_producer_marks_only_emitted_workflow_documents_hosted() -> Result<(), Box<dyn Error>> {
    let families = [
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ];
    let request = request(&families, ExecutionMode::ScaleSet)?;
    let (files, _, overrides) =
        render_schema2_workflows_with_verified_routing_and_overrides(&request)?;
    let actual = overrides
        .hosted_only_paths()
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>();
    let expected = [
        PRODUCT_RELEASE_WORKFLOW,
        ".github/workflows/product-release-images.yml",
        ".github/workflows/product-release-binary.yml",
        ".github/workflows/product-release-generator.yml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(actual, expected);
    assert!(
        files.iter().any(|file| {
            file.path == crate::schema2::schema2_job_image_candidate::WORKFLOW_PATH
        })
    );
    assert!(
        actual
            .iter()
            .all(|path| { files.iter().any(|file| file.path == *path) })
    );
    Ok(())
}

#[test]
fn isolated_product_families_keep_their_complete_hosted_paths() -> Result<(), Box<dyn Error>> {
    for family in [
        RoutingWorkflow::ImageRelease,
        RoutingWorkflow::MacosBinaryRelease,
        RoutingWorkflow::GeneratorRelease,
    ] {
        assert_mode_invariance(&[family])?;
    }
    Ok(())
}
