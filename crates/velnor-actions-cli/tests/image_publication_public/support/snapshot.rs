// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Fixture;
use std::error::Error;

impl Fixture {
    pub(crate) fn set_snapshot_fault(&self, fault: &str) -> Result<(), Box<dyn Error>> {
        let mut state = self.read_state()?;
        state["snapshot_fault"] = fault.into();
        self.write_state(&state)
    }
}
