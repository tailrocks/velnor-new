use crate::journal::StorageMedium;

use super::capacity::usage_credit;

#[test]
fn exclusive_tmpfs_credit_uses_checked_used_totals() {
    assert_eq!(
        usage_credit(StorageMedium::Tmpfs, 4096, 3072, 128, 96),
        Ok((1024, 32))
    );
}

#[test]
fn shared_disk_receives_no_capacity_credit() {
    assert_eq!(
        usage_credit(StorageMedium::Disk, 64 * 1024, 60 * 1024, 4096, 3900),
        Ok((0, 0))
    );
}

#[test]
fn impossible_free_counts_fail_closed() {
    assert!(usage_credit(StorageMedium::Tmpfs, 10, 11, 10, 10).is_err());
    assert!(usage_credit(StorageMedium::Tmpfs, 10, 9, 10, 11).is_err());
}
