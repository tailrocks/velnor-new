// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{Step, StepId};

use crate::yaml::Yaml;

pub(crate) fn push_step_id(entries: &mut Vec<(String, Yaml)>, step: &Step) {
    let Some(id) = step.id.map(StepId::as_str) else {
        return;
    };
    push_explicit_step_id(entries, id);
}

pub(crate) fn push_explicit_step_id(entries: &mut Vec<(String, Yaml)>, id: &str) {
    entries.push(("id".to_owned(), Yaml::str(id.to_owned())));
}
