// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use velnor_runner_github::{ParsedBatch, Poll, SourceMetadata};

use super::admission::{Admission, JobExit};
use super::github::{self, Acquisition};
use super::source::LaunchGrant;
use super::{Event, Run, blocking};
use crate::assign::is_active_offer_job;
use crate::launch::steady::RETRY;
use crate::launch_blocking::ListenFault;
use crate::listen::PollPathResult;
use crate::rust_runner::ReservationId;

const CHECK_BUDGET: Duration = Duration::from_secs(60);
const POLL_GAP: Duration = Duration::from_millis(500);

pub(super) struct Batch {
    parsed: ParsedBatch,
    offers: VecDeque<(i64, SourceMetadata)>,
    deferred: bool,
}

impl Batch {
    fn new(parsed: ParsedBatch) -> Self {
        let offers = parsed
            .jobs
            .iter()
            .filter(|job| is_active_offer_job(&parsed, job))
            .filter_map(|job| job.request_id.map(|id| (id, job.source.clone())))
            .collect();
        Self {
            parsed,
            offers,
            deferred: false,
        }
    }
}

impl Run<'_> {
    pub(super) fn spawn_polls(&mut self) {
        let now = Instant::now();
        let capacity = self.config.host.max_jobs;
        for (index, connection) in self.connections.iter_mut().enumerate() {
            if connection.polling
                || connection.refreshing
                || connection.poll_retry_at.is_some_and(|at| now < at)
            {
                continue;
            }
            connection.polling = true;
            connection.poll_retry_at = None;
            let queue = connection.queue.clone();
            let cursor = connection.cursor.for_poll(now);
            self.tasks.spawn(async move {
                let result = blocking(move || github::poll(&queue, capacity, cursor)).await;
                Event::Polled {
                    connection: index,
                    result,
                }
            });
        }
    }

    pub(super) fn on_polled(
        &mut self,
        connection: usize,
        result: Result<PollPathResult, ListenFault>,
    ) {
        let now = Instant::now();
        let Some(live) = self.connections.get_mut(connection) else {
            return;
        };
        match result {
            Err(error) => {
                eprintln!("rust-runner poll held: {error}");
                live.polling = false;
                live.poll_retry_at = Some(now + RETRY);
            }
            Ok(PollPathResult::RefreshRequired) => {
                live.polling = false;
                self.spawn_refresh(connection);
            }
            Ok(PollPathResult::Response(Poll::Empty)) => {
                live.polling = false;
                live.poll_retry_at = Some(now + POLL_GAP);
            }
            Ok(PollPathResult::Response(Poll::Quarantined(quarantined))) => {
                live.cursor.deferred(quarantined.message_id, now);
                live.polling = false;
                live.poll_retry_at = Some(now + POLL_GAP);
            }
            Ok(PollPathResult::Response(Poll::Batch(parsed))) => {
                live.batch = Some(Batch::new(parsed));
                self.advance(connection);
            }
        }
    }

    pub(super) fn advance(&mut self, connection: usize) {
        loop {
            let Some(live) = self.connections.get_mut(connection) else {
                return;
            };
            let Some(batch) = live.batch.as_mut() else {
                return;
            };
            let Some((request_id, source)) = batch.offers.pop_front() else {
                self.finish_batch(connection);
                return;
            };
            if self.taken.contains(&(connection, request_id)) {
                continue;
            }
            if !matches!(self.admission(), Admission::Open { free } if free > 0) {
                self.defer(connection);
                continue;
            }
            self.spawn_check(connection, request_id, source);
            return;
        }
    }

    fn defer(&mut self, connection: usize) {
        if let Some(batch) = self
            .connections
            .get_mut(connection)
            .and_then(|live| live.batch.as_mut())
        {
            batch.deferred = true;
        }
    }

    fn spawn_check(&mut self, connection: usize, request_id: i64, source: SourceMetadata) {
        let Some(live) = self.connections.get(connection) else {
            return;
        };
        let binding = live.binding.clone();
        let policy = live.policy.clone();
        let pat = live.pat.clone();
        let default_branch = live.default_branch.clone();
        self.tasks.spawn(async move {
            let result = blocking(move || {
                Ok(super::source::check_offer(
                    &binding,
                    &policy,
                    &pat,
                    &source,
                    default_branch.as_deref(),
                    Instant::now() + CHECK_BUDGET,
                ))
            })
            .await
            .unwrap_or(Err("check_failed"));
            Event::Checked {
                connection,
                request_id,
                result,
            }
        });
    }

    pub(super) fn on_checked(
        &mut self,
        connection: usize,
        request_id: i64,
        result: Result<LaunchGrant, &'static str>,
    ) {
        let grant = match result {
            Ok(grant) => grant,
            Err(code) => {
                eprintln!("source_admission=blocked reason={code}");
                self.advance(connection);
                return;
            }
        };
        if let Some(live) = self.connections.get_mut(connection) {
            live.default_branch = Some(grant.default_branch().to_owned());
        }
        if !matches!(self.admission(), Admission::Open { free } if free > 0) {
            self.defer(connection);
            self.advance(connection);
            return;
        }
        let Some(reservation) = self.allocate_reservation() else {
            self.defer(connection);
            self.advance(connection);
            return;
        };
        if !self.slots.take(reservation, connection, grant) {
            self.defer(connection);
            self.advance(connection);
            return;
        }
        self.taken.insert((connection, request_id));
        self.spawn_acquire(connection, reservation, request_id);
    }

    fn spawn_acquire(&mut self, connection: usize, reservation: ReservationId, request_id: i64) {
        let Some(live) = self.connections.get(connection) else {
            return;
        };
        let admin = live.admin.clone();
        let queue = live.queue.clone();
        let scale_set_id = live.scale_set_id;
        let runner_name = self.runner_name(connection, reservation);
        self.tasks.spawn(async move {
            let result = blocking(move || {
                github::acquire_and_jit(&admin, &queue, scale_set_id, request_id, &runner_name)
            })
            .await;
            Event::Acquired {
                connection,
                reservation,
                result,
            }
        });
    }

    pub(super) fn on_acquired(
        &mut self,
        connection: usize,
        reservation: ReservationId,
        result: Result<Acquisition, ListenFault>,
    ) {
        match result {
            Ok(Acquisition::Acquired { jit }) if self.stopping => {
                drop(jit);
                self.begin_proof(reservation, JobExit::Stopped);
            }
            Ok(Acquisition::Acquired { jit }) => self.launch(reservation, &jit),
            Ok(Acquisition::NotAcquired) => self.slots.abandon_launch(reservation),
            Err(error) => {
                eprintln!("rust-runner acquire failed for reservation {reservation}: {error}");
                self.defer(connection);
                self.begin_proof(reservation, JobExit::AcquireFailed);
            }
        }
        self.advance(connection);
    }

    fn finish_batch(&mut self, connection: usize) {
        let Some(live) = self.connections.get_mut(connection) else {
            return;
        };
        let Some(batch) = live.batch.take() else {
            return;
        };
        let now = Instant::now();
        let message_id = batch.parsed.message_id;
        if batch.deferred || !velnor_runner_github::may_ack(&batch.parsed, true) {
            live.cursor.deferred(message_id, now);
            live.polling = false;
            live.poll_retry_at = Some(now + POLL_GAP);
            return;
        }
        let queue = live.queue.clone();
        let parsed = batch.parsed;
        self.tasks.spawn(async move {
            let result = blocking(move || github::acknowledge(&queue, &parsed)).await;
            Event::Acked {
                connection,
                message_id,
                result,
            }
        });
    }

    pub(super) fn on_acked(
        &mut self,
        connection: usize,
        message_id: i64,
        result: Result<(), ListenFault>,
    ) {
        let now = Instant::now();
        let Some(live) = self.connections.get_mut(connection) else {
            return;
        };
        match result {
            Ok(()) => live.cursor.processed(message_id),
            Err(error) => {
                eprintln!("rust-runner acknowledge held: {error}");
                live.cursor.deferred(message_id, now);
            }
        }
        live.polling = false;
        live.poll_retry_at = Some(now + POLL_GAP);
    }
}
