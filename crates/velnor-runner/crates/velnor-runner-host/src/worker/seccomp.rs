//! Runner syscall policy derived from Moby's pinned default seccomp profile.
//!
//! Base: Moby `docker-v29.9.1`, commit
//! `b26fb77d3e7b01f9d21b89539c1ab428f5ca431f`.
//! `moby-default-v29.9.1.json` SHA-256:
//! `785b2429264afba4d594320337cb17f144f3c7d51585f9805eef72e28f4f9334`.
//! `runner-v1.json` is the pinned allowlist with namespace and mount-view
//! syscalls removed from the CAP_SYS_ADMIN rule. Safe `clone` rules apply
//! regardless of capabilities, and `clone3` keeps the ENOSYS fallback.

const RUNNER_PROFILE: &str = include_str!("seccomp/runner-v1.json");

pub(super) fn runner_security_opt() -> String {
    format!("seccomp={RUNNER_PROFILE}")
}

#[cfg(test)]
#[path = "seccomp_tests.rs"]
mod tests;
