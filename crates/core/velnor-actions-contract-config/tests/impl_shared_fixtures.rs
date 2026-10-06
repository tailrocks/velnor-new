//! Fixture vocabulary shared by this target's config cases.
//!
//! Duplicated per contract-family test target at the SIZE split (sibling
//! copies in the `contract`, `contract-workflow`, and
//! `contract-planning` test targets): each integration target links
//! alone, so shared helpers ride per target. Keep the copies in sync;
//! drift fails the agreement pins.

use std::collections::BTreeMap;

use velnor_actions_contract::{ContractError, digest_b3};
use velnor_actions_contract_workflow::workflow::{ExecuteTaskIds, ExecuteTaskRef, MatrixEntry};

/// Sample Cargo manifest path shared by contract cases.
pub(crate) const MANIFEST: &str = "crates/core/velnor-actions-contract/Cargo.toml";
/// Sample task ID shared by contract cases.
pub(crate) const TASK: &str = "stack/rust/crates/core/velnor-actions-contract/clippy/default";
/// Sample task-group ID shared by contract cases.
pub(crate) const GROUP: &str = "stack/rust/crates/core/velnor-actions-contract/validation/default";

/// Fixed leg command shared by contract matrix fixtures.
pub(crate) const SAMPLE_RUN: &str = "mise exec --no-config rust@1.98.1 -- cargo clippy --locked";

/// Build one sample matrix entry shared by contract cases.
pub(crate) fn sample_entry(run_key: &str) -> Result<MatrixEntry, ContractError> {
    let mut tasks = BTreeMap::new();
    tasks.insert("clippy".to_owned(), ExecuteTaskRef::Single(TASK.to_owned()));
    MatrixEntry::derive(
        "rust",
        GROUP,
        SAMPLE_RUN,
        &digest_b3(b"task-bytes"),
        serde_json::json!({"manifest": MANIFEST}),
        ExecuteTaskIds { tasks },
        &digest_b3(b"entry-inputs"),
        run_key,
        "plan",
    )
}
