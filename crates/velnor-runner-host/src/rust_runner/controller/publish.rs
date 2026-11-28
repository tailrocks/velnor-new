// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::Run;
use super::admission::Admission;
use super::connection::Service;
use crate::rust_runner::census::{self, Held, connection_marker, marker_absent};

impl Run<'_> {
    pub(super) fn publish_census(&self) {
        let held = self
            .connections
            .iter()
            .enumerate()
            .filter(|(_, live)| live.service != Service::Closed)
            .map(|(index, live)| Held {
                id: live.binding.connection_id.as_str().to_owned(),
                slots: self.slots.held_by(index),
                admitting: self.connection_open(index),
            })
            .collect();
        if let Err(error) = census::publish(&self.cx.state, &self.cx.config_sha256, held) {
            eprintln!("rust-runner census held: {error}");
        }
    }

    pub(super) fn admits(&self, connection: usize) -> bool {
        matches!(self.admission(), Admission::Open { free } if free > 0)
            && self.connection_open(connection)
    }

    fn connection_open(&self, connection: usize) -> bool {
        let Some(live) = self.connections.get(connection) else {
            return false;
        };
        let marker = connection_marker(&self.cx.state, live.binding.connection_id.as_str());
        live.service == Service::Serving
            && matches!(self.admission(), Admission::Open { .. })
            && marker_absent(&marker)
    }
}
