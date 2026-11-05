// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_actions_contract::workflow::cache_family::S256;
use velnor_actions_workflow_renderer::declared_task_wrapper_digest;
use velnor_actions_workflow_renderer::state_seal::skeleton::STATE_SKELETON_NAME;

use super::state_producer::{ROOTS, build, text_of};
use super::support::rendered_job_block;

#[test]
fn producer_recipe_covers_every_local_action_the_producer_runs() -> Result<(), Box<dyn Error>> {
    const WRAPPER_PATH: &str = ".github/actions/declared-task-0/action.yml";
    const LOCAL_ACTION: &str = "uses: ./.github/actions/";
    let built = build()?;
    let version = env!("CARGO_PKG_VERSION");
    let wrapper = built
        .files
        .get(WRAPPER_PATH)
        .ok_or("the declared task wrapper was not rendered")?;
    assert_eq!(
        declared_task_wrapper_digest(version, version)?,
        S256::of(wrapper.as_bytes()),
        "the recipe hashes bytes other than the emitted wrapper"
    );
    for (producer_id, _) in ROOTS {
        let block = rendered_job_block(text_of(&built, producer_id), producer_id)?;
        let skeleton = block
            .find(&format!("name: {STATE_SKELETON_NAME}"))
            .ok_or("the producer has no skeleton step")?;
        let after = &block[skeleton..];
        let locals: Vec<&str> = after
            .lines()
            .filter_map(|line| line.trim().strip_prefix(LOCAL_ACTION))
            .map(|rest| rest.split_whitespace().next().unwrap_or(rest))
            .collect();
        assert!(
            locals.contains(&"declared-task-0"),
            "{producer_id}: no lane calls the declared task wrapper"
        );
        for local in locals {
            assert!(
                local.strip_prefix("declared-task-").is_some_and(|id| {
                    !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())
                }),
                "{producer_id}: runs the local action {local} that the recipe does not bind"
            );
        }
    }
    Ok(())
}
