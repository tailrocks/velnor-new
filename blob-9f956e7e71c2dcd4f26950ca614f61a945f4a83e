use super::{manifest, test_pins};
use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn publisher_observes_one_release_id_before_and_after_publication() {
    let script = manifest::publish_script(&test_pins()).expect("pinned publisher script");
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
    let final_tag_check = script[second_ci..]
        .find("assert_release_tag_source")
        .map(|offset| second_ci + offset)
        .expect("publisher rechecks the source tag immediately before publication");
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
    assert!(second_ci < final_tag_check);
    assert!(final_tag_check < publish);
    assert!(publish < immutable_read);
    assert!(immutable_read < receipt);
    assert!(script.contains("repos/$GITHUB_REPOSITORY/releases/$release_id"));
    assert!(!script.contains("releases/tags/$tag"));
    assert!(script.contains("browser_download_url == $url"));
    assert!(script.contains("verify_release_assets \"$release_response\" 'false'"));
    assert!(script.contains("verify_release_assets \"$release_response\" 'true'"));
    assert!(script.contains(".target_commitish == $source"));
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

pub(super) fn assert_success_receipt(root: &Path) -> Result<(), Box<dyn Error>> {
    let manifest_name = manifest::acceptance_artifact_paths()[0];
    let receipt_name = manifest::acceptance_artifact_paths()[1];
    let source_manifest = root.join(manifest::candidate_path());
    let accepted_manifest = root.join(manifest_name);
    let receipt_path = root.join(receipt_name);
    let source_bytes = fs::read(source_manifest)?;
    let accepted_bytes = fs::read(&accepted_manifest)?;
    assert_eq!(source_bytes, accepted_bytes);

    let mut expected_assets = Vec::new();
    for path in manifest::release_asset_paths().split_whitespace() {
        let file = root.join(path);
        let name = file
            .file_name()
            .ok_or("release asset has no filename")?
            .to_str()
            .ok_or("release asset filename is not UTF-8")?;
        let digest = super::sha256(&file)?;
        let size = fs::metadata(&file)?.len();
        let url = format!(
            "https://github.com/{}/releases/download/v0.1.5/{name}",
            super::REPOSITORY
        );
        expected_assets.push(format!(
            "{{\"name\":\"{name}\",\"url\":\"{url}\",\"digest\":\"sha256:{digest}\",\"size\":{size}}}"
        ));
    }
    expected_assets.sort();
    let expected_assets = format!("[{}]", expected_assets.join(","));
    let manifest_digest = super::sha256(&accepted_manifest)?;
    let manifest_size = fs::metadata(&accepted_manifest)?.len().to_string();
    let program = ".schema == 1 and .repository == $repository and .version == $version and .source_commit == $source and .workflow_authority_sha == $workflow and .run_id == $run and .run_attempt == $attempt and .tag == $tag and .release_id == 123 and .immutable == true and .manifest.name == $manifest_name and .manifest.sha256 == $manifest_digest and .manifest.size == $manifest_size and (.assets | sort_by(.name)) == ($expected_assets | sort_by(.name))";
    let output = Command::new("jq")
        .args([
            "-e",
            "--arg",
            "repository",
            super::REPOSITORY,
            "--arg",
            "version",
            "0.1.5",
            "--arg",
            "source",
            super::SOURCE_SHA,
            "--arg",
            "workflow",
            super::SOURCE_SHA,
            "--arg",
            "run",
            "987654321",
            "--arg",
            "attempt",
            "2",
            "--arg",
            "tag",
            "v0.1.5",
            "--arg",
            "manifest_name",
            "release-manifest.json",
            "--arg",
            "manifest_digest",
            &manifest_digest,
            "--argjson",
            "manifest_size",
            &manifest_size,
            "--argjson",
            "expected_assets",
            &expected_assets,
            program,
        ])
        .arg(receipt_path)
        .output()?;
    assert!(
        output.status.success(),
        "acceptance receipt differs from exact API-observed release metadata: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
