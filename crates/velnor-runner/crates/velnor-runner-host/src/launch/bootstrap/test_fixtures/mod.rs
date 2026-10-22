pub(crate) use super::cleanup::test_verified_storage_cleanup;
pub(crate) use super::containers::{
    test_bootstrap_start_receipt, test_created_bootstrap_container, test_created_storage_object,
};
pub(crate) use super::delivery::test_listener_delivery_receipt;
pub(crate) use super::publication::test_job_facts_publication;
pub(crate) use super::storage::{
    test_observed_container_cgroup, test_observed_storage_attachment, test_observed_storage_role,
    test_verified_storage_resource_binding,
};

pub(super) fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_generation(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_engine_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

pub(super) fn valid_container_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn valid_bounded_text(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value.bytes().all(|byte| !byte.is_ascii_control())
}
