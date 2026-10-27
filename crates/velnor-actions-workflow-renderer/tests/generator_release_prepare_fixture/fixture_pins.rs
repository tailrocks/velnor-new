// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind};

pub(super) fn command_argv() -> Vec<String> {
    vec!["true".to_owned()]
}

pub(super) fn fixture_step() -> Step {
    Step {
        name: "fixture".to_owned(),
        id: None,
        role: None,
        condition: None,
        parallel: false,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::default(),
        },
    }
}
