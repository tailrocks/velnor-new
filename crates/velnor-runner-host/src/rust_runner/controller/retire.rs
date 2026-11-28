// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::connection::Service;
use super::{Event, Run, blocking, github};
use crate::config::{ConnectionId, HostConfig};
use crate::launch_blocking::ListenFault;
use crate::readiness_scan::controller_config;

impl Run<'_> {
    pub(super) fn retire_removed(&mut self) {
        if !self.config_changed() {
            return;
        }
        let Some((next, config_sha256)) = controller_config(&self.cx.state) else {
            return;
        };
        let removed = removed_ids(&self.config, &next);
        if removed.is_empty() {
            return;
        }
        for live in &mut self.connections {
            if live.service == Service::Serving && removed.contains(&live.binding.connection_id) {
                live.service = Service::Retired;
            }
        }
        if only_removes(&self.config, &removed, &next) {
            self.config = next;
            self.cx.config_sha256 = config_sha256;
        }
    }

    pub(super) fn restart_due(&self) -> bool {
        let retiring = self
            .connections
            .iter()
            .any(|live| live.service == Service::Retired);
        let stranded = self.connections.iter().any(|live| {
            live.service != Service::Serving
                && self
                    .config
                    .connection(&live.binding.connection_id)
                    .is_some()
        });
        (self.config_changed() || stranded) && !retiring
    }

    pub(super) fn close_retired(&mut self) {
        for (index, live) in self.connections.iter_mut().enumerate() {
            if live.service != Service::Retired
                || live.closing
                || live.batch.is_some()
                || live.refreshing
                || self.slots.held_by(index) > 0
            {
                continue;
            }
            live.closing = true;
            let admin = live.admin.clone();
            let (id, session_id) = (live.scale_set_id, live.session.session_id.clone());
            self.tasks.spawn(async move {
                let result = blocking(move || github::close_session(&admin, id, &session_id)).await;
                Event::Closed {
                    connection: index,
                    result,
                }
            });
        }
    }

    pub(super) fn on_closed(&mut self, connection: usize, result: Result<(), ListenFault>) {
        let Some(live) = self.connections.get_mut(connection) else {
            return;
        };
        live.closing = false;
        match result {
            Ok(()) => live.service = Service::Closed,
            Err(error) => eprintln!("rust-runner session close held: {error}"),
        }
    }
}

fn removed_ids(current: &HostConfig, next: &HostConfig) -> Vec<ConnectionId> {
    current
        .connections()
        .iter()
        .map(|connection| connection.connection_id.clone())
        .filter(|id| next.connection(id).is_none())
        .collect()
}

fn only_removes(current: &HostConfig, removed: &[ConnectionId], next: &HostConfig) -> bool {
    let mut trimmed = current.clone();
    removed
        .iter()
        .all(|id| trimmed.remove_connection(id).is_ok())
        && trimmed == *next
}
