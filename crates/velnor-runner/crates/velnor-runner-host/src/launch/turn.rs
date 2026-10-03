//! One session loop. Capacity 1 returns on the first start.

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::Journal;
use crate::listen::{Link, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::capacity::{self, Admit};
use super::slot;
use super::steps;
use super::trace;
use super::{Ready, ack_ready, drive_ready, scale_session};

/// Poll until this call has started `capacity` workers, or the bound ends.
///
/// Capacity 1 returns after the first start, including a primed empty poll.
/// A larger capacity stays on this session through empty polls.
///
/// # Errors
///
/// Returns [`EnsureError`] when one message carries two ids, or a later step fails.
pub(super) async fn poll_and_drive(
    link: &mut Link,
    set_id: i64,
    session: &QueueSession,
    admin_token: &str,
    journal: &Journal,
    docker: &bollard::Docker,
) -> Result<Vec<Started>, EnsureError> {
    trace::session(session);
    let mut workers = Vec::new();
    if let Some(started) =
        scale_session(link, set_id, session, admin_token, journal, docker).await?
    {
        workers.push(started);
    }
    let capacity = capacity::job_capacity();
    let mut turn = Turn {
        link,
        set_id,
        session,
        admin_token,
        journal,
        docker,
        capacity,
    };
    for _ in 0..capacity::poll_bound(capacity) {
        if turn.drive_poll(&mut workers).await? {
            return Ok(workers);
        }
    }
    Ok(workers)
}

struct Turn<'a> {
    link: &'a mut Link,
    set_id: i64,
    session: &'a QueueSession,
    admin_token: &'a str,
    journal: &'a Journal,
    docker: &'a bollard::Docker,
    capacity: u32,
}

impl Turn<'_> {
    async fn drive_poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        let (saved, path) = point_at_queue(self.link, &self.session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| self.link.base().to_owned());
        let polled = poll_path(self.link, self.session, &path, self.capacity);
        restore_base(self.link, saved)?;
        let polled = polled?;
        trace::batch(&polled);
        let idle = steps::idle(&polled);
        let started = u32::try_from(workers.len()).unwrap_or(u32::MAX);
        let running = self.running(started, idle).await?;
        let decision = capacity::admit(capacity::Seat {
            capacity: self.capacity,
            started,
            running,
            idle,
        });
        self.apply(decision, workers, path, queue, &polled).await
    }

    async fn running(&self, started: u32, idle: steps::Idle) -> Result<u32, EnsureError> {
        if !capacity::needs_running(self.capacity, started, idle) {
            return Ok(0);
        }
        slot::running_count(self.journal, self.docker).await
    }

    async fn apply(
        &mut self,
        decision: Admit,
        workers: &mut Vec<Started>,
        path: String,
        queue: Option<String>,
        polled: &Poll,
    ) -> Result<bool, EnsureError> {
        match decision {
            // HTTP 202 keeps the session open. A job can arrive on a later poll.
            Admit::Stay => Ok(false),
            Admit::Stop | Admit::Hold => Ok(true),
            Admit::Error => Err(EnsureError::Unexpected {
                status: 0,
                step: "queue",
            }),
            Admit::Ack { stop } => {
                ack_ready(self.link, self.session, path, queue, polled)?;
                Ok(stop)
            }
            Admit::Start { stop } => {
                let launched = drive_ready(
                    self.link,
                    Ready {
                        set_id: self.set_id,
                        session: self.session,
                        admin_token: self.admin_token,
                        path,
                        queue,
                        polled,
                    },
                    self.journal,
                    self.docker,
                    self.capacity,
                )
                .await?;
                if let Some(worker) = launched {
                    workers.push(worker);
                }
                Ok(stop)
            }
        }
    }
}
