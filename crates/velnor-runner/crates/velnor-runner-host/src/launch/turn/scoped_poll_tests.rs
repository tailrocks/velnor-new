//! Scoped polling regressions.

#[path = "scoped_poll_tests/deferred_mode.rs"]
mod deferred_mode;
#[path = "scoped_poll_tests/lifecycle.rs"]
mod lifecycle;
#[path = "scoped_poll_tests/support.rs"]
#[cfg(test)]
pub(crate) mod support;
#[path = "scoped_poll_tests/zero_header.rs"]
mod zero_header;
