//! One session loop. A running owned worker keeps the session up.

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::Journal;
use crate::listen::{Link, point_at_queue, poll_path, restore_base};
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::capacity::{self, Admit};
use super::slot;
use super::steps;
use super::trace;
use super::{Ready, Rest, ack_ready, drive_ready, scale_session};

/// Poll until admission stops and no owned launch container is running.
///
/// The target equals capacity unless `VELNOR_ADMIT_TARGET` is higher.
/// Capacity 1 still decides to stop after the first start. The session stays
/// while that container is running, and this does not remove it.
/// A full slot skips the statistics mint so restart does not start a second worker.
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
    rest: Rest<'_>,
) -> Result<Vec<Started>, EnsureError> {
    trace::session(session);
    let mut workers = Vec::new();
    let capacity = capacity::job_capacity();
    slot::release_exited(journal, docker).await?;
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    let occupied = slot::occupied(journal).await?;
    let running = slot::running_count(journal, docker).await?;
    if !capacity::statistics_blocked(occupied, running, capacity, population)
        && let Some(started) =
            scale_session(link, set_id, session, admin_token, journal, docker, rest).await?
    {
        workers.push(started);
    }
    let target = capacity::admit_target(capacity);
    let mut turn = Turn {
        link,
        set_id,
        session,
        admin_token,
        journal,
        docker,
        capacity,
        target,
        owner: rest.owner,
        repo: rest.repo,
        pat: rest.pat,
        cursor: 0,
        steady_retry: None,
    };
    let bound = if target > capacity {
        capacity::poll_bound_wide()
    } else {
        capacity::poll_bound(capacity)
    };
    until_idle(&mut turn, &mut workers, bound).await?;
    Ok(workers)
}

/// One session's poll and running-count source.
trait PollHost {
    /// `Ok(false)` keeps the session. `Ok(true)` is an admission stop.
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError>;

    /// Owned containers still running.
    async fn running(&mut self) -> Result<u32, EnsureError>;
}

impl PollHost for Turn<'_> {
    async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        self.drive_poll(workers).await
    }

    async fn running(&mut self) -> Result<u32, EnsureError> {
        slot::running_count(self.journal, self.docker).await
    }
}

/// Keep polling after admission stops while an owned container is running.
///
/// An empty session stays open past `bound`. `launch_once` deletes the session
/// only after this returns, so eight empty polls must not return.
async fn until_idle(
    turn: &mut Turn<'_>,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    pump(turn, workers, bound).await
}

async fn pump<H: PollHost>(
    host: &mut H,
    workers: &mut Vec<Started>,
    bound: usize,
) -> Result<(), EnsureError> {
    let mut polls = 0usize;
    let mut missed = 0u8;
    loop {
        if polls >= bound && !workers.is_empty() && missed >= 2 && host.running().await? == 0 {
            return Ok(());
        }
        let stop = host.poll(workers).await?;
        polls = polls.saturating_add(1);
        if !stop {
            // The broker can assign a job only while this session still exists.
            if workers.is_empty() {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            continue;
        }
        if host.running().await? > 0 {
            missed = 0;
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }
        missed = missed.saturating_add(1);
        if workers.is_empty() || missed >= 2 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// A cleared job name and a steady scale name stay on the queue.
///
/// Neither error acknowledges the message or ends the session.
fn queue_stays(error: Option<&EnsureError>) -> bool {
    matches!(
        error,
        Some(EnsureError::NameSteady | EnsureError::NameCleared)
    )
}

fn progress_batch(polled: &Poll) -> bool {
    let Poll::Batch(batch) = polled else {
        return false;
    };
    crate::assign::progress_only(batch)
}

fn assigned_in(polled: &Poll) -> u32 {
    let Poll::Batch(batch) = polled else {
        return 0;
    };
    let raw = batch
        .statistics
        .as_ref()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    u32::try_from(raw.max(0)).unwrap_or(u32::MAX)
}

pub(crate) async fn admission<E: crate::stage::PairEngine + ?Sized>(
    engine: &E,
    journal: &Journal,
    capacity: u32,
    target: u32,
    started: u32,
    polled: &Poll,
) -> Result<Admit, EnsureError> {
    slot::release_exited(journal, engine).await?;
    let idle = steps::idle(polled);
    let except = steps::mint_subject(polled);
    let occupied = slot::occupied_except(journal, except.as_deref()).await?;
    let running = if capacity::needs_running(idle) {
        slot::running_count(journal, engine).await?
    } else {
        0
    };
    if idle == steps::Idle::Mint && assigned_runner_live(engine, journal, polled).await? {
        return Ok(Admit::Ack {
            stop: capacity::covered_ack_stops(capacity, target, started),
        });
    }
    Ok(capacity::admit(capacity::Seat {
        capacity,
        target,
        started,
        occupied,
        running,
        assigned: assigned_in(polled),
        idle,
        progress: progress_batch(polled),
    }))
}

async fn assigned_runner_live<E: crate::stage::PairEngine + ?Sized>(
    engine: &E,
    journal: &Journal,
    polled: &Poll,
) -> Result<bool, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(false);
    };
    let subject = format!("m{}", batch.message_id);
    slot::subject_running(journal, engine, &subject).await
}

struct Turn<'a> {
    link: &'a mut Link,
    set_id: i64,
    session: &'a QueueSession,
    admin_token: &'a str,
    journal: &'a Journal,
    docker: &'a bollard::Docker,
    capacity: u32,
    target: u32,
    owner: &'a str,
    repo: &'a str,
    pat: &'a str,
    cursor: i64,
    steady_retry: Option<std::time::Instant>,
}

