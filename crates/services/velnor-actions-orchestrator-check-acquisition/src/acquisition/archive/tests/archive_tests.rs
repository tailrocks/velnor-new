use super::*;

use flate2::Compression;
use flate2::write::GzEncoder;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::time::Duration;
use tar::{Builder, EntryType, Header};
struct TarEntry {
    path: &'static str,
    kind: EntryType,
    data: &'static [u8],
    mode: u32,
    link: Option<&'static str>,
}

fn tar_gz(path: &Path, entries: &[TarEntry]) {
    let file = fs::File::create(path).expect("archive");
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);
    for item in entries {
        let mut header = Header::new_gnu();
        if item.path.starts_with("../") {
            let raw = item.path.as_bytes();
            header.as_mut_bytes()[..raw.len()].copy_from_slice(raw);
        } else {
            header.set_path(item.path).expect("path");
        }
        header.set_entry_type(item.kind);
        header.set_mode(item.mode);
        header.set_size(item.data.len() as u64);
        if let Some(link) = item.link {
            header.set_link_name(link).expect("link");
        }
        header.set_cksum();
        builder
            .append(&header, Cursor::new(item.data))
            .expect("entry");
    }
    builder
        .into_inner()
        .expect("tar finish")
        .finish()
        .expect("gzip finish");
}

