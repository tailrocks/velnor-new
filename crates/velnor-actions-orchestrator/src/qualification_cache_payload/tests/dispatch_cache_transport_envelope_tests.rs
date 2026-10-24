use super::super::transport_envelope::{
    HEADER_BYTES, MAGIC, RawTarRange, read_envelope, write_envelope,
};
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

const TEST_LIMIT: u64 = 20 * 1024 * 1024 * 1024;

fn raw_tar() -> Vec<u8> {
    vec![0; 1_024]
}

fn envelope(raw: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&u64::try_from(raw.len()).unwrap_or(u64::MAX).to_be_bytes());
    bytes.extend_from_slice(raw);
    bytes
}

#[test]
fn writer_backpatches_length_and_reader_limits_archive_range() {
    let expected_tar = raw_tar();
    let mut bytes = Cursor::new(Vec::new());
    write_envelope(&mut bytes, TEST_LIMIT, |writer| {
        writer
            .write_all(&expected_tar)
            .map_err(|_| "test_write_failed")
    })
    .expect("write valid envelope");

    let expected_range = RawTarRange {
        offset: HEADER_BYTES,
        length: u64::try_from(expected_tar.len()).unwrap_or(u64::MAX),
    };
    assert_eq!(
        read_envelope(&mut bytes, TEST_LIMIT).expect("read envelope"),
        expected_range
    );
    let mut actual_tar = Vec::new();
    bytes.read_to_end(&mut actual_tar).expect("read raw tar");
    assert_eq!(actual_tar, expected_tar);
}

#[test]
fn reader_rejects_bad_magic_and_short_header() {
    let mut bad_magic = Cursor::new(envelope(&raw_tar()));
    bad_magic.get_mut()[0] ^= 1;
    assert_eq!(
        read_envelope(&mut bad_magic, TEST_LIMIT),
        Err("qualification_cache_transport_invalid")
    );

    let mut short = Cursor::new(MAGIC.to_vec());
    assert_eq!(
        read_envelope(&mut short, TEST_LIMIT),
        Err("qualification_cache_transport_invalid")
    );
}

#[test]
fn reader_rejects_length_mismatch_and_trailing_data() {
    let raw = raw_tar();
    let mut truncated = envelope(&raw);
    truncated.pop();
    assert_eq!(
        read_envelope(&mut Cursor::new(truncated), TEST_LIMIT),
        Err("qualification_cache_transport_trailing_or_truncated")
    );

    let mut trailing = envelope(&raw);
    trailing.push(0);
    assert_eq!(
        read_envelope(&mut Cursor::new(trailing), TEST_LIMIT),
        Err("qualification_cache_transport_trailing_or_truncated")
    );
}

#[test]
fn reader_rejects_invalid_inner_tar_before_returning_its_range() {
    let mut raw = raw_tar();
    raw[0] = 1;
    assert_eq!(
        read_envelope(&mut Cursor::new(envelope(&raw)), TEST_LIMIT),
        Err("qualification_cache_transport_tar_invalid")
    );
}

#[test]
fn reader_rejects_unaligned_small_and_oversized_tar_lengths() {
    let mut unaligned = envelope(&raw_tar());
    let length_start = MAGIC.len();
    unaligned[length_start..length_start + 8].copy_from_slice(&1_025_u64.to_be_bytes());
    assert_eq!(
        read_envelope(&mut Cursor::new(unaligned), TEST_LIMIT),
        Err("qualification_cache_transport_length_invalid")
    );

    let mut too_small = envelope(&raw_tar());
    too_small[length_start..length_start + 8].copy_from_slice(&512_u64.to_be_bytes());
    assert_eq!(
        read_envelope(&mut Cursor::new(too_small), TEST_LIMIT),
        Err("qualification_cache_transport_length_invalid")
    );

    let mut oversized = Cursor::new(Vec::new());
    oversized.write_all(MAGIC).expect("write magic");
    oversized
        .write_all(&(TEST_LIMIT + 512).to_be_bytes())
        .expect("write length");
    oversized
        .seek(SeekFrom::Start(0))
        .expect("rewind oversized header");
    assert_eq!(
        read_envelope(&mut oversized, TEST_LIMIT),
        Err("qualification_cache_transport_length_invalid")
    );
}

#[test]
fn writer_rejects_empty_or_unaligned_tar_and_keeps_error_static() {
    let mut empty = Cursor::new(Vec::new());
    assert_eq!(
        write_envelope(&mut empty, TEST_LIMIT, |_| Ok(())),
        Err("qualification_cache_transport_length_invalid")
    );

    let mut unaligned = Cursor::new(Vec::new());
    assert_eq!(
        write_envelope(&mut unaligned, TEST_LIMIT, |writer| {
            writer
                .write_all(&[0; 1_025])
                .map_err(|_| "test_write_failed")
        }),
        Err("qualification_cache_transport_length_invalid")
    );
}

#[test]
fn readers_and_writers_require_an_explicit_bounded_operation_limit() {
    let mut reader = Cursor::new(envelope(&raw_tar()));
    assert_eq!(
        read_envelope(&mut reader, TEST_LIMIT + 1),
        Err("qualification_cache_transport_limit_invalid")
    );

    let mut writer = Cursor::new(Vec::new());
    assert_eq!(
        write_envelope(&mut writer, 0, |_| Ok(())),
        Err("qualification_cache_transport_limit_invalid")
    );

    let tight_limit = HEADER_BYTES + 1_024;
    let mut bounded_writer = Cursor::new(Vec::new());
    assert_eq!(
        write_envelope(&mut bounded_writer, tight_limit, |writer| {
            writer
                .write_all(&[0; 1_536])
                .map_err(|_| "test_write_failed")
        }),
        Err("qualification_cache_transport_limit_invalid")
    );
    assert!(u64::try_from(bounded_writer.get_ref().len()).expect("bounded length") <= tight_limit);
}
