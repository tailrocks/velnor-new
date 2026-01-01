//! External-data freshness for skippable checks.
//!
//! [`external_data`] marks advisory-backed checks, validates the
//! external-data identity and freshness a baseline proof carries,
//! and decides when a covered obligation may skip re-execution.

pub mod external_data;
