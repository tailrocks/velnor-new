//! Acquire jobs up to capacity, mint JIT, and start one paired worker each.
//!
//! The journal row is committed before acquire, JIT, and Docker. JIT bytes are
//! passed to the starter and are not written to the journal.

use velnor_runner_github::QueueSession;

use velnor_runner_host::BoundedDiscoveryTransport;
use velnor_runner_host::ensure_product_scale_set;
use velnor_runner_host::listen::{Link, Secret, admin_link};
use velnor_runner_host::scale_set::EnsureError;
use velnor_runner_host::worker::Started;
use velnor_runner_journal::journal::Journal;
use velnor_runner_journal::reconcile::Reconcile;

mod bind;
mod capacity;
pub mod control;
mod docker_stub;
pub mod drain_observer;
mod drive;
mod fakes;
mod gate;
pub mod harness;
mod host_lane;
mod inspect;
mod offer;
mod ready;

mod mint_origin;
mod session;
mod steps;
mod trace;
mod turn;

pub(crate) use capacity::{install_job_capacity, job_capacity};

pub use capacity::Admit;

use host_lane::HostLane;

pub(crate) use drive::{Drive, Lane};
#[cfg(test)]
pub(crate) use offer::drive_offer;
pub(crate) use offer::{DriveOutcome, drive_offer_tracked};
pub(crate) use ready::{Ready, ack_ready, drive_ready};

#[cfg(test)]
pub(crate) use capacity::{
    Seat, admit, needs_running, parse_admit_target, poll_limit, statistics_blocked, wide_poll_limit,
};
#[cfg(test)]
pub(crate) use steps::{Idle, idle};
pub use turn::admission;

/// What one launch attempt started. No JIT and no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchReport {
    /// Scale-set id.
    pub set_id: i64,
    /// First worker started in this call. Absent when the queue had no job.
    pub started: Option<Started>,
    /// Every worker started in this call, in start order. No JIT and no token.
    pub workers: Vec<Started>,
}

/// Run one session; unresolved polls and capacity holds preserve its delivery row.
///
/// # Errors
///
/// Returns [`EnsureError`] on failure or when an unacknowledged hold fences session exit.
pub async fn launch_once(
    pat: &str,
    owner: &str,
    repo: &str,
    docker: &bollard::Docker,
    journal: &Journal,
) -> Result<LaunchReport, EnsureError> {
    session::require_no_unresolved(journal).await?;
    let ceiling = job_capacity();
    let capacity = velnor_runner_host::guest::discover_guest_capacity(docker, ceiling)
        .await
        .map_err(|_| EnsureError::Unexpected {
            status: 0,
            step: "docker budget",
        })?;
    let _capacity = install_job_capacity(capacity);
    let set = ensure_product_scale_set(pat, owner, repo)?;
    if std::env::var("VELNOR_RECONCILE").ok().as_deref() == Some("1") {
        let decision = gate::reconcile_gate(journal, docker).await?;
        eprintln!("reconcile={}", gate::gate_line(&decision));
        if let Reconcile::Hold { .. } = decision {
            return Ok(LaunchReport {
                set_id: set.id,
                started: None,
                workers: Vec::new(),
            });
        }
    }
    let mut link = admin_link(pat, owner, repo)?;
    let admin = Secret::new(link.token());
    let (mut session, row) =
        session::open_session(&mut link, set.id, admin.expose(), journal).await?;
    // Actions REST reconciliation is read-only and uses the configured host
    // credential in its distinct Actions:read role. It runs only for actual
    // Started/Completed identities observed by this same session owner.
    let mut actions_transport = BoundedDiscoveryTransport::new();
    let mut actions_reconciler =
        turn::ActionsReconciler::new(owner, repo, pat, &mut actions_transport);
    let driven = turn::poll_and_drive(
        &mut link,
        set.id,
        &mut session,
        admin.expose(),
        &mut actions_reconciler,
        journal,
        docker,
    )
    .await;
    let retain_session = driven.as_ref().is_ok_and(|outcome| outcome.retain_session);
    let closed = session::close_after_poll(&driven, retain_session, || async {
        session::close_session(
            &mut link,
            set.id,
            &session.session_id,
            row,
            admin.expose(),
            journal,
        )
        .await
    })
    .await;
    report(set.id, driven, closed)
}

fn report(
    set_id: i64,
    driven: Result<turn::PollOutcome, EnsureError>,
    closed: Result<(), EnsureError>,
) -> Result<LaunchReport, EnsureError> {
    match (driven, closed) {
        (Ok(outcome), Ok(())) if outcome.retain_session => Err(EnsureError::Uncertain),
        (Ok(outcome), Ok(())) => {
            let workers = outcome.workers;
            let started = workers.first().cloned();
            Ok(LaunchReport {
                set_id,
                started,
                workers,
            })
        }
        (_, Err(error)) | (Err(error), Ok(())) => Err(error),
    }
}

async fn scale_session(
    link: &mut Link,
    set_id: i64,
    session: &QueueSession,
    admin_token: &str,
    journal: &Journal,
    docker: &bollard::Docker,
) -> Result<Option<Started>, EnsureError> {
    let population = session
        .statistics()
        .map_or(0, velnor_runner_github::Statistics::assigned_population);
    if population <= 0 {
        return Ok(None);
    }
    let ctx = Drive {
        set_id,
        queue_path: String::new(),
        queue_token: session.token().to_owned(),
        admin_token: admin_token.to_owned(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane {
        link,
        admin,
        queue: None,
        queue_path: String::new(),
        session: None,
        set_id,
        admin_token,
    };
    let name = runner_name(&session.session_id);
    steps::scale_unacked(&mut lane, &ctx, journal, &name, |volume, jit, bind| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { bind::start_bound(docker, &volume, &payload, &bind).await }
    })
    .await
}

fn runner_name(session_id: &str) -> String {
    let mut name = String::from("s");
    for ch in session_id.chars().filter(char::is_ascii_alphanumeric) {
        name.push(ch);
        if name.len() == 13 {
            break;
        }
    }
    if name.len() == 1 {
        name.push('0');
    }
    name
}

#[cfg(test)]
mod tests;
