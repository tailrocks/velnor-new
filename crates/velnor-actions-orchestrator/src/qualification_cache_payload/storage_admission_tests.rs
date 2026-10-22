use super::{canonical_instance_binding_digest, canonical_projection_digest, parse_projection};
#[path = "storage_admission_tests/fixtures.rs"]
mod fixtures;
use fixtures::{bytes, digest};
pub(super) use fixtures::{document, parsed_budget};
use std::path::Path;

#[test]
fn exact_projection_binds_the_job_and_old_samples_do_not_expire_retained_authority() {
    let value = document();
    let budget = parsed_budget(&value);
    assert_eq!(budget.limits(), (4_000_000, 400_000, 5_000_000, 500_000));
    assert_eq!(
        budget.operation_limit(4 * 1024 * 1024 * 1024, 100, 10, 200, 20),
        Ok(3_999_900)
    );
}

#[test]
fn restore_budget_counts_compressed_archive_and_live_import_growth() {
    let budget = parsed_budget(&document());
    let compressed_archive_bytes = 1_000_000;
    let fresh = super::measure::FreshCapacity {
        available_bytes: 100_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    let limits = budget
        .restore_limits(
            4 * 1024 * 1024,
            compressed_archive_bytes,
            super::measure::CacheTreeUsage {
                bytes: 1_000,
                inodes: 10,
            },
            super::measure::CacheTreeUsage {
                bytes: 2_000,
                inodes: 20,
            },
            fresh,
        )
        .expect("bounded warm restore");
    assert_eq!(limits.max_download_bytes(), 4_986_634);
    assert!(limits.raw_stage_bytes() < 4 * 1024 * 1024);
    assert!(limits.live_growth_bytes() <= fresh.available_bytes);
}

#[test]
fn restore_budget_rejects_archive_download_before_action_when_scratch_is_short() {
    let budget = parsed_budget(&document());
    let fresh = super::measure::FreshCapacity {
        available_bytes: 2_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    assert_eq!(
        budget.restore_limits(
            4 * 1024 * 1024,
            1_990_000,
            super::measure::CacheTreeUsage {
                bytes: 1_000,
                inodes: 10,
            },
            super::measure::CacheTreeUsage {
                bytes: 2_000,
                inodes: 20,
            },
            fresh,
        ),
        Err("cache_storage_download_budget_exceeded")
    );
}

#[test]
fn restore_budget_rejects_small_stage_remainder_without_subtraction_underflow() {
    let budget = parsed_budget(&document());
    let fresh = super::measure::FreshCapacity {
        available_bytes: 100_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    assert_eq!(
        budget.restore_limits(
            4 * 1024 * 1024,
            1_000,
            super::measure::CacheTreeUsage {
                bytes: 1_000,
                inodes: 10,
            },
            super::measure::CacheTreeUsage {
                bytes: 4_992_000,
                inodes: 20,
            },
            fresh,
        ),
        Err("cache_storage_restore_capacity_insufficient")
    );
}

#[test]
fn restore_budget_rejects_live_growth_and_inode_shortfalls() {
    let mut value = document();
    value.cache.live_bytes = 10_000;
    value.cache.live_inodes = 1_000;
    value.instance_binding.roles[10].charge_bytes = 10_000;
    value.instance_binding.roles[10].charge_inodes = 1_000;
    value.instance_binding.roles[2].charge_bytes = 10_015_000;
    value.instance_binding.roles[2].charge_inodes = 1_000_000;
    value.instance_binding_digest =
        canonical_instance_binding_digest(&value.instance_binding).expect("binding digest");
    value.projection_digest = canonical_projection_digest(&value).expect("projection digest");
    let budget = parsed_budget(&value);
    let fresh = super::measure::FreshCapacity {
        available_bytes: 100_000_000,
        available_inodes: Some(100_000),
        fragment_bytes: 4_096,
    };
    assert_eq!(
        budget.restore_limits(
            4 * 1024 * 1024,
            1_000,
            super::measure::CacheTreeUsage {
                bytes: 9_500,
                inodes: 900,
            },
            super::measure::CacheTreeUsage {
                bytes: 2_000,
                inodes: 20,
            },
            fresh,
        ),
        Err("cache_storage_restore_capacity_insufficient")
    );
}

#[test]
fn projection_rejects_runner_and_path_substitution() {
    let value = document();
    assert_eq!(
        parse_projection(&bytes(&value), Path::new("/tmp/runner"), "runner-other").err(),
        Some("cache_storage_runner_binding_mismatch")
    );
    assert_eq!(
        parse_projection(&bytes(&value), Path::new("/tmp/elsewhere"), "runner-17").err(),
        Some("cache_storage_cache_path_or_limit_invalid")
    );
}

#[test]
fn guest_image_sha256_is_validated_and_bound_into_the_projection_digest() {
    let value = document();
    let mut changed = value.clone();
    changed.guest_image_sha256 = digest('f');
    assert_ne!(
        canonical_projection_digest(&value).expect("projection digest"),
        canonical_projection_digest(&changed).expect("changed projection digest")
    );

    let mut invalid = value;
    invalid.guest_image_sha256 = "G".repeat(64);
    invalid.projection_digest = canonical_projection_digest(&invalid).expect("projection digest");
    assert_eq!(
        parse_projection(&bytes(&invalid), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_digest_invalid")
    );
}

#[test]
fn projection_rejects_unknown_duplicate_noncanonical_and_bad_digest_fields() {
    let value = document();
    let mut unknown = serde_json::to_value(&value).expect("value");
    unknown["extra"] = serde_json::json!(true);
    assert_eq!(
        parse_projection(
            &serde_json::to_vec(&unknown).expect("serialize"),
            Path::new("/tmp/runner"),
            "runner-17"
        )
        .err(),
        Some("cache_storage_projection_invalid")
    );

    let mut duplicate = bytes(&value);
    let tail = duplicate.pop().expect("closing object");
    assert_eq!(tail, b'}');
    duplicate.extend_from_slice(br#","version":1}"#);
    assert_eq!(
        parse_projection(&duplicate, Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_projection_invalid")
    );

    let mut noncanonical = bytes(&value);
    noncanonical.insert(0, b' ');
    assert_eq!(
        parse_projection(&noncanonical, Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_projection_noncanonical")
    );

    let mut changed = value;
    changed.job_facts_mount.read_only = false;
    assert_eq!(
        parse_projection(&bytes(&changed), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_job_facts_mount_invalid")
    );
}

#[test]
fn projection_rejects_cross_binding_and_alias_limit_changes() {
    let mut changed = document();
    changed.instance_binding.sample_observed_at_unix_ms += 1;
    changed.instance_binding_digest =
        canonical_instance_binding_digest(&changed.instance_binding).expect("digest");
    changed.projection_digest = canonical_projection_digest(&changed).expect("digest");
    assert_eq!(
        parse_projection(&bytes(&changed), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_projection_binding_invalid")
    );

    let mut changed = document();
    changed.cache.stage_bytes -= 1;
    changed.projection_digest = canonical_projection_digest(&changed).expect("digest");
    assert_eq!(
        parse_projection(&bytes(&changed), Path::new("/tmp/runner"), "runner-17").err(),
        Some("cache_storage_alias_limits_invalid")
    );
}

#[test]
fn budget_refuses_live_stage_and_inode_overruns() {
    let budget = parsed_budget(&document());
    assert_eq!(
        budget.operation_limit(4_096, 4_000_001, 10, 1, 1),
        Err("cache_storage_live_budget_exceeded")
    );
    assert_eq!(
        budget.operation_limit(4_096, 1, 1, 5_000_001, 1),
        Err("cache_storage_stage_budget_exceeded")
    );
    assert_eq!(
        budget.operation_limit(4_096, 1, 400_001, 1, 1),
        Err("cache_storage_inode_budget_exceeded")
    );
}
