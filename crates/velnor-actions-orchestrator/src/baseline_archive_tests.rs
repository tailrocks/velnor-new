//! Baseline ZIP byte and payload-shape regressions.

use super::*;
use std::io::Write as _;

pub(crate) fn metadata(id: u64, bytes: &[u8]) -> BaselineArtifactMetadata {
    BaselineArtifactMetadata {
        id,
        size_in_bytes: u64::try_from(bytes.len()).expect("archive size"),
        digest: format!(
            "sha256:{}",
            crate::cover_identity::generator::sha256_hex(bytes)
        ),
    }
}

fn eocd_offset(bytes: &[u8]) -> usize {
    (0..=bytes.len() - 22)
        .rev()
        .find(|offset| bytes.get(*offset..*offset + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .expect("EOCD")
}

fn central_offset(bytes: &[u8]) -> usize {
    usize::try_from(read_u32(bytes, eocd_offset(bytes) + 16)).expect("central offset")
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("u16"))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("u32"))
}

fn set_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn set_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn duplicate_manifest_archive(payload: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new_stream(Vec::new());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    writer
        .start_file("baseline.json", options)
        .expect("first entry");
    writer.write_all(payload).expect("first payload");
    writer
        .start_file("baseline.yaml", options)
        .expect("second entry");
    writer.write_all(payload).expect("second payload");
    let mut bytes = writer.finish().expect("finish ZIP").into_inner();
    let first = central_offset(&bytes);
    let first_name_len = usize::from(read_u16(&bytes, first + 28));
    let first_extra_len = usize::from(read_u16(&bytes, first + 30));
    let first_comment_len = usize::from(read_u16(&bytes, first + 32));
    let second = first + 46 + first_name_len + first_extra_len + first_comment_len;
    let local = usize::try_from(read_u32(&bytes, second + 42)).expect("local offset");
    let name = b"baseline.json";
    bytes[second + 46..second + 46 + name.len()].copy_from_slice(name);
    bytes[local + 30..local + 30 + name.len()].copy_from_slice(name);
    bytes
}

fn stored_archive(payload: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer
        .start_file(
            "baseline.json",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )
        .expect("start stored entry");
    writer.write_all(payload).expect("write stored payload");
    writer.finish().expect("finish ZIP").into_inner()
}

fn small_declared_size_archive(payload: &[u8]) -> Vec<u8> {
    let mut bytes = test_archive("baseline.json", payload);
    let central = central_offset(&bytes);
    let compressed = usize::try_from(read_u32(&bytes, central + 20)).expect("compressed size");
    let name_len = usize::from(read_u16(&bytes, 26));
    let extra_len = usize::from(read_u16(&bytes, 28));
    let data_end = 30 + name_len + extra_len + compressed;
    let descriptor_len = central - data_end;
    assert!(matches!(descriptor_len, 12 | 16));
    set_u32(&mut bytes, central + 24, 1);
    set_u32(&mut bytes, central - 4, 1);
    bytes
}

fn append_deflate_trailing_bytes(bytes: &mut Vec<u8>, has_descriptor: bool) {
    let central = central_offset(bytes);
    let compressed = usize::try_from(read_u32(bytes, central + 20)).expect("compressed size");
    let name_len = usize::from(read_u16(bytes, 26));
    let extra_len = usize::from(read_u16(bytes, 28));
    let data_start = 30 + name_len + extra_len;
    let insertion = data_start + compressed;
    let trailing = b"junk";
    bytes.splice(insertion..insertion, trailing.iter().copied());
    let new_size = u32::try_from(compressed + trailing.len()).expect("new size");
    let new_eocd = eocd_offset(bytes);
    set_u32(
        bytes,
        new_eocd + 16,
        u32::try_from(central + trailing.len()).expect("offset"),
    );
    let central = central_offset(bytes);
    set_u32(bytes, central + 20, new_size);
    if has_descriptor {
        let descriptor_field = match central - insertion - trailing.len() {
            12 => insertion + trailing.len() + 4,
            16 => insertion + trailing.len() + 8,
            _ => panic!("descriptor size"),
        };
        set_u32(bytes, descriptor_field, new_size);
    } else {
        set_u32(bytes, 18, new_size);
    }
}

