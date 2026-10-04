//! Bounded, non-extracting ZIP admission regressions.

use std::io::Write as _;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::*;

fn archive(entries: &[(&str, &[u8])], method: CompressionMethod) -> Vec<u8> {
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(method);
    for (name, payload) in entries {
        writer.start_file(name, options).expect("start ZIP member");
        writer.write_all(payload).expect("write ZIP member");
    }
    writer.finish().expect("finish ZIP").into_inner()
}

fn descriptor_archive(payload: &[u8], signed: bool) -> (Vec<u8>, usize) {
    let mut bytes = archive(&[("baseline.json", payload)], CompressionMethod::Deflated);
    let original_central = central_offset(&bytes);
    let crc = u32::from_le_bytes(
        bytes[original_central + 16..original_central + 20]
            .try_into()
            .expect("central CRC"),
    );
    let compressed = u32::from_le_bytes(
        bytes[original_central + 20..original_central + 24]
            .try_into()
            .expect("central compressed size"),
    );
    let uncompressed = u32::from_le_bytes(
        bytes[original_central + 24..original_central + 28]
            .try_into()
            .expect("central uncompressed size"),
    );
    let mut descriptor = Vec::new();
    if signed {
        descriptor.extend_from_slice(&DATA_DESCRIPTOR_SIGNATURE.to_le_bytes());
    }
    descriptor.extend_from_slice(&crc.to_le_bytes());
    descriptor.extend_from_slice(&compressed.to_le_bytes());
    descriptor.extend_from_slice(&uncompressed.to_le_bytes());
    let descriptor_offset = original_central;
    bytes.splice(
        original_central..original_central,
        descriptor.iter().copied(),
    );

    set_u16(&mut bytes, 6, 8);
    set_u32(&mut bytes, 14, 0);
    set_u32(&mut bytes, 18, 0);
    set_u32(&mut bytes, 22, 0);
    let central = original_central + descriptor.len();
    set_u16(&mut bytes, central + 8, 8);
    let end = eocd_offset(&bytes);
    set_u32(
        &mut bytes,
        end + 16,
        u32::try_from(central).expect("central offset"),
    );
    (bytes, descriptor_offset)
}

fn central_offset(bytes: &[u8]) -> usize {
    let end = eocd_offset(bytes);
    usize::try_from(u32::from_le_bytes(
        bytes[end + 16..end + 20].try_into().expect("offset"),
    ))
    .expect("central offset")
}

