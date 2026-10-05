//! Exact schema-2 generator-release workflow body, without the generator marker.

use velnor_actions_workflow_renderer::RenderedTree;
#[path = "schema2_generator_release_action_snapshots.rs"]
mod action_snapshots;
#[path = "schema2_generator_release_qualification_snapshots.rs"]
mod qualification_snapshots;
#[path = "schema2_generator_release_security_snapshots.rs"]
mod security_snapshots;
use action_snapshots::{Actions, action, action_text};

pub(super) const GENERATOR_RELEASE: &str = include_str!("snapshots/generator-release.yml");

/// Byte-lock the rendered workflow and check release invariants.
pub(super) fn assert_rendered(tree: &RenderedTree) -> Result<(), Box<dyn std::error::Error>> {
    let body = tree
        .get(".github/workflows/generator-release.yml")
        .ok_or("missing .github/workflows/generator-release.yml")?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE));
    assert!(body.lines().count() < 400, "workflow has too many lines");
    let actions = action_snapshots::rendered_actions(tree)?;
    assert_generator(body, &actions)?;
    Ok(())
}

/// Committed `generator-release.yml` matches the rendered bytes.
pub(super) fn assert_committed(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join(".github/workflows/generator-release.yml");
    let body = std::fs::read_to_string(&path)?;
    assert_eq!(body, super::marked(GENERATOR_RELEASE), "{}", path.display());
    assert!(body.lines().count() < 400, "workflow has too many lines");
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
    assert_asset_catalog(&action_text(actions));
    assert_pinned_gh_invocations(&action_text(actions));
    assert_target_builds(body, actions)?;
    assert_manifest_job(body, actions)?;
    assert_publish_job(body, actions)?;
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
            "attest-manifest",
            "publish-generator",
        ]
    );
}

