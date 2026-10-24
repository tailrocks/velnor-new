use std::io::Write;

use flate2::{Compression, write::GzEncoder};
use tempfile::NamedTempFile;

use super::super::MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES;
use super::super::archive::Blob;
use super::{TAR_BLOCK, TAR_EXTENSION_LIMIT, inspect_layer, scan_extensions, validate_layer_path};

fn write_octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    let encoded = format!("{value:0width$o}");
    field[..width].copy_from_slice(encoded.as_bytes());
    field[width] = 0;
}

fn tar_header(path: &str, size: u64, kind: u8) -> [u8; TAR_BLOCK] {
    let mut header = [0_u8; TAR_BLOCK];
    header[..path.len()].copy_from_slice(path.as_bytes());
    write_octal(&mut header[100..108], 0o644);
    write_octal(&mut header[108..116], 0);
    write_octal(&mut header[116..124], 0);
    write_octal(&mut header[124..136], size);
    write_octal(&mut header[136..148], 0);
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let encoded = format!("{checksum:06o}");
    header[148..154].copy_from_slice(encoded.as_bytes());
    header[154] = 0;
    header[155] = b' ';
    header
}

fn tar_archive(entries: &[(&str, &[u8], u8)]) -> Vec<u8> {
    let mut archive = Vec::new();
    for (path, contents, kind) in entries {
        archive.extend_from_slice(&tar_header(path, contents.len() as u64, *kind));
        archive.extend_from_slice(contents);
        let padded = contents.len().div_ceil(TAR_BLOCK) * TAR_BLOCK;
        archive.resize(archive.len() + padded - contents.len(), 0);
    }
    archive.resize(archive.len() + 2 * TAR_BLOCK, 0);
    archive
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip input");
    encoder.finish().expect("gzip output")
}

fn store(bytes: &[u8]) -> (NamedTempFile, Blob) {
    let mut file = NamedTempFile::new().expect("temporary layer");
    file.write_all(bytes).expect("layer bytes");
    let blob = Blob {
        offset: 0,
        size: bytes.len() as u64,
    };
    (file, blob)
}

#[test]
fn extension_scan_fills_headers_across_members_before_admitting_extensions() {
    for kind in *b"xL" {
        let limit = usize::try_from(TAR_EXTENSION_LIMIT).expect("extension limit fits usize");
        let payload = vec![b'a'; limit + 1];
        let tar = tar_archive(&[("PaxHeaders/metadata", &payload, kind)]);
        let mut compressed = gzip(&tar[..1]);
        compressed.extend_from_slice(&gzip(&tar[1..]));
        let (mut file, blob) = store(&compressed);
        let result = scan_extensions(
            file.as_file_mut(),
            &blob,
            true,
            MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES,
        );
        assert_eq!(
            result
                .expect_err("oversized extension rejected")
                .to_string(),
            "oci_layer_extension_limit"
        );
    }
}

#[test]
fn extension_scan_rejects_truncated_headers_and_bodies() {
    let short_header = gzip(&tar_header("member", 0, b'0')[..TAR_BLOCK - 1]);
    let (mut file, blob) = store(&short_header);
    assert_eq!(
        scan_extensions(
            file.as_file_mut(),
            &blob,
            true,
            MAX_OCI_CANDIDATE_UNCOMPRESSED_BYTES,
        )
        .expect_err("partial header rejected")
        .to_string(),
        "oci_layer_extension_truncated_header"
    );

    let tar = tar_archive(&[("member", b"payload", b'0')]);
    let mut truncated = gzip(&tar);
    truncated.truncate(truncated.len() - 1);
    let (mut file, blob) = store(&truncated);
    assert!(inspect_layer(file.as_file_mut(), &blob, true, 0, 0).is_err());
}

#[test]
fn inspect_layer_rejects_duplicate_paths_after_normalization() {
    let tar = tar_archive(&[("nested/./item", b"a", b'0'), ("nested/item", b"b", b'0')]);
    let (mut file, blob) = store(&tar);
    assert_eq!(
        inspect_layer(file.as_file_mut(), &blob, false, 0, 0)
            .expect_err("normalized duplicate rejected")
            .to_string(),
        "oci_layer_duplicate_path"
    );
    assert_eq!(
        validate_layer_path("nested/./item").expect("normalized path"),
        validate_layer_path("nested/item").expect("canonical path")
    );
}

#[test]
fn raw_and_gzip_layers_have_matching_diff_ids_and_inventory() {
    let tar = tar_archive(&[("bin/tool", b"binary", b'0')]);
    let compressed = gzip(&tar);
    let (mut raw_file, raw_blob) = store(&tar);
    let (mut gzip_file, gzip_blob) = store(&compressed);
    let raw = inspect_layer(raw_file.as_file_mut(), &raw_blob, false, 0, 0).expect("raw layer");
    let zipped =
        inspect_layer(gzip_file.as_file_mut(), &gzip_blob, true, 0, 0).expect("gzip layer");
    assert_eq!(raw, zipped);
    assert_eq!(raw.1, tar.len() as u64);
    assert_eq!(raw.2, 1);
}
