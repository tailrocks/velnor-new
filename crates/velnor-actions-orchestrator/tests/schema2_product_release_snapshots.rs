//! Exact schema-2 product-release workflow and action snapshots.

use std::path::Path;

use velnor_actions_workflow_renderer::RenderedTree;

#[path = "schema2_generator_release_action_snapshots.rs"]
mod action_snapshots;
#[path = "schema2_generator_release_cross_snapshots.rs"]
mod cross_snapshots;
#[path = "schema2_generator_release_publication_snapshots.rs"]
mod publication_snapshots;
#[path = "schema2_generator_release_qualification_snapshots.rs"]
mod qualification_snapshots;
#[path = "schema2_generator_release_security_snapshots.rs"]
mod security_snapshots;

use action_snapshots::{Actions, action_text};

pub(super) const PRODUCT_RELEASE: &str = include_str!("snapshots/product-release.yml");
const IMAGE_RELEASE: &str = include_str!("snapshots/product-release-images.yml");
const BINARY_RELEASE: &str = include_str!("snapshots/product-release-binary.yml");
const GENERATOR_RELEASE: &str = include_str!("snapshots/product-release-generator.yml");

/// Byte-lock the coordinator, three reusable modules, and their local actions.
pub(super) fn assert_rendered(tree: &RenderedTree) -> Result<(), Box<dyn std::error::Error>> {
    let paths = [
        (".github/workflows/product-release.yml", PRODUCT_RELEASE),
        (
            ".github/workflows/product-release-images.yml",
            IMAGE_RELEASE,
        ),
        (
            ".github/workflows/product-release-binary.yml",
            BINARY_RELEASE,
        ),
        (
            ".github/workflows/product-release-generator.yml",
            GENERATOR_RELEASE,
        ),
    ];
    let mut workflows = Vec::new();
    for (path, expected) in paths {
        let body = tree.get(path).ok_or_else(|| format!("missing {path}"))?;
        assert_eq!(body, super::marked(expected), "{path}");
        workflows.push(body);
    }
    let actions = action_snapshots::rendered_actions(tree)?;
    assert_product(&workflows, &actions)
}

/// The checked-in workflows and actions match schema-2 output.
pub(super) fn assert_committed(
    root: &Path,
    tree: &RenderedTree,
) -> Result<(), Box<dyn std::error::Error>> {
    let paths = [
        (".github/workflows/product-release.yml", PRODUCT_RELEASE),
        (
            ".github/workflows/product-release-images.yml",
            IMAGE_RELEASE,
        ),
        (
            ".github/workflows/product-release-binary.yml",
            BINARY_RELEASE,
        ),
        (
            ".github/workflows/product-release-generator.yml",
            GENERATOR_RELEASE,
        ),
    ];
    let mut workflows = Vec::new();
    for (relative, _) in paths {
        let path = root.join(relative);
        let body = std::fs::read_to_string(&path)?;
        let expected = tree
            .get(relative)
            .ok_or_else(|| format!("missing generated workflow: {relative}"))?;
        assert_eq!(&body, expected, "{}", path.display());
        workflows.push(body);
    }
    let workflow_refs = workflows.iter().map(String::as_str).collect::<Vec<_>>();
    let actions = action_snapshots::committed_actions_matching_tree(root, tree)?;
    assert_product(&workflow_refs, &actions)
}

fn assert_product(workflows: &[&str], actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    let [parent, images, binary, generator] = workflows else {
        return Err("expected coordinator and three family workflows".into());
    };
    assert!(
        parent.contains("name: Velnor product releases\n"),
        "{parent}"
    );
    assert!(parent.contains("workflow_dispatch: {}"), "{parent}");
    assert!(!parent.contains("schedule:"), "{parent}");
    assert!(!parent.contains("push:"), "{parent}");
    assert!(!parent.contains("inputs:"), "{parent}");
    assert_parent_job_order(parent);
    assert_source_gate(parent)?;
    for (path, module) in [
        ("images", *images),
        ("binary", *binary),
        ("generator", *generator),
    ] {
        assert!(module.contains("workflow_call:"), "{path}: {module}");
        assert!(!module.contains("workflow_dispatch:"), "{path}: {module}");
        assert!(!module.contains("schedule:"), "{path}: {module}");
        assert!(
            module.contains("product-release.yml@refs/heads/main"),
            "{path} does not validate its caller: {module}"
        );
    }
    assert_family_calls(parent)?;

    let all_workflows = workflows.join("\n");
    let all = format!("{all_workflows}\n{}", action_text(actions));
    assert!(all.contains("v0.1.2"), "{all}");
    assert!(!all.contains("velnor-actions-0.1.0"), "{all}");
    assert!(!all.contains("generator-${GITHUB_SHA}"), "{all}");
    assert_permissions(images, binary, generator)?;
    assert_checkouts(parent, generator, actions)?;
    security_snapshots::assert_isolated_candidate_execution(&all_workflows, actions)?;
    publication_snapshots::assert_asset_catalog(&action_text(actions));
    publication_snapshots::assert_pinned_gh_policy(actions);
    cross_snapshots::assert_portable_gh_watchdog(actions);
    publication_snapshots::assert_target_builds(generator, actions)?;
    cross_snapshots::assert_qualify_install_tools(actions)?;
    publication_snapshots::assert_manifest_job(generator, actions)?;
    publication_snapshots::assert_publish_job(generator, actions)?;
    Ok(())
}

