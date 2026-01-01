//! Fixture-only source identities exercise admission; no release claims.

use super::{
    OwnedBuildBootstrap, OwnedPublicationSpec, SourceBuildBinding, SourceQualificationTrigger,
    render_owned_publication_files,
};

fn source() -> SourceBuildBinding {
    SourceBuildBinding {
        label: "mise".to_owned(),
        source_json: "{\"fixture\":true}".to_owned(),
    }
}

fn spec() -> OwnedPublicationSpec {
    let bootstrap = |target: &str, runner: &str| OwnedBuildBootstrap {
        target: target.to_owned(),
        runner: runner.to_owned(),
        assets_json: "{\"fixture\":true}".to_owned(),
    };
    OwnedPublicationSpec {
        trigger: SourceQualificationTrigger::DefaultBranchDispatch,
        generator_version: "0.1.0".to_owned(),
        checkout_uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
        sources: vec![source()],
        bootstraps: vec![
            bootstrap("x86_64-unknown-linux-gnu", "ubuntu-26.04"),
            bootstrap("aarch64-unknown-linux-gnu", "ubuntu-24.04-arm"),
            bootstrap("aarch64-apple-darwin", "macos-26"),
        ],
    }
}

#[test]
fn emits_only_complete_native_source_candidates_without_publication_authority() {
    let files = render_owned_publication_files(&spec()).expect("valid fixture");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, ".github/workflows/owned-tools.yml");
    let text = &files[0].bytes;
    assert!(text.contains("workflow_dispatch:"));
    assert!(!text.contains("pull_request:") && !text.contains("push:"));
    assert!(text.contains("cancel-in-progress: false"));
    assert!(text.contains("contents: read"));
    assert!(!text.contains("contents: write") && !text.contains("id-token"));
    assert!(!text.contains("attestations") && !text.contains("secrets."));
    assert!(!text.contains("release upload") && !text.contains("clobber"));
    assert!(text.contains("persist-credentials: false"));
    assert!(text.contains("github.run_id") && text.contains("github.run_attempt"));
    assert!(text.contains("aarch64-unknown-linux-gnu") && text.contains("macos-26"));
    assert!(text.contains("overwrite: false"));
    assert!(text.contains("scripts/qualify-owned-tool.py"));
    assert!(text.contains("scripts/download-owned-tool-candidate.py"));
    assert!(text.contains("needs: build"));
    assert!(text.contains("actions: read"));
    assert!(text.contains("qualified-receipt-$OWNED_TOOL_TARGET.json"));
    assert!(text.contains("native-report-$OWNED_TOOL_TARGET.json"));
    assert!(text.contains("always()"));
    assert!(!text.contains("inputs."));
}

#[test]
fn refuses_missing_duplicate_and_non_native_hosts_or_unapproved_sources() {
    let mut incomplete = spec();
    incomplete.bootstraps.pop();
    assert!(render_owned_publication_files(&incomplete).is_err());
    let mut duplicate = spec();
    duplicate.bootstraps[1] = duplicate.bootstraps[0].clone();
    assert!(render_owned_publication_files(&duplicate).is_err());
    let mut non_native = spec();
    non_native.bootstraps[1].runner = "ubuntu-26.04".to_owned();
    assert!(render_owned_publication_files(&non_native).is_err());
    let mut no_source = spec();
    no_source.sources.clear();
    assert!(render_owned_publication_files(&no_source).is_err());
    let mut duplicate_source = spec();
    duplicate_source.sources.push(source());
    assert!(render_owned_publication_files(&duplicate_source).is_err());
}

#[test]
fn reviewed_push_is_exact_read_only_infrastructure_category() {
    let mut spec = spec();
    spec.trigger = SourceQualificationTrigger::ReviewedInfrastructurePush;
    let text = &render_owned_publication_files(&spec).expect("reviewed fixture")[0].bytes;
    assert!(text.contains("push:") && text.contains("owned-tool-candidates"));
    assert!(!text.contains("workflow_dispatch:") && !text.contains("pull_request:"));
    assert!(!text.contains("tags:") && !text.contains("workflow_run:"));
    assert!(text.contains("github.event_name == 'push'"));
    assert!(text.contains("github.ref == 'refs/heads/owned-tool-candidates'"));
    assert!(text.contains("github.workflow_sha == github.sha"));
    assert!(text.contains("github.workflow_ref == format("));
    assert!(!text.contains("contents: write") && !text.contains("id-token"));
    assert!(!text.contains("attestations") && !text.contains("publish-owned-tool-artifacts"));
}
