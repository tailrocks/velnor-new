use super::super::super::manifest::PayloadRoot;
use super::super::super::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use super::super::payload::inventory_directory;
use super::super::transport_envelope::read_envelope;
use super::write_payload;
use std::fs;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::os::unix::fs::{PermissionsExt, symlink};
use tempfile::TempDir;
use velnor_actions_contract::QualificationCacheLayer;

struct Fixture {
    _scratch: TempDir,
    roots: CacheRootSet,
    payload_roots: Vec<PayloadRoot>,
    long_relative: String,
}

struct ArchiveMember {
    path: String,
    hard_link: bool,
    symbolic_link: bool,
    link_target: Option<String>,
}

#[test]
fn writer_emits_one_deterministic_v1_envelope() {
    let fixture = fixture();
    assert_eq!(write_archive(&fixture), write_archive(&fixture));
}

#[test]
fn writer_preserves_long_paths_and_link_targets() {
    let fixture = fixture();
    let members = archive_members(&write_archive(&fixture));
    let prefix = format!("roots/tofu_plugins/{}/", fixture.long_relative);

    assert_eq!(members[0].path, "manifest.json");
    assert_eq!(members[1].path, "roots");
    assert_eq!(members[2].path, "roots/tofu_plugins");
    for name in [
        "primary-content.txt",
        "hardlink-content.txt",
        "relative-alias",
    ] {
        assert!(
            members
                .iter()
                .any(|member| member.path == format!("{prefix}{name}"))
        );
    }
    let hardlink = members
        .iter()
        .find(|member| member.hard_link)
        .expect("hardlink member");
    let primary_path = format!("{prefix}primary-content.txt");
    let duplicate_path = format!("{prefix}hardlink-content.txt");
    let link_target = hardlink.link_target.as_deref().expect("hardlink target");
    assert!([primary_path.as_str(), duplicate_path.as_str()].contains(&hardlink.path.as_str()));
    assert!([primary_path.as_str(), duplicate_path.as_str()].contains(&link_target));
    assert_ne!(hardlink.path, link_target);
    let symlink = members
        .iter()
        .find(|member| member.symbolic_link)
        .expect("symbolic-link member");
    assert_eq!(symlink.link_target.as_deref(), Some("primary-content.txt"));
}

#[test]
fn writer_rejects_a_stale_root_digest_before_finishing() {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let root = scratch.path().join("tofu");
    fs::create_dir(&root).expect("cache root");
    fs::write(root.join("provider"), b"provider bytes").expect("provider file");
    let roots = CacheRootSet {
        matrix_key: "normal-tofu".to_owned(),
        layer: QualificationCacheLayer::TofuProviders,
        roots: vec![CacheRootObservation {
            id: CacheRootId::TofuPlugins,
            path: root.clone(),
        }],
    };
    let (inventory, _) = inventory_directory(&root)
        .expect("inventory root")
        .expect("nonempty root");
    let payload_roots = vec![PayloadRoot {
        id: CacheRootId::TofuPlugins,
        present: true,
        mode: Some(inventory.root_mode),
        digest: Some(format!("b3-{}", "0".repeat(64))),
    }];
    let mut output = Cursor::new(Vec::new());
    assert_eq!(
        write_payload(&mut output, 4 * 1024 * 1024, &roots, &payload_roots, "{}",),
        Err("qualification_cache_payload_file_changed")
    );
}

fn fixture() -> Fixture {
    let scratch = tempfile::tempdir().expect("scratch directory");
    let root = scratch.path().join("tofu");
    let nested = root.join("long-directory-name-".repeat(8));
    fs::create_dir_all(&nested).expect("nested cache directory");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).expect("root mode");
    let original = nested.join("primary-content.txt");
    fs::write(&original, b"provider bytes\n").expect("provider file");
    fs::hard_link(&original, nested.join("hardlink-content.txt")).expect("hard link");
    symlink("primary-content.txt", nested.join("relative-alias")).expect("internal symbolic link");

    let roots = CacheRootSet {
        matrix_key: "normal-tofu".to_owned(),
        layer: QualificationCacheLayer::TofuProviders,
        roots: vec![CacheRootObservation {
            id: CacheRootId::TofuPlugins,
            path: root.clone(),
        }],
    };
    let (inventory, _) = inventory_directory(&root)
        .expect("inventory root")
        .expect("nonempty root");
    let payload_roots = vec![PayloadRoot {
        id: CacheRootId::TofuPlugins,
        present: true,
        mode: Some(inventory.root_mode),
        digest: Some(inventory.digest),
    }];
    let long_relative = nested
        .strip_prefix(&root)
        .expect("relative nested path")
        .to_string_lossy()
        .into_owned();
    Fixture {
        _scratch: scratch,
        roots,
        payload_roots,
        long_relative,
    }
}

fn write_archive(fixture: &Fixture) -> Vec<u8> {
    let mut output = Cursor::new(Vec::new());
    write_payload(
        &mut output,
        4 * 1024 * 1024,
        &fixture.roots,
        &fixture.payload_roots,
        "{}",
    )
    .expect("write cache transport");
    output.into_inner()
}

fn archive_members(bytes: &[u8]) -> Vec<ArchiveMember> {
    let mut cursor = Cursor::new(bytes);
    let range = read_envelope(&mut cursor, 4 * 1024 * 1024).expect("valid envelope");
    cursor
        .seek(SeekFrom::Start(range.offset))
        .expect("seek tar");
    let mut raw_tar = vec![0; usize::try_from(range.length).expect("tar fits test memory")];
    cursor.read_exact(&mut raw_tar).expect("read tar");
    tar::Archive::new(Cursor::new(raw_tar))
        .entries()
        .expect("tar entries")
        .map(|entry| {
            let entry = entry.expect("valid entry");
            let link_target = entry
                .link_name()
                .expect("valid link metadata")
                .map(|path| path.to_string_lossy().into_owned());
            Ok(ArchiveMember {
                path: entry
                    .path()
                    .expect("PAX path resolves")
                    .to_string_lossy()
                    .into_owned(),
                hard_link: entry.header().entry_type().is_hard_link(),
                symbolic_link: entry.header().entry_type().is_symlink(),
                link_target,
            })
        })
        .collect::<Result<Vec<_>, &'static str>>()
        .expect("collect archive members")
}
