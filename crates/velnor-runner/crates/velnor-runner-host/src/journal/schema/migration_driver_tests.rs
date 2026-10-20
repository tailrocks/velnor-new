use super::expected_next_version;

#[test]
fn migration_driver_accepts_only_known_next_versions() {
    for version in 0..=8 {
        assert_eq!(expected_next_version(version), Some(version + 1));
    }
    assert_eq!(expected_next_version(9), Some(11));
    for version in 10..=32 {
        assert_eq!(expected_next_version(version), Some(version + 1));
    }
    for version in [-1, 33] {
        assert_eq!(expected_next_version(version), None);
    }
}
