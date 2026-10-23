use super::*;
use std::{cell::Cell, io::Cursor, rc::Rc, time::Duration};
use zip::write::SimpleFileOptions;

struct Counted {
    input: Cursor<Vec<u8>>,
    bytes: Rc<Cell<usize>>,
    operations: Rc<Cell<usize>>,
}
impl Read for Counted {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        self.operations.set(self.operations.get() + 1);
        let n = self.input.read(output)?;
        self.bytes.set(self.bytes.get() + n);
        Ok(n)
    }
}
impl Seek for Counted {
    fn seek(&mut self, position: std::io::SeekFrom) -> std::io::Result<u64> {
        self.operations.set(self.operations.get() + 1);
        self.input.seek(position)
    }
}

fn streaming_zip(comment: usize) -> Vec<u8> {
    let mut state = 0x1234_5678_u32;
    let payload: Vec<u8> = (0..1024 * 1024)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect();
    let mut writer = zip::ZipWriter::new_stream(Vec::new());
    for (name, body) in [
        ("payload.tar.gz", payload.as_slice()),
        ("payload.sbom.json", b"{}".as_slice()),
        ("release-payload.json", b"native-proof".as_slice()),
    ] {
        writer
            .start_file(
                name,
                SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644),
            )
            .expect("file");
        writer.write_all(body).expect("body");
    }
    let mut bytes = writer.finish().expect("stream ZIP").into_inner();
    let length = u16::try_from(comment).expect("ZIP comment length");
    let end = bytes.len();
    bytes[end - 2..].copy_from_slice(&length.to_le_bytes());
    bytes.extend(std::iter::repeat_n(b'x', comment));
    bytes
}

#[test]
fn streaming_leaf_metadata_does_not_scan_incompressible_payload() {
    let bytes = streaming_zip(0);
    assert!(bytes.len() > 1024 * 1024, "ordinary incompressible DEFLATE");
    assert_eq!(u16::from_le_bytes([bytes[6], bytes[7]]) & 8, 8);
    let count = Rc::new(Cell::new(0));
    let input = Counted {
        input: Cursor::new(bytes),
        bytes: Rc::clone(&count),
        operations: Rc::new(Cell::new(0)),
    };
    let deadline = CheckDeadline::after(Duration::from_secs(5)).expect("deadline");
    deadline
        .scope(|| {
            let archive =
                reader::open(input, ZipProfile::PreviewLeaf).expect("native streaming leaf");
            assert_eq!(archive.len(), 3);
        })
        .expect("scope");
    assert!(
        count.get() < 4096,
        "metadata read {}, not payload",
        count.get()
    );
}

#[test]
fn maximum_zip_comment_fits_fixed_tail_budget_for_every_profile() {
    let bytes = streaming_zip(usize::from(u16::MAX));
    for profile in [
        ZipProfile::Recovery,
        ZipProfile::PreviewLeaf,
        ZipProfile::PreviewAggregate,
        ZipProfile::Evidence,
    ] {
        let deadline = CheckDeadline::after(Duration::from_secs(5)).expect("deadline");
        let manifest = deadline
            .scope(|| {
                read_manifest_reader(Cursor::new(&bytes), profile, "release-payload.json", 1024)
            })
            .expect("scope")
            .expect("bounded ZIP comment");
        assert_eq!(manifest, b"native-proof");
    }
}

fn manifest(bytes: &[u8], profile: ZipProfile) -> Result<Vec<u8>, OrchestratorError> {
    CheckDeadline::after(Duration::from_secs(5))
        .expect("deadline")
        .scope(|| read_manifest_reader(Cursor::new(bytes), profile, "release-payload.json", 1024))
        .expect("scope")
}

fn zip64_end_records(bytes: &[u8]) -> Vec<u8> {
    let end = bytes
        .windows(4)
        .rposition(|w| w == b"PK\x05\x06")
        .expect("EOCD");
    let tail = &bytes[end..];
    let count = u64::from(u16::from_le_bytes(tail[10..12].try_into().expect("count")));
    let size = u64::from(u32::from_le_bytes(tail[12..16].try_into().expect("size")));
    let offset = u64::from(u32::from_le_bytes(tail[16..20].try_into().expect("offset")));
    let mut result = bytes[..end].to_vec();
    result.extend_from_slice(b"PK\x06\x06");
    result.extend_from_slice(&44_u64.to_le_bytes());
    result.extend_from_slice(&[45, 0, 45, 0]);
    result.extend_from_slice(&[0; 8]);
    for value in [count, count, size, offset] {
        result.extend_from_slice(&value.to_le_bytes());
    }
    result.extend_from_slice(b"PK\x06\x07");
    result.extend_from_slice(&0_u32.to_le_bytes());
    result.extend_from_slice(&u64::try_from(end).expect("offset").to_le_bytes());
    result.extend_from_slice(&1_u32.to_le_bytes());
    let mut tail = tail.to_vec();
    tail[8..20].fill(255);
    result.extend_from_slice(&tail);
    result
}

