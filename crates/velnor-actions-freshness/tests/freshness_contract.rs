//! Public repository-maintenance API contract.

use std::path::Path;

use velnor_actions_freshness::{Operation, run};

#[test]
fn typed_toolchain_operation_reads_the_repository_policy() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(run(&root, &Operation::ToolchainSpecs), 0);
}
