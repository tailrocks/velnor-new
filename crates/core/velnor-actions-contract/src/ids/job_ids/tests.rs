//! Job-ID validation unit tests.
//!
//! Canonical sibling suite for `super` (declared `#[cfg(test)] mod tests;`).

use super::validate_job_id;

#[test]
fn branding_gate_skips_package_slugs_only() {
    assert!(validate_job_id("plan").is_ok());
    assert!(validate_job_id("rust-demo").is_ok());
    assert!(validate_job_id("rust-velnor-actions-contract").is_ok());
    assert!(validate_job_id("tofu-velnor-infra").is_ok());
    assert!(validate_job_id("velnor-plan").is_err());
    assert!(validate_job_id("task-velnor-final").is_err());
    assert!(validate_job_id("").is_err());
}