#[test]
fn zip64_fixed_end_records_with_maximum_comment_fit_every_profile() {
    let bytes = zip64_end_records(&streaming_zip(usize::from(u16::MAX)));
    for profile in [
        ZipProfile::Recovery,
        ZipProfile::PreviewLeaf,
        ZipProfile::PreviewAggregate,
        ZipProfile::Evidence,
    ] {
        assert_eq!(
            manifest(&bytes, profile).expect("ZIP64 metadata"),
            b"native-proof"
        );
    }
}

#[test]
fn absolute_offsets_allow_harmless_prefix_but_never_autodetect_an_sfx_offset() {
    let bytes = streaming_zip(0);
    let mut prefixed = b"PREFIX".to_vec();
    prefixed.extend_from_slice(&bytes);
    for profile in [
        ZipProfile::Recovery,
        ZipProfile::PreviewLeaf,
        ZipProfile::PreviewAggregate,
    ] {
        assert!(manifest(&prefixed, profile).is_err(), "no offset fallback");
    }
    let end = prefixed.len() - 22;
    let offset = u32::from_le_bytes(prefixed[end + 16..end + 20].try_into().expect("offset"));
    let mut central = usize::try_from(offset).expect("offset") + 6;
    for _ in 0..3 {
        assert_eq!(&prefixed[central..central + 4], b"PK\x01\x02");
        let local = u32::from_le_bytes(
            prefixed[central + 42..central + 46]
                .try_into()
                .expect("local"),
        );
        prefixed[central + 42..central + 46].copy_from_slice(&(local + 6).to_le_bytes());
        let variable: usize = [28, 30, 32]
            .into_iter()
            .map(|n| {
                usize::from(u16::from_le_bytes(
                    prefixed[central + n..central + n + 2]
                        .try_into()
                        .expect("length"),
                ))
            })
            .sum();
        central += 46 + variable;
    }
    prefixed[end + 16..end + 20].copy_from_slice(&(offset + 6).to_le_bytes());
    for profile in [
        ZipProfile::Recovery,
        ZipProfile::PreviewLeaf,
        ZipProfile::PreviewAggregate,
    ] {
        assert_eq!(
            manifest(&prefixed, profile).expect("absolute offsets"),
            b"native-proof"
        );
    }
}

#[test]
fn false_footer_storm_and_excessive_declared_entries_still_fail_closed() {
    let mut storm = streaming_zip(usize::from(u16::MAX));
    let comment = storm.len() - usize::from(u16::MAX);
    for chunk in storm[comment..].as_chunks_mut::<22>().0 {
        chunk.fill(255);
        chunk[..4].copy_from_slice(b"PK\x05\x06");
    }
    let mut bomb = streaming_zip(0);
    let end = bomb.len() - 22;
    bomb[end + 8..end + 12].fill(255);
    let mut corrupt = streaming_zip(0);
    let end = corrupt.len() - 22;
    corrupt[end + 16..end + 20].copy_from_slice(&1_u32.to_le_bytes());
    for bytes in [storm, bomb, corrupt] {
        let operations = Rc::new(Cell::new(0));
        let input = Counted {
            input: Cursor::new(bytes),
            bytes: Rc::new(Cell::new(0)),
            operations: Rc::clone(&operations),
        };
        CheckDeadline::after(Duration::from_secs(5))
            .expect("deadline")
            .scope(|| {
                assert!(reader::open(input, ZipProfile::PreviewLeaf).is_err());
            })
            .expect("scope");
        assert!(
            operations.get() <= 354,
            "finite metadata work plus initial two seeks"
        );
    }

    let mut evidence_storm = streaming_zip(usize::from(u16::MAX));
    let comment = evidence_storm.len() - usize::from(u16::MAX);
    for chunk in evidence_storm[comment..].as_chunks_mut::<22>().0 {
        chunk.fill(255);
        chunk[..4].copy_from_slice(b"PK\x05\x06");
    }
    let operations = Rc::new(Cell::new(0));
    let input = Counted {
        input: Cursor::new(evidence_storm),
        bytes: Rc::new(Cell::new(0)),
        operations: Rc::clone(&operations),
    };
    CheckDeadline::after(Duration::from_secs(5))
        .expect("deadline")
        .scope(|| drop(reader::open(input, ZipProfile::Evidence)))
        .expect("scope");
    assert!(operations.get() <= 256 + ZipProfile::Evidence.entries() * 32 + 2);
}
