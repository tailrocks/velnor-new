use std::io::Cursor;

use serde_json::Value;
use tar::{Builder, EntryType};

use super::new_header;

pub(in crate::launch::resource_probe::provider::archive::worker::tests) fn rewrite_json_entry(
    archive_bytes: &[u8],
    target: &str,
    mutate: impl FnOnce(&mut Value),
) -> Vec<u8> {
    let mut archive = tar::Archive::new(Cursor::new(archive_bytes));
    let mut builder = Builder::new(Vec::new());
    let mut replacement = Some(mutate);
    let mut found = false;
    for entry in archive.entries().expect("source entries are valid") {
        let mut entry = entry.expect("source entry is valid");
        let path = entry
            .path()
            .expect("source entry path is valid")
            .to_str()
            .expect("source entry path is utf8")
            .to_owned();
        if path == target {
            found = true;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).expect("source metadata is read");
            let mut value: Value =
                serde_json::from_slice(&bytes).expect("source metadata JSON is valid");
            replacement.take().expect("target entry occurs once")(&mut value);
            let bytes = serde_json::to_vec(&value).expect("replacement JSON is valid");
            let mut header = entry.header().clone();
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            builder
                .append(&header, Cursor::new(bytes))
                .expect("replacement metadata is written");
        } else {
            let header = entry.header().clone();
            builder
                .append(&header, &mut entry)
                .expect("source entry is copied");
        }
    }
    assert!(found, "target entry exists in source archive");
    builder.into_inner().expect("replacement archive finalizes")
}

pub(in crate::launch::resource_probe::provider::archive::worker::tests) fn append_file_entry(
    archive_bytes: &[u8],
    path: &str,
    bytes: &[u8],
) -> Vec<u8> {
    let mut builder = copy_archive(archive_bytes);
    let mut header = new_header(EntryType::Regular, bytes.len() as u64);
    assert!(path.len() <= 100, "test path fits the tar name field");
    header.as_mut_bytes()[..100].fill(0);
    header.as_mut_bytes()[..path.len()].copy_from_slice(path.as_bytes());
    header.set_cksum();
    builder
        .append(&header, Cursor::new(bytes))
        .expect("appended file entry is written");
    builder
        .into_inner()
        .expect("archive with appended file finalizes")
}

pub(in crate::launch::resource_probe::provider::archive::worker::tests) fn append_pax_link(
    archive_bytes: &[u8],
    path: &str,
    target: &str,
) -> Vec<u8> {
    let mut builder = copy_archive(archive_bytes);
    builder
        .append_pax_extensions([("linkpath", target.as_bytes())])
        .expect("PAX link extension is written");
    let mut header = new_header(EntryType::Symlink, 0);
    builder
        .append_link(&mut header, path, "safe-target")
        .expect("symbolic link entry is written");
    builder
        .into_inner()
        .expect("archive with PAX link finalizes")
}

fn copy_archive(bytes: &[u8]) -> Builder<Vec<u8>> {
    let mut archive = tar::Archive::new(Cursor::new(bytes));
    let mut builder = Builder::new(Vec::new());
    for entry in archive.entries().expect("source entries are valid") {
        let mut entry = entry.expect("source entry is valid");
        let header = entry.header().clone();
        builder
            .append(&header, &mut entry)
            .expect("source entry is copied");
    }
    builder
}
