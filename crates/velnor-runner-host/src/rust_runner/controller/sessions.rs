// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::{Duration, Instant};

use velnor_runner_github::QueueSession;

use super::connection::Service;
use super::{Event, Run, blocking, github};
use crate::launch_blocking::ListenFault;

const CLOSE_WAIT: Duration = Duration::from_secs(10);

pub(super) async fn close_all(sessions: Vec<(github::Admin, i64, String)>) {
    let closes = sessions.into_iter().map(|(admin, id, session_id)| {
        blocking(move || github::close_session(&admin, id, &session_id))
    });
    match tokio::time::timeout(CLOSE_WAIT, futures_util::future::join_all(closes)).await {
        Ok(results) => {
            for error in results.into_iter().filter_map(Result::err) {
                eprintln!("rust-runner session close held: {error}");
            }
        }
        Err(_) => eprintln!("rust-runner session close held: not finished within {CLOSE_WAIT:?}"),
    }
}

impl Run<'_> {
    pub(super) async fn close_sessions(&mut self) {
        let sessions = self
            .connections
            .iter()
            .filter(|live| live.service != Service::Closed)
            .map(|live| {
                (
                    live.admin.clone(),
                    live.scale_set_id,
                    live.session.session_id.clone(),
                )
            })
            .collect();
        close_all(sessions).await;
    }

    pub(super) fn spawn_due_renewals(&mut self) {
        let now = Instant::now();
        for (index, live) in self.connections.iter_mut().enumerate() {
            if live.service == Service::Closed || live.refreshing || now < live.admin_due {
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