impl Turn<'_> {
    async fn drive_poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
        let (saved, path) = point_at_queue(self.link, &self.session.message_queue_url)?;
        let queue = saved.as_ref().map(|_| self.link.base().to_owned());
        let now = std::time::Instant::now();
        self.cursor = super::steady::poll_cursor(self.cursor, self.steady_retry, now);
        let polled = poll_path(self.link, self.session, &path, self.capacity, self.cursor);
        restore_base(self.link, saved)?;
        let polled = polled?;
        trace::batch(&polled);
        let started = u32::try_from(workers.len()).unwrap_or(u32::MAX);
        let decision = admission(
            self.docker,
            self.journal,
            self.capacity,
            self.target,
            started,
            &polled,
        )
        .await?;
        self.apply(decision, workers, path, queue, &polled).await
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
            Admit::Stay => self.stay(workers).await,
            Admit::Hold => self.hold().await,
            Admit::Stop => Ok(true),
            Admit::Error => Err(EnsureError::Unexpected {
                status: 0,
                step: "queue",
            }),
            Admit::Ack { stop } => {
                // Fail only an unstarted `m{id}` row. A container row stays.
                super::name_taken::fail_unstarted(self.journal, polled).await?;
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
                        path: path.clone(),
                        queue: queue.clone(),
                        polled,
                    },
                    self.journal,
                    self.docker,
                    self.capacity,
                    Rest {
                        owner: self.owner,
                        repo: self.repo,
                        pat: self.pat,
                    },
                )
                .await;
                if queue_stays(launched.as_ref().err()) {
                    // Skip this message id. Do not delete that runner again and do not ack.
                    let id = super::steady::message_id(polled);
                    if let Some(next) = super::steady::steady_cursor(self.cursor, id) {
                        self.cursor = next;
                        self.steady_retry = Some(std::time::Instant::now() + super::steady::RETRY);
                        return Ok(false);
                    }
                    tokio::time::sleep(super::steady::RETRY).await;
                    return Ok(false);
                }
                if let Err(EnsureError::Conflict) = &launched
                    && super::name_taken::should_ack(steps::idle(polled))
                {
                    // A container row stays. The message is still acknowledged.
                    // A new mint of the same name cannot succeed.
                    super::name_taken::fail_unstarted(self.journal, polled).await?;
                    ack_ready(self.link, self.session, path, queue, polled)?;
                    return Ok(false);
                }
                let Some(worker) = launched? else {
                    return Ok(false);
                };
                workers.push(worker);
                Ok(stop)
            }
        }
    }

    async fn stay(&self, workers: &[Started]) -> Result<bool, EnsureError> {
        if super::steady::pause_empty(self.cursor) {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        if self.target > self.capacity && workers.len() >= self.capacity as usize {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
        Ok(false)
    }

    async fn hold(&self) -> Result<bool, EnsureError> {
        if self.target > self.capacity {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            return Ok(false);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::{PollHost, pump};
    use crate::scale_set::EnsureError;
    use crate::worker::Started;

    struct Fake {
        polls: usize,
    }

    #[expect(
        clippy::unused_async_trait_impl,
        reason = "fake test poll host matches the async trait without I/O or awaiting"
    )]
    impl PollHost for Fake {
        async fn poll(&mut self, workers: &mut Vec<Started>) -> Result<bool, EnsureError> {
            let _ = workers;
            self.polls = self.polls.saturating_add(1);
            Ok(self.polls >= 10)
        }

        async fn running(&mut self) -> Result<u32, EnsureError> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn empty_polls_keep_the_same_session() -> Result<(), String> {
        let mut host = Fake { polls: 0 };
        let mut workers = Vec::new();
        pump(&mut host, &mut workers, 8)
            .await
            .map_err(|err| err.to_string())?;
        // Bound 8 used to return before this poll. `launch_once` deletes only after return.
        assert!(host.polls >= 9, "{}", host.polls);
        Ok(())
    }

    #[test]
    fn cleared_name_keeps_the_session() {
        assert!(super::queue_stays(Some(&EnsureError::NameCleared)));
        assert!(super::queue_stays(Some(&EnsureError::NameSteady)));
        assert!(!super::queue_stays(Some(&EnsureError::Conflict)));
        assert!(!super::queue_stays(None));
    }
}
