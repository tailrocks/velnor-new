// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[must_use]
pub const fn default_crate_timeout_minutes() -> u16 {
    crate::workflow::timeout::JobTimeout::DEFAULT_MINUTES
}

#[must_use]
pub const fn default_plan_timeout_minutes() -> u16 {
    crate::workflow::timeout::JobTimeout::DEFAULT_MINUTES
}

#[must_use]
pub const fn default_dependency_seed_timeout_minutes() -> u16 {
    30
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
pub(super) fn is_default_crate_timeout(value: &u16) -> bool {
    *value == default_crate_timeout_minutes()
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
pub(super) fn is_default_plan_timeout(value: &u16) -> bool {
    *value == default_plan_timeout_minutes()
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if requires fn(&u16) -> bool"
)]
pub(super) fn is_default_dependency_seed_timeout(value: &u16) -> bool {
    *value == default_dependency_seed_timeout_minutes()
}
