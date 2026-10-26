// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepKind};

use crate::RenderError;

pub(crate) const SEED_PARALLEL_TARGETS_OPERATION: &str = "seed-parallel-targets-v1";
pub(crate) const TARGET_ROOT_ENV: &str = "VELNOR_TARGET_ROOT";
pub(crate) const TARGET_LANES_ENV: &str = "VELNOR_TARGET_LANES";
const LANES: [&str; 2] = ["velnor-clippy", "velnor-doc"];

pub fn seed_parallel_targets_step(target: &str, lanes: &[&str]) -> Result<Step, RenderError> {
    let mut canonical_lanes = lanes.to_vec();
    canonical_lanes.sort_unstable();
    let parts: Vec<&str> = target.split('/').collect();
    if parts.last() != Some(&"target")
        || target.starts_with('/')
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        })
        || canonical_lanes.is_empty()
        || canonical_lanes.windows(2).any(|pair| pair[0] == pair[1])
        || canonical_lanes.iter().any(|lane| !LANES.contains(lane))
    {
        return Err(RenderError::BadCommand(
            "parallel_target_seed_arguments".to_owned(),
        ));
    }
    Ok(Step {
        parallel: false,
        name: "Seed independent parallel targets".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Internal {
            operation: SEED_PARALLEL_TARGETS_OPERATION.to_owned(),
            env: BTreeMap::from([
                (TARGET_ROOT_ENV.to_owned(), target.to_owned()),
                (TARGET_LANES_ENV.to_owned(), canonical_lanes.join(",")),
            ]),
        },
    })
}
