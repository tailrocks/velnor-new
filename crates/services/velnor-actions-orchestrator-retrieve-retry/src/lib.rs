//! Bounded per-leg fetch retry for report retrieval.
//!
//! [`retrieve_retry`] runs one download attempt closure up to the
//! admitted attempt bound: retries absorb flakes, while persistent
//! failures skip the leg for the merge to judge.

pub mod retrieve_retry;
