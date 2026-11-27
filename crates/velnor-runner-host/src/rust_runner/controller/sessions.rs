// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use velnor_runner_github::QueueSession;

use super::{Event, Run, blocking, github};
use crate::launch_blocking::ListenFault;

impl Run<'_> {
    pub(super) async fn close_sessions(&mut self) {
        for live in &self.connections {
            let admin = live.admin.clone();
            let (id, session_id) = (live.scale_set_id, live.session.session_id.clone());
            if let Err(error) =
                blocking(move || github::close_session(&admin, id, &session_id)).await
            {
                eprintln!("rust-runner session close held: {error}");
            }
        }
    }

    pub(super) fn spawn_due_renewals(&mut self) {
        let now = Instant::now();
        for (index, live) in self.connections.iter_mut().enumerate() {
            if live.refreshing || now < live.admin_due {
                continue;
            }
            live.refreshing = true;
            let (pat, binding) = (live.pat.clone(), live.binding.clone());
            self.tasks.spawn(async move {
                let result = blocking(move || github::renew_admin(&pat, &binding)).await;
                Event::Renewed {
                    connection: index,
                    result,
                }
            });
        }
    }

    pub(super) fn spawn_refresh(&mut self, connection: usize) {
        let Some(live) = self.connections.get_mut(connection) else {
            return;
        };
        live.refreshing = true;
        let (pat, binding, admin) = (live.pat.clone(), live.binding.clone(), live.admin.clone());
        let (id, session_id) = (live.scale_set_id, live.session.session_id.clone());
        self.tasks.spawn(async move {
            let result =
                blocking(move || github::refresh(&pat, &binding, &admin, id, &session_id)).await;
            Event::Refreshed { connection, result }
        });
    }

    pub(super) fn on_refreshed(
        &mut self,
        connection: usize,
        result: Result<(github::Admin, QueueSession), ListenFault>,
    ) -> Result<(), ListenFault> {
        let Some(live) = self.connections.get_mut(connection) else {
            return Ok(());
        };
        if let Err(error) = &result {
            eprintln!("rust-runner session refresh held: {error}");
        }
        live.refreshed(Instant::now(), result.ok())
    }

    pub(super) fn on_renewed(
        &mut self,
        connection: usize,
        result: Result<github::Admin, ListenFault>,
    ) {
        let Some(live) = self.connections.get_mut(connection) else {
            return;
        };
        if let Err(error) = &result {
            eprintln!("rust-runner admin renewal held: {error}");
        }
        live.renewed(Instant::now(), result.ok());
    }
}