fn assert_parent_job_order(parent: &str) {
    assert_eq!(
        super::job_ids(parent),
        vec![
            "release-eligibility",
            "prepare-images",
            "release-images",
            "prepare-binary",
            "release-binary",
            "prepare-generator",
            "release-generator",
        ]
    );
}

fn assert_source_gate(parent: &str) -> Result<(), Box<dyn std::error::Error>> {
    let gate = super::job_body(parent, "release-eligibility")?;
    for required in [
        "workflow_authority_sha",
        "ci_run_id",
        "ci_attempt",
        "scripts/check-freshness.sh",
        "GITHUB_WORKFLOW_SHA",
        "actions/workflows/ci.yml/runs",
        "Required",
        "actionlint",
        "zizmor",
    ] {
        assert!(gate.contains(required), "missing {required}: {gate}");
    }
    assert!(gate.contains("GH_TOKEN: ${{ github.token }}"), "{gate}");
    assert!(gate.contains("contents: read"), "{gate}");
    assert!(gate.contains("actions: read"), "{gate}");
    Ok(())
}

fn assert_family_calls(parent: &str) -> Result<(), Box<dyn std::error::Error>> {
    for (id, prepare, path) in [
        (
            "release-images",
            "prepare-images",
            "./.github/workflows/product-release-images.yml # zizmor: ignore[self-repository]",
        ),
        (
            "release-binary",
            "prepare-binary",
            "./.github/workflows/product-release-binary.yml # zizmor: ignore[self-repository]",
        ),
        (
            "release-generator",
            "prepare-generator",
            "./.github/workflows/product-release-generator.yml # zizmor: ignore[self-repository]",
        ),
    ] {
        let job = super::job_body(parent, id)?;
        assert!(job.contains(&format!("uses: {path}")), "{job}");
        assert!(job.contains("actions: write"), "{job}");
        assert!(job.contains("artifact-metadata: write"), "{job}");
        assert!(job.contains("attestations: write"), "{job}");
        assert!(job.contains("contents: write"), "{job}");
        assert!(job.contains("id-token: write"), "{job}");
        assert!(
            job.contains(&format!(
                "release_action: ${{{{ needs.{prepare}.outputs.action }}}}"
            )),
            "{job}"
        );
    }
    assert!(parent.contains("cancel-in-progress: false"), "{parent}");
    Ok(())
}

fn assert_permissions(
    images: &str,
    binary: &str,
    generator: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    for (body, publisher, attesters) in [
        (images, "publish-images", &["attest-images"][..]),
        (binary, "publish-binary", &["attest-binary"][..]),
        (
            generator,
            "publish-generator",
            &[
                "attest-linux",
                "attest-macos",
                "attest-macos-intel",
                "attest-manifest",
            ][..],
        ),
    ] {
        let publish = super::job_body(body, publisher)?;
        assert!(
            publish.contains("contents: write"),
            "{publisher}: {publish}"
        );
        assert!(
            !publish.contains("id-token: write"),
            "{publisher}: {publish}"
        );
        for attester in attesters {
            let job = super::job_body(body, attester)?;
            assert!(job.contains("id-token: write"), "{attester}: {job}");
            assert!(!job.contains("contents: write"), "{attester}: {job}");
        }
    }
    let publish = super::job_body(generator, "publish-generator")?;
    assert!(
        publish.contains("environment:\n      name: generator-release"),
        "{publish}"
    );
    Ok(())
}

fn assert_checkouts(
    parent: &str,
    generator: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let prepare = super::job_body(parent, "prepare-generator")?;
    assert_eq!(prepare.matches("actions/checkout@").count(), 1, "{prepare}");
    assert!(
        prepare.contains("ref: ${{ needs.release-eligibility.outputs.source_sha }}"),
        "{prepare}"
    );
    for id in [
        "build-linux",
        "attest-linux",
        "attest-macos",
        "attest-macos-intel",
        "build-macos",
        "build-macos-intel",
        "candidate-manifest",
        "attest-manifest",
        "publish-generator",
    ] {
        let job = super::job_body(generator, id)?;
        assert_eq!(job.matches("actions/checkout@").count(), 1, "{id}: {job}");
        assert!(job.contains("ref: ${{ inputs.source_sha }}"), "{id}: {job}");
    }
    for id in ["qualify-linux", "qualify-macos", "qualify-macos-intel"] {
        let job = super::job_body(generator, id)?;
        assert!(!job.contains("actions/checkout@"), "{id}: {job}");
        let first_step = job
            .split("- name:")
            .nth(1)
            .ok_or("missing first qualifier step")?;
        assert!(first_step.contains("run:"), "{id}: {first_step}");
        assert!(
            !first_step.contains("uses: ./.github/actions/"),
            "{id}: {first_step}"
        );
    }
    for (name, action) in actions {
        assert!(!action.contains("actions/checkout@"), "{name}: {action}");
    }
    Ok(())
}
