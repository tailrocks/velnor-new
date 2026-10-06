//! Complete fixed fixture source, assembled exclusively from compiled bytes.

pub(super) fn compiled() -> String {
    [
        include_str!("package_update_setup.sh"),
        include_str!("package_update_preview.sh"),
        include_str!("package_update_cases.sh"),
    ]
    .concat()
}
