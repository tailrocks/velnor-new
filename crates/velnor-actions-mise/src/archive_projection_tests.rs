//! Pure source fixtures; they never insert a qualified production registry entry.

use super::identity::{ActionEntrypoint, ImmutableAction, QualifiedRuntimeClosure};
use super::*;

fn tuple() -> CapabilityTuple {
    CapabilityTuple {
        action: ImmutableAction {
            repository: "fixture/cache".into(),
            commit: "1".repeat(40),
        },
        restore: ActionEntrypoint {
            path: "dist/restore/index.js".into(),
            bundle_sha256: "2".repeat(64),
        },
        save: ActionEntrypoint {
            path: "dist/save/index.js".into(),
            bundle_sha256: "3".repeat(64),
        },
        adapter_source_closure_sha256: "4".repeat(64),
        inventory_template_sha256: crate::source_archive_inventory::source_template_sha256(
            crate::source_archive_inventory::InventorySourceProgram::Archive,
        ),
        runtime: QualifiedRuntimeClosure {
            closure_sha256: "5".repeat(64),
            host_profile: "fixture-linux-x86_64-foundation-v1".into(),
            tar_sha256: "6".repeat(64),
            node_sha256: "7".repeat(64),
            codec_abi: "fixture-zstd-v1".into(),
        },
        transform_revision: "fixture-v1".into(),
    }
}

#[test]
fn no_registry_authority_and_source_fixture_is_descriptive() {
    assert!(qualified_source_archive_projection().is_none());
    let projection = SourceArchiveProjection::fixture(tuple()).expect("pure fixture");
    assert_eq!(projection.identity(), projection.tuple.sha256());
    let source = projection.source_literals();
    assert!(source.contains(projection.identity()));
    assert!(!source.contains("def "));
    assert!(!source.contains("import "));
    assert!(!source.contains("os.environ"));
}

#[test]
fn framing_is_ordered_and_unambiguous() {
    assert_ne!(
        framed_sha256("capability-v1", &[b"ab", b"c"]),
        framed_sha256("capability-v1", &[b"a", b"bc"])
    );
    assert_ne!(
        framed_sha256("capability-v1", &[b"a", b"b"]),
        framed_sha256("payload-v1", &[b"a", b"b"])
    );
    assert_ne!(
        framed_sha256("payload-v1", &[b"a", b"b"]),
        framed_sha256("payload-v1", &[b"b", b"a"])
    );
    assert_ne!(
        framed_sha256("payload-v1", &[b"a"]),
        framed_sha256("payload-v1", &[b"a", b""])
    );
    assert_eq!(
        framed_sha256("capability-v1", &[b"ab", b"c"]),
        "6c6ec732e88f1b562fd70ab2c0daf9abfc51c08e082ec8af581bb572985b2e05"
    );
}

#[test]
fn every_capability_role_changes_identity() {
    let original = tuple();
    let mut variants = vec![original.clone(); 14];
    variants[0].action.repository = "fixture/other".into();
    variants[1].action.commit = "a".repeat(40);
    variants[2].restore.path = "dist/restore/other.js".into();
    variants[3].restore.bundle_sha256 = "a".repeat(64);
    variants[4].save.path = "dist/save/other.js".into();
    variants[5].save.bundle_sha256 = "a".repeat(64);
    variants[6].adapter_source_closure_sha256 = "a".repeat(64);
    variants[7].inventory_template_sha256 = "a".repeat(64);
    variants[8].runtime.closure_sha256 = "a".repeat(64);
    variants[9].runtime.host_profile = "fixture-other-v1".into();
    variants[10].runtime.tar_sha256 = "a".repeat(64);
    variants[11].runtime.node_sha256 = "a".repeat(64);
    variants[12].runtime.codec_abi = "fixture-other-v1".into();
    variants[13].transform_revision = "fixture-v2".into();
    for variant in variants {
        assert_ne!(original.sha256(), variant.sha256());
    }
}

#[test]
fn root_context_is_separate_and_identical_for_every_payload_use() {
    let projection = SourceArchiveProjection::fixture(tuple()).expect("pure fixture");
    let before = projection.payload_context_sha256(&["alpha", "beta"]);
    for _use in ["before", "after", "signed", "rust-health"] {
        assert_eq!(
            before,
            projection.payload_context_sha256(&["alpha", "beta"])
        );
    }
    assert_ne!(
        before,
        projection.payload_context_sha256(&["beta", "alpha"])
    );
    assert_ne!(before, projection.payload_context_sha256(&["beta"]));
    assert_ne!(before, projection.identity());
}

#[test]
fn fixture_validation_rejects_malformed_and_wrapper_template_digests() {
    let mut invalid = tuple();
    invalid.action.commit = "main".into();
    assert_eq!(
        SourceArchiveProjection::fixture(invalid),
        Err("immutable_digest")
    );
    let mut invalid = tuple();
    invalid.inventory_template_sha256 = compiled_source_sha256(b"wrapper-with-literals");
    assert_eq!(
        SourceArchiveProjection::fixture(invalid),
        Err("inventory_template_sha256")
    );
    let mut invalid = tuple();
    invalid.transform_revision = "quote'\nloader".into();
    assert_eq!(
        SourceArchiveProjection::fixture(invalid),
        Err("runtime_or_revision")
    );
    let mut invalid = tuple();
    invalid.restore.path = invalid.save.path.clone();
    assert_eq!(
        SourceArchiveProjection::fixture(invalid),
        Err("entrypoint_roles")
    );
}
