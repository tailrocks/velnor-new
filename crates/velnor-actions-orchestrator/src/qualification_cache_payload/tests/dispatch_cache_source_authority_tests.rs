use super::SourceDirectory;
use crate::qualification_cache_payload::manifest::PayloadRoot;
use crate::qualification_cache_payload::roots::{CacheRootId, CacheRootObservation, CacheRootSet};
use crate::qualification_cache_payload::transfer::payload::{digest_open_directory, digest_stage};
use crate::qualification_cache_payload::transfer::write_transport_payload;
use std::fs;
use std::io::Cursor;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use velnor_actions_contract::workflow::QualificationCacheLayer;

#[test]
fn source_digest_rejects_a_nonsticky_writable_ancestor() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical scratch");
    let parent = base.join("writable-parent");
    let source = parent.join("cache-root");
    fs::create_dir_all(&source).expect("source root");
    fs::write(source.join("marker"), b"payload").expect("marker");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).expect("writable parent");

    assert_eq!(
        digest_stage(&source),
        Err("qualification_cache_payload_source_ancestor_invalid")
    );
}

#[test]
fn payload_writer_rejects_a_nonsticky_writable_source_ancestor() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical scratch");
    let parent = base.join("writable-parent");
    let source_path = parent.join("cache-root");
    fs::create_dir_all(&source_path).expect("source root");
    fs::write(source_path.join("marker"), b"payload").expect("marker");
    let digest = digest_stage(&source_path)
        .expect("safe source digest")
        .expect("nonempty source");
    let mode = fs::metadata(&source_path).expect("root metadata").mode() & 0o777;
    let roots = CacheRootSet {
        matrix_key: "source-check".to_owned(),
        layer: QualificationCacheLayer::TofuProviders,
        roots: vec![CacheRootObservation {
            id: CacheRootId::TofuPlugins,
            path: source_path.clone(),
        }],
    };
    let payload_roots = [PayloadRoot {
        id: CacheRootId::TofuPlugins,
        present: true,
        mode: Some(mode),
        digest: Some(digest),
    }];
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o777)).expect("writable parent");
    let mut output = Cursor::new(Vec::new());

    assert_eq!(
        write_transport_payload(&mut output, 65_536, &roots, &payload_roots, "{}"),
        Err("qualification_cache_payload_source_ancestor_invalid")
    );
}

#[test]
fn retained_source_descriptor_detects_ancestor_replacement() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical scratch");
    let original = base.join("parent");
    let source_path = original.join("cache-root");
    let replacement = base.join("replacement");
    let displaced = base.join("displaced");
    fs::create_dir_all(&source_path).expect("source root");
    fs::write(source_path.join("marker"), b"original payload").expect("source marker");
    fs::create_dir_all(replacement.join("cache-root")).expect("replacement root");
    fs::write(
        replacement.join("cache-root/marker"),
        b"replacement payload",
    )
    .expect("replacement marker");
    let source = SourceDirectory::open(&source_path).expect("retained source");
    let digest = digest_open_directory(source.fd())
        .expect("digest through retained descriptor")
        .expect("source digest");

    fs::rename(&original, &displaced).expect("displace ancestor");
    fs::rename(&replacement, &original).expect("replace ancestor");
    assert_eq!(
        digest_open_directory(source.fd()).expect("retained descriptor digest"),
        Some(digest)
    );
    assert_eq!(
        source.revalidate(),
        Err("qualification_cache_payload_file_changed")
    );

    fs::rename(&original, &replacement).expect("move replacement away");
    fs::rename(&displaced, &original).expect("restore ancestor");
    assert_eq!(source.revalidate(), Ok(()));
}
