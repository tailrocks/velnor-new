//! Process a ready queue item after capacity and slot checks.

use velnor_runner_github::{Poll, QueueSession, Transport};
use velnor_runner_host::{listen::Link, scale_set::EnsureError};
use velnor_runner_journal::journal::Journal;
use velnor_runner_launch_slot as slot;

use super::{Drive, DriveOutcome, HostLane, Lane, bind, drive_offer_tracked, steps};

pub(crate) struct Ready<'a> {
    pub(super) set_id: i64,
    pub(super) queue_token: String,
    pub(super) admin_token: &'a str,
    pub(super) path: String,
    pub(super) polled: &'a Poll,
}

pub(crate) async fn drive_ready<T>(
    lane: &mut T,
    ready: Ready<'_>,
    journal: &Journal,
    docker: &bollard::Docker,
    capacity: u32,
) -> Result<DriveOutcome, EnsureError>
where
    T: Transport + Lane,
{
    if slot::busy(journal, docker, capacity).await? {
        return Ok(DriveOutcome::default());
    }
    let ctx = Drive {
        set_id: ready.set_id,
        queue_path: ready.path,
        queue_token: ready.queue_token,
        admin_token: ready.admin_token.to_owned(),
    };
    drive_offer_tracked(lane, &ctx, ready.polled, journal, |volume, jit, bind| {
        let volume = volume.to_owned();
        let payload = jit.to_vec();
        async move { bind::start_bound(docker, &volume, &payload, &bind).await }
    })
    .await
}

pub(crate) fn ack_ready(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    admin_token: &str,
    path: String,
    queue: Option<String>,
    polled: &Poll,
) -> Result<Option<i64>, EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(None);
    };
    let ctx = Drive {
        set_id,
        queue_path: path,
        queue_token: session.token().to_owned(),
        admin_token: admin_token.to_owned(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane {
        link,
        admin,
        queue,
        queue_path: String::new(),
        session: Some(session),
        set_id,
        admin_token,
    };
    steps::acknowledge(&mut lane, &ctx, batch)?;
    Ok(Some(batch.message_id))
}
