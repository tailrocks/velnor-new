use std::{collections::BTreeSet, io::Cursor, time::Instant};

use flate2::{Compression, write::GzEncoder};

fn archive(entries: &[(&str, &[u8], tar::EntryType)]) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for (name, bytes, entry_type) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(*entry_type);
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, name, Cursor::new(bytes))
            .expect("append");
    }
    builder
        .into_inner()
        .expect("finish tar")
        .finish()
        .expect("finish gzip")
}

#[test]
fn held_byte_extractor_returns_only_the_exact_regular_inventory() {
    let bytes = archive(&[("jackin", b"payload", tar::EntryType::Regular)]);
    let expected = BTreeSet::from(["jackin".to_owned()]);
    let actual = super::extract_tar_gz_members(&bytes, &expected, Instant::now()).expect("extract");
    assert_eq!(actual.get("jackin").expect("member"), b"payload");
}

#[test]
fn held_byte_extractor_rejects_links_and_unlisted_members() {
    let symlink = archive(&[("jackin", b"", tar::EntryType::Symlink)]);
    let expected = BTreeSet::from(["jackin".to_owned()]);
    assert!(super::extract_tar_gz_members(&symlink, &expected, Instant::now()).is_err());

    let extra = archive(&[
        ("jackin", b"a", tar::EntryType::Regular),
        ("other", b"b", tar::EntryType::Regular),
    ]);
    assert!(super::extract_tar_gz_members(&extra, &expected, Instant::now()).is_err());
}