#[test]
fn zip_extracts_wrapper_and_preserves_executable_bits() {
    let temp = scratch("zip");
    let root = scratch_root(&temp);
    let archive = root.join("tool.zip");
    zip_file(
        &archive,
        &[
            ("tool-1.2/bin/tool", b"tool", 0o755),
            ("tool-1.2/lib/data", b"x", 0o644),
        ],
    );
    let destination = root.join("prefix");
    extract(
        &archive,
        &destination,
        "https://example.test/tool.zip?sha=1",
    )
    .expect("extract");
    assert_eq!(
        fs::read(destination.join("tool-1.2/bin/tool")).expect("read"),
        b"tool"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            fs::metadata(destination.join("tool-1.2/bin/tool"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o111,
            0
        );
    }
}

#[test]
fn crate_archive_extracts_wrapper() {
    let temp = scratch("crate");
    let root = scratch_root(&temp);
    let archive = root.join("crate");
    tar_gz(
        &archive,
        &[TarEntry {
            path: "package/src/lib.rs",
            kind: EntryType::Regular,
            data: b"pub fn ok() {}",
            mode: 0o755,
            link: None,
        }],
    );
    let destination = root.join("prefix");
    extract(&archive, &destination, "https://example.test/package.crate").expect("extract");
    assert_eq!(
        fs::read(destination.join("package/src/lib.rs")).expect("read"),
        b"pub fn ok() {}"
    );
}

#[cfg(unix)]
#[test]
fn safe_in_root_symlink_is_staged_after_regular_files() {
    let temp = scratch("safe-link");
    let root = scratch_root(&temp);
    let archive = root.join("tool.tar.gz");
    tar_gz(
        &archive,
        &[
            TarEntry {
                path: "tool/bin/tool",
                kind: EntryType::Regular,
                data: b"tool",
                mode: 0o755,
                link: None,
            },
            TarEntry {
                path: "tool/bin/alias",
                kind: EntryType::Symlink,
                data: b"",
                mode: 0o777,
                link: Some("tool"),
            },
        ],
    );
    let destination = root.join("prefix");
    extract(&archive, &destination, "https://example.test/tool.tgz").expect("extract");
    assert_eq!(
        fs::read_link(destination.join("tool/bin/alias")).expect("link"),
        Path::new("tool")
    );
}

#[test]
fn tar_parent_path_is_rejected_and_destination_removed() {
    let temp = scratch("parent");
    let root = scratch_root(&temp);
    let archive = root.join("bad.tar.gz");
    tar_gz(
        &archive,
        &[TarEntry {
            path: "../escape",
            kind: EntryType::Regular,
            data: b"bad",
            mode: 0o644,
            link: None,
        }],
    );
    let destination = root.join("prefix");
    assert!(extract(&archive, &destination, "https://example.test/bad.tar.gz").is_err());
    assert!(!destination.exists());
    assert!(!root.join("escape").exists());
}

#[test]
fn tar_symlink_is_rejected() {
    let temp = scratch("symlink");
    let root = scratch_root(&temp);
    let archive = root.join("bad.tar.gz");
    tar_gz(
        &archive,
        &[TarEntry {
            path: "tool/link",
            kind: EntryType::Symlink,
            data: b"",
            mode: 0o777,
            link: Some("../../outside"),
        }],
    );
    let destination = root.join("prefix");
    assert!(extract(&archive, &destination, "https://example.test/bad.tgz").is_err());
    assert!(!destination.exists());
}

#[test]
fn tar_hardlink_is_rejected() {
    let temp = scratch("hardlink");
    let root = scratch_root(&temp);
    let archive = root.join("bad.tar.gz");
    tar_gz(
        &archive,
        &[TarEntry {
            path: "tool/link",
            kind: EntryType::Link,
            data: b"",
            mode: 0o644,
            link: Some("tool/bin/tool"),
        }],
    );
    let destination = root.join("prefix");
    assert!(extract(&archive, &destination, "https://example.test/bad.tar.gz").is_err());
    assert!(!destination.exists());
}

#[test]
fn duplicate_paths_are_rejected() {
    let temp = scratch("duplicate");
    let root = scratch_root(&temp);
    let archive = root.join("bad.tar.gz");
    tar_gz(
        &archive,
        &[
            TarEntry {
                path: "tool/bin/tool",
                kind: EntryType::Regular,
                data: b"one",
                mode: 0o644,
                link: None,
            },
            TarEntry {
                path: "tool/bin/tool",
                kind: EntryType::Regular,
                data: b"two",
                mode: 0o644,
                link: None,
            },
        ],
    );
    let destination = root.join("prefix");
    assert!(extract(&archive, &destination, "https://example.test/bad.crate").is_err());
    assert!(!destination.exists());
}

#[test]
fn zip_symlink_mode_is_rejected() {
    let temp = scratch("zip-link");
    let root = scratch_root(&temp);
    let archive = root.join("bad.zip");
    zip_file(&archive, &[("tool/link", b"outside", 0o644)]);
    let mut bytes = fs::read(&archive).expect("zip bytes");
    let marker = b"PK\x01\x02";
    let central = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("central directory");
    let mode = (0o120_777_u32 << 16).to_le_bytes();
    bytes[central + 38..central + 42].copy_from_slice(&mode);
    fs::write(&archive, bytes).expect("patched zip");
    let destination = root.join("prefix");
    assert!(extract(&archive, &destination, "https://example.test/bad.zip").is_err());
    assert!(!destination.exists());
}

#[test]
fn declared_entry_size_limit_is_checked_before_writing() {
    let temp = scratch("limit");
    let root = scratch_root(&temp);
    let archive = root.join("large.tar.gz");
    let file = fs::File::create(&archive).expect("archive");
    let mut encoder = GzEncoder::new(file, Compression::default());
    let mut header = Header::new_gnu();
    header.set_path("too-large").expect("path");
    header.set_size(MAX_ENTRY_BYTES + 1);
    header.set_mode(0o644);
    header.set_cksum();
    encoder.write_all(header.as_bytes()).expect("header");
    encoder.write_all(&[0; 1024]).expect("end blocks");
    encoder.finish().expect("gzip finish");
    let destination = root.join("prefix");
    assert!(extract(&archive, &destination, "https://example.test/large.tgz").is_err());
    assert!(!destination.exists());
}

#[test]
fn global_budget_is_admitted_before_output_creation() {
    let temp = scratch("global-budget");
    let root = scratch_root(&temp);
    let archive = root.join("tool.tar.gz");
    tar_gz(
        &archive,
        &[TarEntry {
            path: "tool/bin/tool",
            kind: EntryType::Regular,
            data: b"tool",
            mode: 0o755,
            link: None,
        }],
    );
    let destination = root.join("prefix");
    let mut budget = ArchiveBudget {
        bytes: 0,
        entries: 1,
    };
    assert!(
        extract_archive(
            &archive,
            &destination,
            "https://example.test/tool.tgz",
            &mut budget,
            CheckDeadline::after(Duration::from_secs(60)).expect("deadline"),
        )
        .is_err()
    );
    assert!(!destination.exists());
    assert_eq!(budget.bytes, 0);
    assert_eq!(budget.entries, 0);
}

#[cfg(unix)]
#[test]
fn archive_source_symlink_is_rejected() {
    let temp = scratch("source-link");
    let root = scratch_root(&temp);
    let actual = root.join("actual.zip");
    zip_file(&actual, &[("tool", b"x", 0o644)]);
    let linked = root.join("linked.zip");
    std::os::unix::fs::symlink(&actual, &linked).expect("link");
    let destination = root.join("prefix");
    assert!(extract(&linked, &destination, "https://example.test/tool.zip").is_err());
}
