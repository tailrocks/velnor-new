// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Run;
use super::admission::Admission;
use crate::rust_runner::census::{self, connection_marker};

impl Run<'_> {
    pub(super) fn publish_census(&self) {
        let held = self
            .connections
            .iter()
            .enumerate()
            .map(|(index, live)| {
                (
                    live.binding.connection_id.as_str().to_owned(),
                    self.slots.held_by(index),
                )
            })
            .collect();
        if let Err(error) = census::publish(&self.cx.state, &self.cx.config_sha256, held) {
            eprintln!("rust-runner census held: {error}");
        }
    }

    pub(super) fn admits(&self, connection: usize) -> bool {
        let Some(live) = self.connections.get(connection) else {
            return false;
        };
        let marker = connection_marker(&self.cx.state, live.binding.connection_id.as_str());
        matches!(self.admission(), Admission::Open { free } if free > 0)
            && std::fs::symlink_metadata(&marker).is_err()
    }
}