fn assert_global_policy(body: &str, actions: &Actions) {
    let combined = format!("{body}\n{}", action_text(actions));
    assert_eq!(body.matches("contents: write").count(), 1, "{body}");
    assert_eq!(body.matches("id-token: write").count(), 3, "{body}");
    assert!(combined.contains("v0.1.1"), "{combined}");
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
            assert!(
                job.contains("Fetch exact public source without an action post hook"),
                "{id}: {job}"
            );
        } else {
            assert_eq!(checkouts, 1, "{id}: {job}");
        }
        if id != "verify-release-source" {
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
    let source_gate = super::job_body(body, "verify-release-source")?;
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

fn assert_asset_catalog(body: &str) {
    assert!(body.contains("release-manifest.json"), "{body}");
    assert!(
        body.contains("cmp release-manifest.json manifest-assets/release-manifest.json"),
        "{body}"
    );
    assert!(body.contains("sha256sum --check"), "{body}");
    for binary in [
        "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
        "velnor-actions-0.1.1-aarch64-apple-darwin",
    ] {
        assert!(body.contains(binary), "missing {binary}");
    }
    for archive in ["generator-linux-assets.tar", "generator-macos-assets.tar"] {
        assert!(
            body.contains(archive),
            "missing candidate archive {archive}"
        );
    }
    for provenance in [
        "velnor-actions-0.1.1-x86_64-unknown-linux-gnu.provenance.json",
        "velnor-actions-0.1.1-aarch64-apple-darwin.provenance.json",
    ] {
        assert!(body.contains(provenance), "missing provenance {provenance}");
    }
    assert!(!body.contains("ubuntu-26.04-scale-set"), "{body}");
    assert!(!body.contains("runs-on: [velnor"), "{body}");
    assert!(!body.contains("binary-assets"), "{body}");
    assert!(!body.contains("image-assets"), "{body}");
}

fn assert_target_builds(body: &str, actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    assert_attest(body, actions, "attest-linux", "ubuntu-26.04")?;
    assert_attest(body, actions, "attest-macos", "macos-15")?;
    qualification_snapshots::assert_candidate_qualification(
        body,
        actions,
        "qualify-linux",
        "generator-release-qualify-linux",
        "build-linux",
        "linux-assets",
        "velnor-actions-0.1.1-x86_64-unknown-linux-gnu",
    )?;
    qualification_snapshots::assert_candidate_qualification(
        body,
        actions,
        "qualify-macos",
        "generator-release-qualify-macos",
        "build-macos",
        "macos-assets",
        "velnor-actions-0.1.1-aarch64-apple-darwin",
    )?;
    assert_job_action(body, "build-linux", "generator-release-build-linux")?;
    assert_job_action(body, "build-macos", "generator-release-build-macos")?;
    let linux = super::job_body(body, "build-linux")?;
    assert!(linux.contains("runs-on: ubuntu-26.04\n"), "{linux}");
    let linux_action = action(actions, "generator-release-build-linux")?;
    assert!(linux_action.contains("ELF"), "{linux_action}");
    assert!(linux_action.contains("sha256sum"), "{linux_action}");
    assert!(
        linux_action.contains(
            "mbx build --release --locked --package velnor-actions-cli --bin velnor-actions"
        ),
        "{linux_action}"
    );
    assert!(
        linux_action.contains("rust@1.98.1 mr-boxington@1.21.1"),
        "{linux_action}"
    );
    let macos = super::job_body(body, "build-macos")?;
    assert!(macos.contains("runs-on: macos-15\n"), "{macos}");
    let macos_action = action(actions, "generator-release-build-macos")?;
    assert!(macos_action.contains("*Mach-O*arm64*"), "{macos_action}");
    assert!(macos_action.contains("shasum -a 256"), "{macos_action}");
    Ok(())
}

fn assert_manifest_job(body: &str, actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    assert_attest(body, actions, "attest-manifest", "ubuntu-26.04")?;
    assert_job_action(body, "attest-manifest", "generator-release-manifest")?;
    let manifest = action(actions, "generator-release-manifest")?;
    assert!(manifest.contains("name: Setup Mise"), "{manifest}");
    assert!(
        manifest.contains("mise --no-config --no-env --no-hooks install gh@2.102.0"),
        "{manifest}"
    );
    assert!(
        manifest.contains("create-release-manifest.sh '0.1.1' 'tailrocks/velnor-new'"),
        "{manifest}"
    );
    assert!(manifest.contains("release-manifest.json"), "{manifest}");
    assert!(manifest.contains("Attest built artifacts"), "{manifest}");
    assert!(manifest.contains("Upload release manifest"), "{manifest}");
    let manifest_builder =
        include_str!("../../../scripts/generator-release/create-release-manifest.sh");
    assert!(
        manifest_builder.contains(".schema == 1"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".version == $version"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".commit == $commit"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(".toolchain.rust == $rust"),
        "{manifest_builder}"
    );
    assert!(
        manifest_builder.contains(r#".toolchain["mr-boxington"] == $mr_boxington"#),
        "{manifest_builder}"
    );
    let manifest_job = super::job_body(body, "attest-manifest")?;
    assert!(manifest_job.contains("- attest-linux"), "{manifest_job}");
    assert!(manifest_job.contains("- attest-macos"), "{manifest_job}");
    Ok(())
}

fn assert_pinned_gh_invocations(actions: &str) {
    const PREFIX: &str = "mise --no-config --no-env --no-hooks exec gh@2.102.0 -- ";
    for (offset, _) in actions.match_indices("gh ") {
        assert_eq!(
            actions.get(offset.saturating_sub(PREFIX.len())..offset),
            Some(PREFIX),
            "all generated release GitHub CLI commands must select the exact Mise pin"
        );
    }
}

fn assert_publish_job(body: &str, actions: &Actions) -> Result<(), Box<dyn std::error::Error>> {
    let publish = super::job_body(body, "publish-generator")?;
    assert!(publish.contains("runs-on: ubuntu-26.04\n"), "{publish}");
    assert!(publish.contains("contents: write"), "{publish}");
    assert!(!publish.contains("id-token:"), "{publish}");
    assert_job_action(body, "publish-generator", "generator-release-publish")?;
    let publish_action = action(actions, "generator-release-publish")?;
    assert!(
        publish_action.contains("GH_TOKEN: ${{ github.token }}"),
        "{publish_action}"
    );
    assert_eq!(
        publish_action
            .matches("name: Download verified release attestation bundles")
            .count(),
        3,
        "{publish_action}"
    );
    for artifact in [
        "generator-linux-assets-attestations",
        "generator-macos-assets-attestations",
        "generator-release-manifest-attestations",
    ] {
        assert!(
            publish_action.contains(&format!("name: {artifact}")),
            "missing downloaded bundle artifact {artifact}: {publish_action}"
        );
    }
    assert!(
        publish_action.contains("gh attestation verify"),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("--source-digest"),
        "{publish_action}"
    );
    assert!(
        publish_action.contains("--source-ref refs/heads/main"),
        "{publish_action}"
    );
    assert!(publish_action.contains(".intoto.jsonl"), "{publish_action}");
    assert!(publish_action.contains("v0.1.1"), "{publish_action}");
    assert!(publish.contains("- attest-linux"), "{publish}");
    assert!(publish.contains("- attest-macos"), "{publish}");
    assert!(publish.contains("- attest-manifest"), "{publish}");
    Ok(())
}

fn assert_job_action(
    workflow: &str,
    id: &str,
    action_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::job_body(workflow, id)?;
    assert!(
        job.contains(&format!("uses: ./.github/actions/{action_name}")),
        "{job}"
    );
    Ok(())
}

fn assert_attest(
    body: &str,
    actions: &Actions,
    id: &str,
    runs_on: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let job = super::job_body(body, id)?;
    assert!(job.contains(&format!("runs-on: {runs_on}\n")), "{job}");
    assert!(job.contains("id-token: write"), "{job}");
    assert!(!job.contains("contents: write"), "{job}");
    let action_name = if id == "attest-manifest" {
        "generator-release-manifest".to_owned()
    } else {
        format!("generator-release-{id}")
    };
    assert_job_action(body, id, &action_name)?;
    assert!(action(actions, &action_name)?.contains("provenance.json"));
    Ok(())
}
