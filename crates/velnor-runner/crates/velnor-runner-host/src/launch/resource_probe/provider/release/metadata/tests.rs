use serde_json::{Value, json};

use crate::launch::resource_probe::provider::archive::worker::MAX_WORKER_ARCHIVE_BYTES;

use super::{ASSETS, DIND_ARCHIVE, RUNNER_ARCHIVE, release_metadata};

const SOURCE_SHA: &str = "f1b044a606b6bc469f9718644f5c0ed88bf965b6";
const RELEASE_TAG: &str = "runner-f1b044a606b6bc469f9718644f5c0ed88bf965b6";

#[test]
fn rejects_worker_asset_size_above_download_bound() {
    for oversized in [RUNNER_ARCHIVE, DIND_ARCHIVE] {
        let metadata = release_metadata_fixture(oversized, MAX_WORKER_ARCHIVE_BYTES + 1);
        assert!(release_metadata(&metadata, RELEASE_TAG, SOURCE_SHA).is_err());
    }
}

#[test]
fn worker_asset_size_at_the_download_bound_is_accepted() {
    let metadata = release_metadata_fixture(RUNNER_ARCHIVE, MAX_WORKER_ARCHIVE_BYTES);
    assert!(release_metadata(&metadata, RELEASE_TAG, SOURCE_SHA).is_ok());
}

fn release_metadata_fixture(large_asset: &str, large_size: u64) -> Value {
    let assets = ASSETS
        .iter()
        .map(|name| {
            let size = if *name == large_asset { large_size } else { 1 };
            json!({
                "name": name,
                "digest": format!("sha256:{}", "0".repeat(64)),
                "size": size,
                "state": "uploaded"
            })
        })
        .collect::<Vec<_>>();
    json!({
        "tagName": RELEASE_TAG,
        "targetCommitish": SOURCE_SHA,
        "isDraft": false,
        "isImmutable": true,
        "isPrerelease": false,
        "assets": assets
    })
}
