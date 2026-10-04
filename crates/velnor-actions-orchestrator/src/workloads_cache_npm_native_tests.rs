//! Opt-in executable proof uses the same pinned native owner as generated jobs.

use std::{env, process::Command};

#[test]
#[ignore = "requires VELNOR_NPM_TEST_NODE_BIN pointing to the catalog-selected native owner"]
fn pinned_native_producer_repairs_content_and_feeds_real_npm_ci() {
    let node = env::var("VELNOR_NPM_TEST_NODE_BIN").expect("pinned Node executable required");
    let owner = serde_json::to_string(&super::owner_expectation(
        &velnor_actions_mise::ToolCatalog::pinned(),
    ))
    .expect("catalog owner expectation");
    let fixture = include_str!("workloads_cache_npm_native_fixture.py");
    let result = Command::new("python3")
        .args(["-c", fixture, &super::producer_code(), &node, &owner])
        .status()
        .expect("pinned native producer fixture");
    assert!(result.success());
}
