//! Owned source admission fixtures; no publication claims.

use super::ApprovedOwnedSource;
use std::collections::BTreeMap;

fn source() -> ApprovedOwnedSource {
    let base = "https://github.com/tailrocks/velnor-new/releases/download/fixture-owned-source";
    ApprovedOwnedSource {
        tool: "mise".to_owned(),
        version: "2026.10.6-owned-cargo-wrapper".to_owned(),
        source_commit: "1".repeat(40),
        source_tree: "2".repeat(40),
        upstream_base_commit: "6be3cbdc639a66c03651479428e4c5f60b00485f".to_owned(),
        archive_url: format!("{base}/source.tar"),
        archive_sha256: "4".repeat(64),
        receipt_url: format!("{base}/source-receipt.json"),
        receipt_sha256: "5".repeat(64),
        patch_url: format!("{base}/base.patch"),
        patch_sha256: "6".repeat(64),
        lockfile_sha256: "7".repeat(64),
        license_files: BTreeMap::from([("LICENSE".to_owned(), "8".repeat(64))]),
    }
}

#[test]
fn admits_complete_owned_source_and_rejects_nonportable_license_aliases() {
    let mut value = source();
    assert!(value.validate().is_ok());
    value.version = "2026.10.6-velnor.1".to_owned();
    assert!(value.validate().is_ok());
    for path in [".Git/config", "LICENSE-λ", "dir//LICENSE", "dir/./LICENSE"] {
        let mut value = source();
        value.license_files.insert(path.to_owned(), "9".repeat(64));
        assert!(value.validate().is_err(), "{path}");
    }
}

#[test]
fn source_approval_rejects_dispatch_injection_and_partial_transport() {
    let mutations: [fn(&mut ApprovedOwnedSource); 7] = [
        |value: &mut ApprovedOwnedSource| value.archive_sha256 = "0".repeat(64),
        |value: &mut ApprovedOwnedSource| value.tool = "sh".to_owned(),
        |value: &mut ApprovedOwnedSource| value.version = "${{ inputs.version }}".to_owned(),
        |value: &mut ApprovedOwnedSource| value.source_commit = value.upstream_base_commit.clone(),
        |value: &mut ApprovedOwnedSource| value.patch_url.push_str("?redirect=1"),
        |value: &mut ApprovedOwnedSource| {
            value.archive_url = value
                .archive_url
                .replace("tailrocks/velnor-new", "other/source");
        },
        |value: &mut ApprovedOwnedSource| {
            value
                .license_files
                .insert("../outside".to_owned(), "f".repeat(64));
        },
    ];
    for mutate in mutations {
        let mut value = source();
        mutate(&mut value);
        assert!(value.validate().is_err());
    }
}
