//! Default job-timeout helpers and serialization predicates for `[workflow]` configuration.

/// Default crate-job timeout: the shared five-minute job default.
#[must_use]
pub const fn default_crate_timeout_minutes() -> u16 {
    crate::workflow::timeout::JobTimeout::DEFAULT_MINUTES
}

/// Default planner-job timeout: the shared five-minute job default.
#[must_use]
pub const fn default_plan_timeout_minutes() -> u16 {
    crate::workflow::timeout::JobTimeout::DEFAULT_MINUTES
}

/// Serialization predicate keeping the default crate timeout out of emitted config.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
pub(super) fn is_default_crate_timeout(value: &u16) -> bool {
    *value == default_crate_timeout_minutes()
}

/// Serialization predicate keeping the default plan timeout out of emitted config.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
pub(super) fn is_default_plan_timeout(value: &u16) -> bool {
    *value == default_plan_timeout_minutes()
}