#[test]
fn service_size_digest_and_single_manifest_are_bound() {
    let payload = br#"{"schema":2}"#;
    let archive = test_archive("baseline.json", payload);
    assert_eq!(
        extract_baseline(&metadata(99, &archive), &archive),
        Ok(payload.to_vec())
    );

    let mut wrong_digest = metadata(99, &archive);
    wrong_digest.digest = format!("sha256:{}", "0".repeat(64));
    assert!(extract_baseline(&wrong_digest, &archive).is_err());

    let mut wrong_size = metadata(99, &archive);
    wrong_size.size_in_bytes += 1;
    assert!(extract_baseline(&wrong_size, &archive).is_err());

    let mut wrong_id = metadata(0, &archive);
    wrong_id.id = 0;
    assert!(extract_baseline(&wrong_id, &archive).is_err());

    let oversized = vec![0; MAX_BASELINE_ARCHIVE_BYTES + 1];
    assert!(extract_baseline(&metadata(99, &oversized), &oversized).is_err());
}

#[test]
fn stored_manifest_remains_supported() {
    let payload = br#"{"schema":2}"#;
    let archive = stored_archive(payload);
    assert_eq!(
        extract_baseline(&metadata(99, &archive), &archive),
        Ok(payload.to_vec())
    );
}

#[test]
fn valid_streaming_data_descriptor_archive_is_accepted() {
    let payload = br#"{"schema":2}"#;
    let archive = test_archive("baseline.json", payload);
    let header = archive.get(..8).expect("local ZIP header");
    let flags = u16::from_le_bytes([header[6], header[7]]);
    assert_ne!(
        flags & (1 << 3),
        0,
        "fixture carries GPBF data-descriptor bit"
    );
    assert_eq!(
        extract_baseline(&metadata(99, &archive), &archive),
        Ok(payload.to_vec())
    );
}

