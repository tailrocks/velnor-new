// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;

use super::fixture::{
    LAYER, LAYER_GZ, build, check, digest, gzip, member, plain_layer, refused, spec, tar,
};

#[test]
fn a_valid_raw_and_gzip_candidate_is_accepted_with_its_identities() {
    let layer = plain_layer();
    let image = build(&spec(vec![(layer.clone(), LAYER)]));
    let graph = check(&image.archive()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(graph["layer_digests"], json!([digest(&layer)]));
    let compressed = gzip(&layer);
    let image = build(&spec(vec![
        (compressed.clone(), LAYER_GZ),
        (layer.clone(), LAYER),
    ]));
    let graph = check(&image.archive()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        graph["layer_digests"],
        json!([digest(&compressed), digest(&layer)])
    );
}

#[test]
fn the_same_layer_used_twice_is_accepted() {
    let layer = plain_layer();
    let image = build(&spec(vec![(layer.clone(), LAYER), (layer, LAYER)]));
    assert!(check(&image.archive()).is_ok());
}

#[test]
fn a_blob_with_the_right_size_but_other_bytes_is_refused() {
    let layer = plain_layer();
    let mut image = build(&spec(vec![(layer.clone(), LAYER)]));
    let mut forged = layer.clone();
    forged[600] ^= 1;
    image.replace(|data| data == layer.as_slice(), &forged);
    assert_eq!(
        refused(&image.archive()),
        "OCI layer DiffID is not the same as the digest of its uncompressed bytes."
    );
}

#[test]
fn a_compressed_layer_with_the_same_content_but_other_bytes_is_refused() {
    let layer = plain_layer();
    let declared = gzip(&layer);
    let mut image = build(&spec(vec![(declared.clone(), LAYER_GZ)]));
    let mut other = declared.clone();
    other[4] ^= 1;
    image.replace(|data| data == declared.as_slice(), &other);
    assert_eq!(
        refused(&image.archive()),
        "OCI blob bytes are not the same as the bytes identified by their descriptor."
    );
}

#[test]
fn a_manifest_whose_bytes_do_not_match_its_digest_is_refused() {
    let mut image = build(&spec(vec![(plain_layer(), LAYER)]));
    for (name, data) in &mut image.entries {
        if name.starts_with("blobs/") && data.starts_with(b"{\"config\"") {
            let last = data.len() - 2;
            data[last] = b'x';
        }
    }
    assert_eq!(
        refused(&image.archive()),
        "OCI manifest blob has an incorrect digest."
    );
}

#[test]
fn a_platform_other_than_linux_amd64_is_refused() {
    let mut candidate = spec(vec![(plain_layer(), LAYER)]);
    candidate.platform = json!({"architecture": "arm64", "os": "linux"});
    assert_eq!(
        refused(&build(&candidate).archive()),
        "OCI archive platform is not linux/amd64."
    );
}

#[test]
fn diff_ids_that_do_not_describe_the_layers_are_refused() {
    let mut candidate = spec(vec![(plain_layer(), LAYER)]);
    candidate.diff_ids = Some(vec![format!("sha256:{}", "0".repeat(64))]);
    assert_eq!(
        refused(&build(&candidate).archive()),
        "OCI layer DiffID is not the same as the digest of its uncompressed bytes."
    );
    candidate.diff_ids = Some(vec!["sha256:zz".to_owned()]);
    assert_eq!(
        refused(&build(&candidate).archive()),
        "OCI image configuration has incorrect DiffID data for its manifest layers."
    );
    candidate.diff_ids = Some(Vec::new());
    assert_eq!(
        refused(&build(&candidate).archive()),
        "OCI image configuration has incorrect DiffID data for its manifest layers."
    );
}

#[test]
fn an_unknown_layer_media_type_is_refused() {
    let image = build(&spec(vec![(plain_layer(), "application/x-other")]));
    assert_eq!(
        refused(&image.archive()),
        "OCI layer media type is incorrect."
    );
}

#[test]
fn an_unreferenced_or_missing_blob_is_refused() {
    let mut image = build(&spec(vec![(plain_layer(), LAYER)]));
    image
        .entries
        .push((format!("blobs/sha256/{}", "e".repeat(64)), b"x".to_vec()));
    assert_eq!(
        refused(&image.archive()),
        "OCI archive has missing or unreferenced blobs."
    );
    let mut image = build(&spec(vec![(plain_layer(), LAYER)]));
    image.entries.pop();
    assert_eq!(
        refused(&image.archive()),
        "OCI archive has missing or unreferenced blobs."
    );
}

#[test]
fn a_repeated_archive_path_is_refused() {
    let mut image = build(&spec(vec![(plain_layer(), LAYER)]));
    let first = image.entries[0].clone();
    image.entries.push(first);
    assert_eq!(
        refused(&image.archive()),
        "Duplicate archive path: oci-layout"
    );
}

#[test]
fn unsafe_unexpected_and_non_regular_archive_paths_are_refused() {
    let base = build(&spec(vec![(plain_layer(), LAYER)]));
    for (name, message) in [
        ("../x", "OCI archive contains a path that is not safe."),
        ("/x", "OCI archive contains a path that is not safe."),
        ("./x", "OCI archive contains a path that is not safe."),
        ("extra.txt", "Unexpected OCI archive path: extra.txt"),
    ] {
        let mut image = build(&spec(vec![(plain_layer(), LAYER)]));
        image.entries.insert(0, (name.to_owned(), b"x".to_vec()));
        assert_eq!(refused(&image.archive()), message, "{name}");
    }
    let mut archive: Vec<u8> = member("link", b"", b'2');
    archive.extend(base.archive());
    assert_eq!(
        refused(&archive),
        "OCI archive entry is not a regular file: link"
    );
}

#[test]
fn a_duplicate_json_key_or_a_constant_is_refused() {
    let mut image = build(&spec(vec![(plain_layer(), LAYER)]));
    image.entries[0].1 = br#"{"imageLayoutVersion":"1.0.0","imageLayoutVersion":"1.0.0"}"#.to_vec();
    assert_eq!(
        refused(&image.archive()),
        "Duplicate JSON key: imageLayoutVersion"
    );
    image.entries[0].1 = br#"{"imageLayoutVersion":NaN}"#.to_vec();
    assert_eq!(
        refused(&image.archive()),
        "JSON constant has an incorrect value: NaN."
    );
}

#[test]
fn a_compressed_or_truncated_or_empty_outer_archive_is_refused() {
    let archive = build(&spec(vec![(plain_layer(), LAYER)])).archive();
    assert_eq!(refused(&archive[..2000]), "Cannot read OCI archive.");
    assert_eq!(refused(&[]), "Cannot read OCI archive.");
    assert_eq!(
        refused(&tar(&[("oci-layout", b"{}")])),
        "OCI archive must contain oci-layout and index.json."
    );
}

#[test]
fn a_package_extension_header_over_the_limit_is_refused_before_parsing() {
    let mut archive = member("pax", &vec![b'a'; (1 << 20) + 1], b'x');
    archive.extend(build(&spec(vec![(plain_layer(), LAYER)])).archive());
    assert_eq!(
        refused(&archive),
        "Tar extension header exceeds its size limit."
    );
}
