//! TAR framing-changing metadata is rejected before `tar::Archive` can consume it.

use super::*;

use std::fs;
use std::io::Write;
use std::path::Path;

#[test]
fn pax_size_override_is_rejected_before_parser_uses_a_different_boundary() {
    let temp = scratch("pax-size-override");
    let root = scratch_root(&temp);
    let archive = root.join("size-override.tar.gz");
    tar_gz_with_pax_record(&archive, "size", "1024");

    let destination = root.join("prefix");
    let error = extract(&archive, &destination, "https://example.test/tool.tar.gz")
        .expect_err("PAX size cannot change parser framing");
    assert_problem(&error, "tool_archive_pax_size_override");
    assert!(!destination.exists());
}

#[test]
fn pax_sparse_metadata_is_rejected_before_tar_parser_admission() {
    let temp = scratch("pax-sparse");
    let root = scratch_root(&temp);
    let archive = root.join("sparse.tar.gz");
    tar_gz_with_pax_record(&archive, "GNU.sparse.map", "0,1");

    let destination = root.join("prefix");
    let error = extract(&archive, &destination, "https://example.test/tool.tar.gz")
        .expect_err("PAX sparse metadata is unsupported");
    assert_problem(&error, "tool_archive_sparse_unsupported");
    assert!(!destination.exists());
}

#[test]
fn gnu_sparse_entry_is_rejected_before_tar_parser_reads_sparse_extensions() {
    let temp = scratch("gnu-sparse");
    let root = scratch_root(&temp);
    let archive = root.join("sparse.tar.gz");
    let file = fs::File::create(&archive).expect("archive");
    let mut encoder = GzEncoder::new(file, Compression::default());
    let mut header = Header::new_gnu();
    header.set_path("sparse").expect("path");
    header.set_entry_type(EntryType::GNUSparse);
    header.set_size(0);
    header.set_mode(0o644);
    header.set_cksum();
    encoder.write_all(header.as_bytes()).expect("header");
    encoder.write_all(&[0; 1024]).expect("end blocks");
    encoder.finish().expect("gzip finish");

    let destination = root.join("prefix");
    let error = extract(&archive, &destination, "https://example.test/tool.tar.gz")
        .expect_err("GNU sparse entries are unsupported");
    assert_problem(&error, "tool_archive_sparse_unsupported");
    assert!(!destination.exists());
}

fn tar_gz_with_pax_record(path: &Path, key: &str, value: &str) {
    let payload = pax_record(key, value);
    let file = fs::File::create(path).expect("archive");
    let mut encoder = GzEncoder::new(file, Compression::default());
    let mut extension = Header::new_gnu();
    extension.set_path("PaxHeaders/metadata").expect("PAX path");
    extension.set_entry_type(EntryType::XHeader);
    extension.set_size(payload.len() as u64);
    extension.set_mode(0o644);
    extension.set_cksum();
    encoder.write_all(extension.as_bytes()).expect("PAX header");
    encoder.write_all(&payload).expect("PAX metadata");
    write_padding(&mut encoder, payload.len());

    let mut regular = Header::new_gnu();
    regular.set_path("member").expect("regular path");
    regular.set_entry_type(EntryType::Regular);
    regular.set_size(1);
    regular.set_mode(0o644);
    regular.set_cksum();
    encoder
        .write_all(regular.as_bytes())
        .expect("regular header");
    encoder.write_all(b"x").expect("regular data");
    write_padding(&mut encoder, 1);
    encoder.write_all(&[0; 1024]).expect("end blocks");
    encoder.finish().expect("gzip finish");
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let fields = format!("{key}={value}\n");
    let mut length = fields.len() + 2;
    loop {
        let next = length.to_string().len() + 1 + fields.len();
        if next == length {
            break;
        }
        length = next;
    }
    format!("{length} {fields}").into_bytes()
}

fn write_padding(writer: &mut impl Write, size: usize) {
    let padding = (512 - size % 512) % 512;
    writer.write_all(&vec![0; padding]).expect("tar padding");
}

fn assert_problem(error: &OrchestratorError, expected: &str) {
    assert!(
        matches!(error, OrchestratorError::Internal { problem } if problem == expected),
        "unexpected archive error: {error}"
    );
}