fn eocd_offset(bytes: &[u8]) -> usize {
    (0..=bytes.len() - 22)
        .rev()
        .find(|offset| bytes.get(*offset..*offset + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .expect("EOCD")
}

fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn append_compressed_trailing_bytes(bytes: &mut Vec<u8>, descriptor_signature: Option<bool>) {
    let central = central_offset(bytes);
    let compressed_size = u32::from_le_bytes(
        bytes[central + 20..central + 24]
            .try_into()
            .expect("central compressed size"),
    );
    let name_len = usize::from(u16::from_le_bytes(
        bytes[26..28].try_into().expect("local name length"),
    ));
    let extra_len = usize::from(u16::from_le_bytes(
        bytes[28..30].try_into().expect("local extra length"),
    ));
    let data_start = LOCAL_BYTES + name_len + extra_len;
    let insertion = data_start + usize::try_from(compressed_size).expect("compressed size");
    let trailing = b"junk";
    bytes.splice(insertion..insertion, trailing.iter().copied());
    let compressed_size = compressed_size + u32::try_from(trailing.len()).expect("trailing size");
    let end = eocd_offset(bytes);
    set_u32(
        bytes,
        end + 16,
        u32::try_from(central + trailing.len()).expect("central offset"),
    );
    let central = central_offset(bytes);
    set_u32(bytes, central + 20, compressed_size);
    match descriptor_signature {
        Some(signed) => set_u32(
            bytes,
            insertion + trailing.len() + if signed { 8 } else { 4 },
            compressed_size,
        ),
        None => set_u32(bytes, 18, compressed_size),
    }
}

fn truncate_compressed_stream(bytes: &mut Vec<u8>) {
    let central = central_offset(bytes);
    let compressed_size = u32::from_le_bytes(
        bytes[central + 20..central + 24]
            .try_into()
            .expect("central compressed size"),
    );
    let name_len = usize::from(u16::from_le_bytes(
        bytes[26..28].try_into().expect("local name length"),
    ));
    let extra_len = usize::from(u16::from_le_bytes(
        bytes[28..30].try_into().expect("local extra length"),
    ));
    let data_start = LOCAL_BYTES + name_len + extra_len;
    let last_compressed_byte = data_start + usize::try_from(compressed_size).expect("size") - 1;
    bytes.remove(last_compressed_byte);
    let central = central - 1;
    let end = eocd_offset(bytes);
    set_u32(
        bytes,
        end + 16,
        u32::try_from(central).expect("central offset"),
    );
    let compressed_size = compressed_size - 1;
    set_u32(bytes, central + 20, compressed_size);
    set_u32(bytes, 18, compressed_size);
}

#[test]
fn accepts_one_bounded_stored_or_deflated_regular_manifest() {
    let payload = br#"{"schema":1}"#;
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        assert_eq!(
            baseline_payload(&archive(&[("baseline.json", payload)], method)),
            Ok(String::from_utf8(payload.to_vec()).expect("UTF-8"))
        );
    }
    assert_eq!(
        baseline_payload(&archive(
            &[("baseline.json", b"")],
            CompressionMethod::Deflated,
        )),
        Ok(String::new())
    );
}

#[test]
fn accepts_signed_and_unsigned_deflated_data_descriptors() {
    let payload = br#"{"schema":1}"#;
    for signed in [true, false] {
        let (bytes, _) = descriptor_archive(payload, signed);
        assert_eq!(
            baseline_payload(&bytes),
            Ok(String::from_utf8(payload.to_vec()).expect("UTF-8"))
        );
    }
}

#[test]
fn rejects_trailing_bytes_inside_deflated_data_with_and_without_descriptor() {
    let payload = br#"{"schema":1}"#;
    for descriptor_signature in [None, Some(false), Some(true)] {
        let mut bytes = match descriptor_signature {
            Some(signed) => descriptor_archive(payload, signed).0,
            None => archive(&[("baseline.json", payload)], CompressionMethod::Deflated),
        };
        append_compressed_trailing_bytes(&mut bytes, descriptor_signature);
        assert_eq!(
            baseline_payload(&bytes).expect_err("trailing compressed bytes"),
            "baseline_archive_crc_or_deflate",
            "descriptor signature={descriptor_signature:?}"
        );
    }
}

#[test]
fn rejects_truncated_deflate_stream() {
    let mut bytes = archive(&[("baseline.json", b"{}")], CompressionMethod::Deflated);
    truncate_compressed_stream(&mut bytes);
    assert_eq!(
        baseline_payload(&bytes).expect_err("truncated raw DEFLATE stream"),
        "baseline_archive_crc_or_deflate"
    );
}

#[test]
fn rejects_multiple_members_wrong_name_and_oversized_manifest() {
    let payload = br#"{"schema":1}"#;
    assert!(
        baseline_payload(&archive(
            &[("baseline.json", payload), ("extra.txt", b"x")],
            CompressionMethod::Stored,
        ))
        .is_err()
    );
    assert!(
        baseline_payload(&archive(
            &[("../baseline.json", payload)],
            CompressionMethod::Stored,
        ))
        .is_err()
    );

    let oversized = vec![b'x'; usize::try_from(MAX_MANIFEST_BYTES).expect("bound") + 1];
    let zipped = archive(
        &[("baseline.json", &oversized)],
        CompressionMethod::Deflated,
    );
    assert!(baseline_payload(&zipped).is_err());
}

#[test]
fn rejects_encryption_descriptors_with_populated_local_fields_zip64_and_multidisk() {
    let payload = b"{}";
    let central = central_offset(&archive(
        &[("baseline.json", payload)],
        CompressionMethod::Stored,
    ));
    let mut encrypted = archive(&[("baseline.json", payload)], CompressionMethod::Stored);
    set_u16(&mut encrypted, 6, 1);
    set_u16(&mut encrypted, central + 8, 1);
    assert!(baseline_payload(&encrypted).is_err());

    let mut descriptor = archive(&[("baseline.json", payload)], CompressionMethod::Stored);
    set_u16(&mut descriptor, 6, 8);
    set_u16(&mut descriptor, central + 8, 8);
    assert!(baseline_payload(&descriptor).is_err());

    let mut zip64 = archive(&[("baseline.json", payload)], CompressionMethod::Stored);
    set_u32(&mut zip64, central + 24, u32::MAX);
    assert!(baseline_payload(&zip64).is_err());

    let mut locator = archive(&[("baseline.json", payload)], CompressionMethod::Stored);
    let end = eocd_offset(&locator);
    set_u32(&mut locator, end - 20, 0x0706_4b50);
    assert!(baseline_payload(&locator).is_err());

    let mut multidisk = archive(&[("baseline.json", payload)], CompressionMethod::Stored);
    let end = eocd_offset(&multidisk);
    set_u16(&mut multidisk, end + 4, 1);
    assert!(baseline_payload(&multidisk).is_err());
}

#[test]
fn rejects_malformed_and_truncated_data_descriptors() {
    let (mut wrong_crc, descriptor) = descriptor_archive(b"{}", true);
    set_u32(&mut wrong_crc, descriptor + 4, u32::MAX);
    assert!(baseline_payload(&wrong_crc).is_err());

    let (mut wrong_local, _) = descriptor_archive(b"{}", true);
    set_u32(&mut wrong_local, 14, 1);
    assert!(baseline_payload(&wrong_local).is_err());

    let (mut truncated, descriptor) = descriptor_archive(b"{}", true);
    let old_central = central_offset(&truncated);
    truncated.remove(descriptor + 15);
    let end = eocd_offset(&truncated);
    set_u32(
        &mut truncated,
        end + 16,
        u32::try_from(old_central - 1).expect("central offset"),
    );
    assert!(baseline_payload(&truncated).is_err());
}

#[test]
fn rejects_symlink_and_local_central_disagreement() {
    let mut symlink = archive(&[("baseline.json", b"{}")], CompressionMethod::Stored);
    let central = central_offset(&symlink);
    set_u32(&mut symlink, central + 38, 0o120777 << 16);
    assert!(baseline_payload(&symlink).is_err());

    let mut mismatch = archive(&[("baseline.json", b"{}")], CompressionMethod::Stored);
    let central = central_offset(&mismatch);
    set_u32(&mut mismatch, 14, 0);
    assert!(baseline_payload(&mismatch).is_err());
    set_u32(&mut mismatch, 14, u32::MAX);
    set_u32(&mut mismatch, central + 16, u32::MAX);
    assert!(
        baseline_payload(&mismatch).is_err(),
        "CRC must be checked at EOF"
    );
}

#[test]
fn rejects_archive_bytes_over_the_transport_bound() {
    let oversized = vec![0; MAX_BASELINE_ARCHIVE_BYTES + 1];
    assert_eq!(
        baseline_payload(&oversized).expect_err("oversize ZIP"),
        "baseline_archive_shape"
    );
}
