// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_rejects_archive_identities(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    expect_archive_rejection(
        fixture,
        "tampered.tar",
        &tamper_layer_data(archive, parts),
        Some("OCI layer DiffID is not the same as the digest of its uncompressed bytes."),
    )?;
    let index_text = String::from_utf8(parts.index.clone())?;
    expect_archive_rejection(
        fixture,
        "platform.tar",
        &assemble_archive(
            &parts.layer,
            &parts.config,
            &parts.manifest,
            &replace_once(
                &index_text,
                "\"architecture\":\"amd64\"",
                "\"architecture\":\"arm64\"",
            )?
            .into_bytes(),
            &parts.layout,
        ),
        Some("OCI archive platform is not linux/amd64."),
    )?;
    let oversized_meta = vec![b' '; 1024 * 1024 + 1];
    expect_archive_rejection(
        fixture,
        "bigmeta.tar",
        &assemble_archive(
            &parts.layer,
            &parts.config,
            &parts.manifest,
            &oversized_meta,
            &parts.layout,
        ),
        Some("OCI metadata file is too large: index.json"),
    )?;
    expect_archive_rejection(
        fixture,
        "sizemismatch.tar",
        &assemble_archive(
            &parts.layer,
            &parts.config,
            &parts.manifest,
            &replace_once(
                &index_text,
                &format!("\"size\":{}", parts.manifest.len()),
                &format!("\"size\":{}", parts.manifest.len() + 1),
            )?
            .into_bytes(),
            &parts.layout,
        ),
        Some("OCI manifest size is not the same as its descriptor size."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_bad_layers(
    fixture: &Fixture,
    parts: &ImageParts,
) -> Result<(), Box<dyn Error>> {
    let evil_layer = tar_archive(&[("../evil", b"x".as_slice())]);
    let evil_config = config_for(&sha256_hex(&evil_layer));
    let evil_manifest = manifest_for(&evil_config, &evil_layer);
    let evil_index = index_for(&evil_manifest);
    expect_archive_rejection(
        fixture,
        "layertraversal.tar",
        &assemble_archive(
            &evil_layer,
            &evil_config,
            &evil_manifest,
            &evil_index,
            &parts.layout,
        ),
        Some("OCI layer has a path that is not safe."),
    )?;
    let dup_layer = tar_archive(&[("dup.txt", b"a".as_slice()), ("dup.txt", b"b".as_slice())]);
    let dup_config = config_for(&sha256_hex(&dup_layer));
    let dup_manifest = manifest_for(&dup_config, &dup_layer);
    let dup_index = index_for(&dup_manifest);
    expect_archive_rejection(
        fixture,
        "layerdup.tar",
        &assemble_archive(
            &dup_layer,
            &dup_config,
            &dup_manifest,
            &dup_index,
            &parts.layout,
        ),
        Some("OCI layer repeats a path: dup.txt"),
    )?;
    let manifest_text = String::from_utf8(parts.manifest.clone())?;
    let layer_entry = format!(
        "{{\"mediaType\":\"application/vnd.oci.image.layer.v1.tar\",\"digest\":\"sha256:{}\",\"size\":{}}}",
        sha256_hex(&parts.layer),
        parts.layer.len()
    );
    let empty_manifest = replace_once(&manifest_text, &layer_entry, "")?;
    let empty_index = index_for(empty_manifest.as_bytes());
    expect_archive_rejection(
        fixture,
        "emptylayers.tar",
        &assemble_archive(
            &parts.layer,
            &parts.config,
            empty_manifest.as_bytes(),
            &empty_index,
            &parts.layout,
        ),
        Some("OCI manifest must contain image layers."),
    )?;
    let extra_manifest = replace_once(&manifest_text, "\"layers\":[", "\"extra\":1,\"layers\":[")?;
    let extra_index = index_for(extra_manifest.as_bytes());
    expect_archive_rejection(
        fixture,
        "extrakey.tar",
        &assemble_archive(
            &parts.layer,
            &parts.config,
            extra_manifest.as_bytes(),
            &extra_index,
            &parts.layout,
        ),
        Some("OCI manifest has unknown fields."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_bad_publication_state(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    expect_archive_rejection(
        fixture,
        "unreferenced.tar",
        &splice_before_end(
            archive.to_vec(),
            &tar_entry(
                &format!("blobs/sha256/{}", "9".repeat(64)),
                b"unreferenced",
                b'0',
            ),
        ),
        Some("OCI archive has missing or unreferenced blobs."),
    )?;
    let wrong_diff = config_for(&"0".repeat(64));
    let wrong_manifest = manifest_for(&wrong_diff, &parts.layer);
    let wrong_index = index_for(&wrong_manifest);
    expect_archive_rejection(
        fixture,
        "diffid.tar",
        &assemble_archive(
            &parts.layer,
            &wrong_diff,
            &wrong_manifest,
            &wrong_index,
            &parts.layout,
        ),
        Some("OCI layer DiffID is not the same as the digest of its uncompressed bytes."),
    )?;
    let win_config = String::from_utf8(config_for(&sha256_hex(&parts.layer)))?
        .replacen(
            "\"architecture\":\"amd64\"",
            "\"architecture\":\"arm64\"",
            1,
        )
        .into_bytes();
    let win_manifest = manifest_for(&win_config, &parts.layer);
    let win_index = index_for(&win_manifest);
    expect_archive_rejection(
        fixture,
        "configplatform.tar",
        &assemble_archive(
            &parts.layer,
            &win_config,
            &win_manifest,
            &win_index,
            &parts.layout,
        ),
        Some("OCI image configuration platform is not linux/amd64."),
    )?;
    expect_archive_rejection(
        fixture,
        "paxoverflow.tar",
        &splice_before_end(
            archive.to_vec(),
            &tar_entry("pax-header", &vec![b' '; 2 * 1024 * 1024], b'x'),
        ),
        Some("Tar extension header exceeds its size limit."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_unsafe_paths(fixture: &Fixture) -> Result<(), Box<dyn Error>> {
    for (mode, name) in [
        ("tar_member", "../evil"),
        ("tar_member", "/abs"),
        ("tar_member", ""),
        ("tar_member", "."),
        ("tar_member", "a/../../b"),
        ("tar_layer", "../evil"),
        ("tar_layer", "/abs"),
    ] {
        let marker = if mode == "tar_member" {
            "TAR-MEMBER-REJECTED"
        } else {
            "TAR-LAYER-REJECTED"
        };
        let message = if mode == "tar_member" {
            "OCI archive contains a path that is not safe."
        } else {
            "OCI layer has a path that is not safe."
        };
        expect_tar_rejection(fixture, mode, name, marker, message)?;
    }
    Ok(())
}

#[test]
fn gc1_t45_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_sources_are_fixed()?;
    let (fixture, parts, archive) = fixture()?;
    let record = record_json(&parts, &archive);
    check_rejects_record_shapes(&fixture, &record)?;
    check_rejects_record_identities(&fixture, &record)?;
    check_rejects_record_digests(&fixture, &record, &parts, &archive)?;
    check_rejects_bad_archives(&fixture, &parts, &archive)?;
    check_rejects_archive_identities(&fixture, &parts, &archive)?;
    check_rejects_bad_layers(&fixture, &parts)?;
    check_rejects_bad_publication_state(&fixture, &parts, &archive)?;
    check_rejects_unsafe_paths(&fixture)?;
    Ok(())
}
