// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use crate::yaml_tree::document;

const STEP: &str = "No doctests (rustdoc listing)";

#[test]
fn the_rustdoc_listing_step_of_this_repository_is_skipped_on_a_push_the_merge_group_verified()
-> Result<(), Box<dyn Error>> {
    let workflow = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
    let tree = document(&fs::read_to_string(workflow)?)?;
    let listing: Vec<&serde_json::Value> = tree["jobs"]
        .as_object()
        .into_iter()
        .flatten()
        .flat_map(|(_, job)| job["steps"].as_array().into_iter().flatten())
        .filter(|step| step["name"] == STEP)
        .collect();
    assert!(!listing.is_empty(), "no job runs the rustdoc listing step");
    for step in listing {
        let condition = step["if"].as_str().unwrap_or_default();
        assert!(
            condition.contains("needs.rust-plan.outputs.skip_push != 'true'")
                && condition.contains("steps.replan.outcome == 'success'"),
            "the listing step runs on a push the merge group verified: {step}"
        );
    }
    Ok(())
}
