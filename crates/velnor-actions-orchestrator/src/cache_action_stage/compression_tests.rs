use super::{ArchiveCompression, decode_archive};
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::{Read, Seek, SeekFrom, Write};

fn gzip(input: &[u8]) -> std::fs::File {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input).expect("write gzip input");
    let bytes = encoder.finish().expect("finish gzip");
    let mut file = tempfile::tempfile().expect("temporary gzip");
    file.write_all(&bytes).expect("write compressed data");
    file.rewind().expect("rewind compressed data");
    file
}

#[test]
fn gzip_decoder_rejects_bytes_after_the_last_member() {
    let mut file = gzip(b"valid tar bytes");
    file.seek(SeekFrom::End(0)).expect("seek after gzip member");
    file.write_all(b"unparsed trailing bytes")
        .expect("append trailing bytes");
    file.rewind().expect("rewind compressed data");
    assert!(decode_archive(file, ArchiveCompression::Gzip, None).is_err());
}

#[test]
fn gzip_decoder_returns_the_complete_decompressed_stream() {
    let file = gzip(b"valid tar bytes");
    let mut decoded =
        decode_archive(file, ArchiveCompression::Gzip, None).expect("decode complete gzip");
    let mut bytes = Vec::new();
    decoded
        .read_to_end(&mut bytes)
        .expect("read decompressed data");
    assert_eq!(bytes, b"valid tar bytes");
}
