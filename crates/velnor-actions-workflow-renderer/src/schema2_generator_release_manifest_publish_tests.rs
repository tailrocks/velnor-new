use super::{manifest, test_pins};

#[test]
fn publisher_observes_one_release_id_before_and_after_publication() {
    let script = manifest::publish_script(&test_pins());
    let create_tag = script
        .find("created_ref=")
        .expect("publisher creates the exact source tag");
    let create_draft = script
        .find("gh release create")
        .expect("publisher creates a draft release");
    let resolve_id = script
        .find("gh release view")
        .expect("publisher resolves the created release ID");
    let upload = script
        .find("gh release upload")
        .expect("publisher uploads the qualified inventory");
    let metadata_checks = script
        .match_indices("verify_release_metadata")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(metadata_checks.len(), 3);
    let draft_read = metadata_checks[1];
    let second_ci = script
        .rfind("actions/workflows/ci.yml/runs?")
        .expect("publisher rechecks Required immediately before publication");
    let publish = script
        .find("--method PATCH -F draft=false")
        .expect("publisher publishes by stable release ID");
    let immutable_read = metadata_checks[2];
    let receipt = script
        .find("release-accepted/release-acceptance.json")
        .expect("publisher emits an acceptance receipt only after immutable verification");

    assert!(create_tag < create_draft);
    assert!(create_draft < resolve_id);
    assert!(resolve_id < upload);
    assert!(upload < draft_read);
    assert!(draft_read < second_ci);
    assert!(second_ci < publish);
    assert!(publish < immutable_read);
    assert!(immutable_read < receipt);
    assert!(script.contains("repos/$GITHUB_REPOSITORY/releases/$release_id"));
    assert!(!script.contains("releases/tags/$tag"));
    assert!(script.contains("browser_download_url == $url"));
    assert!(script.contains("$matches[0].size == $size"));
    assert!(script.contains("$matches[0].digest == $digest"));
    assert!(script.contains(".immutable == true"));
}

#[test]
fn acceptance_inventory_uses_the_canonical_manifest_and_every_release_asset() {
    let paths = manifest::release_asset_paths();
    let assets = paths.split_whitespace().collect::<Vec<_>>();
    assert_eq!(assets.len(), 20);
    assert!(assets.contains(&"manifest-assets/release-manifest.json"));
    assert!(assets.contains(&"release-attestations/release-manifest.json.intoto.jsonl"));
    assert!(!paths.contains("velnor-actions-release-manifest.json"));
    assert_eq!(
        manifest::acceptance_artifact_paths(),
        [
            "release-accepted/release-manifest.json",
            "release-accepted/release-acceptance.json"
        ]
    );
    assert_eq!(
        manifest::acceptance_artifact_name(),
        "generator-release-accepted-${{ github.sha }}"
    );
}
