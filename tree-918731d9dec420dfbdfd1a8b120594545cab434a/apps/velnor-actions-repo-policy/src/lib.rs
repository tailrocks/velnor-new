//! Repository policy integration tests (SIZE-split host).
//!
//! The `velnor_repo_policy` test binary owns the workspace-wide policy
//! pins (manifests, lints, versions, sizes, registration) that lived in
//! `velnor-actions-cli` before the SIZE split; this library target exists
//! only so the crate has a product target next to its test target.
