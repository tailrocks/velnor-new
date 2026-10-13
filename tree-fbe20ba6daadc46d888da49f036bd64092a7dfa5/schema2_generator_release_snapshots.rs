//! Exact schema-2 generator-release workflow body, without the generator marker.

use velnor_actions_workflow_renderer::RenderedTree;
#[path = "schema2_generator_release_action_snapshots.rs"]
mod action_snapshots;
#[path = "schema2_generator_release_qualification_snapshots.rs"]
mod qualification_snapshots;
#[path = "schema2_generator_release_security_snapshots.rs"]
mod security_snapshots;
use action_snapshots::{Actions, action_text};

pub(super) const GENERATOR_RELEASE: &str = include_str!("snapshots/generator-release.yml");

/// Byte-lock the rendered workflow and check release invariants.
pub(super) fn assert_rendered(tree: &RenderedTree) -> Result<(), Box<dyn std::error::Error>> {
    let body = tree
        .get(".github/workflows/generator-release.yml")
        .ok_or("missing .github/workflows/generator-release.yml")?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE));
    let actions = action_snapshots::rendered_actions(tree)?;
    assert_generator(body, &actions)?;
    Ok(())
}

/// Committed `generator-release.yml` matches the rendered bytes.
pub(super) fn assert_committed(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join(".github/workflows/generator-release.yml");
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE), "{}", path.display());
    let actions = action_snapshots::committed_actions(root)?;
    assert_generator(&body, &actions)?;
    Ok(())
}

fn assert_generator(body: &str, actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    assert!(body.contains("name: Generator release\n"), "{body}");
    assert_job_order(body);
    assert_global_policy(body, actions);
    assert_checkouts(body, actions)?;
    assert_source_gate(body)?;
    security_snapshots::assert_isolated_candidate_execution(body, actions)?;
    publication_snapshots::assert_asset_catalog(&action_text(actions));
    publication_snapshots::assert_pinned_gh_policy(actions);
    publication_snapshots::assert_target_builds(body, actions)?;
    publication_snapshots::assert_manifest_job(body, actions)?;
    publication_snapshots::assert_publish_job(body, actions)?;
    Ok(())
}

fn assert_job_order(body: &str) {
    assert_eq!(
        super::job_ids(body),
        vec![
            "verify-release-source",
            "build-linux",
            "qualify-linux",
            "attest-linux",
            "build-macos",
            "qualify-macos",
            "attest-macos",
            "build-macos-intel",
            "qualify-macos-intel",
            "attest-macos-intel",
            "candidate-manifest",
            "attest-manifest",
            "publish-generator",
        ]
    );
}

fn assert_global_policy(body: &str, actions: &Actions) {
    let combined = format!("{body}\n{}", action_text(actions));
    assert_eq!(body.matches("contents: write").count(), 1, "{body}");
    assert_eq!(body.matches("id-token: write").count(), 4, "{body}");
    assert!(combined.contains("v0.1.3"), "{combined}");
    assert!(!combined.contains("generator-${GITHUB_SHA}"), "{combined}");
    assert!(body.contains("workflow_dispatch: {}"), "{body}");
    assert!(!body.contains("inputs:"), "{body}");
}

fn assert_checkouts(body: &str, actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    for id in super::job_ids(body) {
        let job = super::job_body(body, id)?;
        let checkouts = job.matches("actions/checkout@").count();
        if id.starts_with("qualify-") {
            assert_eq!(checkouts, 0, "{id}: {job}");
        } else {
            assert_eq!(checkouts, 1, "{id}: {job}");
        }
        if !matches!(id, "verify-release-source" | "candidate-manifest") {
            assert_eq!(
                job.matches("uses: ./.github/actions/").count(),
                1,
                "{id}: {job}"
            );
        }
    }
    for (name, action) in actions {
        assert!(!action.contains("actions/checkout@"), "{name}: {action}");
    }
    Ok(())
}

fn assert_source_gate(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    assert!(body.contains("refs/heads/main"), "{body}");
    assert!(body.contains("actions/workflows/ci.yml/runs"), "{body}");
    assert!(body.contains("Required"), "{body}");
    assert!(body.contains("bash scripts/check-freshness.sh"), "{body}");
    assert!(
        !body.contains("check-freshness.sh --check-upstream"),
        "{body}"
    );
    assert!(body.contains("git rev-parse HEAD"), "{body}");
    assert!(body.contains("GITHUB_WORKFLOW_SHA"), "{body}");
    let source_gate = super::job_body(body, "verify-release-source")?;
    assert!(
        source_gate.contains(
            r#"gh() { timeout --signal=TERM --kill-after=5s 60s mise --no-config --no-env --no-hooks exec gh@2.102.0 -- gh \"$@\"; }\nexport -f gh"#
        ),
        "{source_gate}"
    );
    let recheck = source_gate
        .split("Recheck default-branch source")
        .nth(1)
        .ok_or("missing default-branch source recheck")?
        .split("- name:")
        .next()
        .ok_or("missing recheck step body")?;
    assert!(
        recheck.contains("GH_TOKEN: ${{ github.token }}"),
        "{recheck}"
    );
    Ok(())
}

#[path = "schema2_generator_release_publication_snapshots.rs"]
mod publication_snapshots;