#[test]
fn duplicate_central_baseline_names_are_rejected() {
    let archive = duplicate_manifest_archive(b"{}");
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn local_header_name_mismatch_is_rejected() {
    let mut archive = test_archive("baseline.json", b"{}");
    archive[30] = b'x';
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn dos_device_member_is_rejected() {
    let mut archive = test_archive("baseline.json", b"{}");
    let central = central_offset(&archive);
    set_u16(&mut archive, central + 4, 20);
    set_u32(&mut archive, central + 38, 0x40);
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn trailing_deflate_bytes_are_rejected_with_and_without_descriptor() {
    for has_descriptor in [false, true] {
        let mut archive = if has_descriptor {
            test_archive("baseline.json", b"{}")
        } else {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            writer
                .start_file(
                    "baseline.json",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .expect("start entry");
            writer.write_all(b"{}").expect("write manifest");
            writer.finish().expect("finish ZIP").into_inner()
        };
        assert_eq!(
            extract_baseline(&metadata(99, &archive), &archive),
            Ok(b"{}".to_vec()),
            "accept the clean archive before injecting compressed tail bytes"
        );
        append_deflate_trailing_bytes(&mut archive, has_descriptor);
        let shape = zip_admission::preflight_zip(&archive).expect("valid ZIP record layout");
        let data_start =
            30 + usize::from(read_u16(&archive, 26)) + usize::from(read_u16(&archive, 28));
        let data_end =
            data_start + usize::try_from(shape.compressed_size).expect("compressed size");
        assert!(
            !zip_admission::exact_deflate_consumption(
                &archive[data_start..data_end],
                shape.uncompressed_size,
            ),
            "reject unconsumed bytes inside declared compressed span"
        );
        assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
    }
}

#[test]
fn consistent_but_wrong_crc_is_rejected_while_reading_manifest() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "baseline.json",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .expect("start entry");
    writer.write_all(b"{}").expect("write manifest");
    let mut archive = writer.finish().expect("finish ZIP").into_inner();
    let central = central_offset(&archive);
    let wrong_crc = read_u32(&archive, central + 16) ^ 1;
    set_u32(&mut archive, 14, wrong_crc);
    set_u32(&mut archive, central + 16, wrong_crc);

    assert!(zip_admission::preflight_zip(&archive).is_ok());
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn malformed_data_descriptor_is_rejected_by_preflight() {
    let mut archive = test_archive("baseline.json", b"{}");
    let central = central_offset(&archive);
    let compressed = usize::try_from(read_u32(&archive, central + 20)).expect("compressed size");
    let data_start = 30 + usize::from(read_u16(&archive, 26)) + usize::from(read_u16(&archive, 28));
    let descriptor = data_start + compressed;
    assert_eq!(read_u32(&archive, descriptor), 0x0807_4b50);
    let wrong_crc = read_u32(&archive, descriptor + 4) ^ 1;
    set_u32(&mut archive, descriptor + 4, wrong_crc);

    assert!(matches!(
        zip_admission::preflight_zip(&archive),
        Err(reason) if reason == "baseline_archive_local_central_mismatch"
    ));
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn zip64_extra_field_is_rejected_by_preflight() {
    for local_extra in [false, true] {
        let mut archive = test_archive("baseline.json", b"{}");
        if local_extra {
            let extra = 30 + usize::from(read_u16(&archive, 26));
            archive.splice(extra..extra, [1, 0, 0, 0]);
            set_u16(&mut archive, 28, 4);
            let eocd = eocd_offset(&archive);
            let central = read_u32(&archive, eocd + 16);
            set_u32(&mut archive, eocd + 16, central + 4);
        } else {
            let central = central_offset(&archive);
            let extra = central + 46 + usize::from(read_u16(&archive, central + 28));
            let old_size = read_u32(&archive, eocd_offset(&archive) + 12);
            archive.splice(extra..extra, [1, 0, 0, 0]);
            set_u16(&mut archive, central + 30, 4);
            let eocd = eocd_offset(&archive);
            set_u32(&mut archive, eocd + 12, old_size + 4);
        }

        let expected_reason = if local_extra {
            "baseline_archive_local_central_mismatch"
        } else {
            "baseline_archive_shape"
        };
        assert!(matches!(
            zip_admission::preflight_zip(&archive),
            Err(reason) if reason == expected_reason
        ));
        assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
    }
}

#[test]
fn ambiguous_eocd_candidates_are_rejected_by_preflight() {
    let mut archive = test_archive("baseline.json", b"{}");
    let eocd = eocd_offset(&archive);
    set_u16(&mut archive, eocd + 20, 22);
    let mut embedded_eocd = [0; 22];
    embedded_eocd[..4].copy_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    archive.extend_from_slice(&embedded_eocd);

    assert!(matches!(
        zip_admission::preflight_zip(&archive),
        Err(reason) if reason == "baseline_archive_invalid"
    ));
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn forged_small_size_cannot_expand_past_manifest_limit() {
    let payload = vec![b'x'; MAX_BASELINE_JSON_BYTES + 8];
    let archive = small_declared_size_archive(&payload);
    assert!(
        archive.len() < MAX_BASELINE_ARCHIVE_BYTES,
        "highly compressible fixture stays below transport cap"
    );
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());
}

#[test]
fn decompressed_reader_stops_at_limit_plus_one() {
    struct CountingReader(usize);

    impl std::io::Read for CountingReader {
        fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
            output.fill(b'x');
            self.0 += output.len();
            Ok(output.len())
        }
    }

    let mut reader = CountingReader(0);
    assert!(read_manifest(&mut reader, 1).is_err());
    assert_eq!(reader.0, MAX_BASELINE_JSON_BYTES + 1);
}

#[test]
fn only_one_exact_regular_manifest_entry_is_accepted() {
    let payload = br#"{"schema":2}"#;
    for name in ["extra.json", "../baseline.json", "folder/baseline.json"] {
        let archive = test_archive(name, payload);
        assert!(
            extract_baseline(&metadata(99, &archive), &archive).is_err(),
            "{name}"
        );
    }

    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("baseline.json", options)
        .expect("start baseline");
    writer.write_all(payload).expect("write baseline");
    writer.start_file("extra", options).expect("start extra");
    writer.write_all(b"x").expect("write extra");
    let archive = writer.finish().expect("finish archive").into_inner();
    assert!(extract_baseline(&metadata(99, &archive), &archive).is_err());

    let mut symlink = test_archive("baseline.json", payload);
    let central = central_offset(&symlink);
    set_u16(&mut symlink, central + 4, (3 << 8) | 0x14);
    set_u32(&mut symlink, central + 38, 0o120_777 << 16);
    assert!(extract_baseline(&metadata(99, &symlink), &symlink).is_err());
}
