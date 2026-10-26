// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0


use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

use crate::{RenderError, steps};

pub fn action_step(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    action_step_with_env(name, uses, with, BTreeMap::new())
}

pub fn action_step_with_env(
    name: &str,
    uses: &str,
    with: BTreeMap<String, String>,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if name.trim().is_empty() {
        return Err(RenderError::BadActionRef("empty_name".to_owned()));
    }
    crate::expressions::check_name_content(name)?;
    steps::validate_uses(uses)?;
    steps::scan_for_private_subcommands(name)?;
    steps::scan_for_private_subcommands(uses)?;
    for (key, value) in &with {
        crate::expressions::check_with_key(key)?;
        if !crate::tool_seed::is_guarded_seed_key_input(uses, &with, key, value) {
            crate::expressions::check_with_value(key, value)?;
        }
        steps::scan_for_private_subcommands(key)?;
        steps::scan_for_private_subcommands(value)?;
    }
    crate::commands::validate_env(&env)?;
    Ok(Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
            env,
        },
    })
}
