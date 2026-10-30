use super::super::RuntimeIdentityV1;
use super::decode_identity;
use crate::qualification_cache_payload::storage_admission::mountinfo::validate_read_only_mountinfo;

fn identity() -> RuntimeIdentityV1 {
    super::test_identity()
}

fn mount_line(id: u64, target: &str, options: &str) -> String {
    format!("{id} 25 0:55 / {target} {options} rw,relatime - tmpfs tmpfs rw\n")
}

#[test]
fn runtime_identity_requires_the_exact_canonical_v1_image_record() {
    let bytes = serde_json::to_vec(&identity()).expect("identity bytes");
    assert_eq!(decode_identity(&bytes), Ok(identity()));

    let mut changed = identity();
    changed.runner_source_revision = "g".repeat(40);
    let bytes = serde_json::to_vec(&changed).expect("identity bytes");
    assert_eq!(
        decode_identity(&bytes),
        Err("cache_storage_runtime_identity_invalid")
    );
}

#[test]
fn runtime_identity_rejects_unknown_fields_duplicates_and_noncanonical_json() {
    let valid = serde_json::to_vec(&identity()).expect("identity bytes");
    let mut duplicate = valid.clone();
    duplicate.pop();
    duplicate.extend_from_slice(b",\"schema_version\":1}");
    assert_eq!(
        decode_identity(&duplicate),
        Err("cache_storage_runtime_identity_invalid")
    );
    let unknown = valid[..valid.len() - 1]
        .iter()
        .copied()
        .chain(b",\"extra\":true}".iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(
        decode_identity(&unknown),
        Err("cache_storage_runtime_identity_invalid")
    );
    let pretty = serde_json::to_vec_pretty(&identity()).expect("pretty identity");
    assert_eq!(
        decode_identity(&pretty),
        Err("cache_storage_runtime_identity_noncanonical")
    );
}

#[test]
fn runtime_identity_mount_must_be_one_exact_read_only_direct_root_mount() {
    let expected = "/.velnor-runtime-identity";
    assert_eq!(
        validate_read_only_mountinfo(
            &mount_line(42, expected, "ro,nosuid,nodev"),
            expected,
            42,
            0,
            55
        ),
        Ok(())
    );
    assert_eq!(
        validate_read_only_mountinfo(
            &mount_line(42, expected, "rw,nosuid,nodev"),
            expected,
            42,
            0,
            55
        ),
        Err("cache_storage_mount_not_read_only_bind")
    );
    assert_eq!(
        validate_read_only_mountinfo(
            &mount_line(42, "/opt/velnor-runtime-identity", "ro"),
            expected,
            42,
            0,
            55,
        ),
        Err("cache_storage_mount_missing")
    );
    let duplicated = format!(
        "{}{}",
        mount_line(42, expected, "ro"),
        mount_line(42, expected, "ro")
    );
    assert_eq!(
        validate_read_only_mountinfo(&duplicated, expected, 42, 0, 55),
        Err("cache_storage_mount_ambiguous")
    );
}
