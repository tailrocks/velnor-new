//! Production adapter for queue acknowledgements and worker launches.

use velnor_runner_github::{Poll, QueueSession};

use crate::journal::{Journal, LaunchReservation};
use crate::listen::Link;
use crate::scale_set::EnsureError;
use crate::worker::Started;

use super::super::{Ready, ack_ready, drive_ready};
use super::poll::{DispatchFuture, PollDispatcher};

pub(super) struct LinkDispatcher<'a> {
    pub(super) link: &'a mut Link,
    pub(super) set_id: i64,
    pub(super) session: &'a QueueSession,
    pub(super) admin_token: &'a str,
    pub(super) docker: &'a bollard::Docker,
}

impl PollDispatcher for LinkDispatcher<'_> {
    fn ack(
        &mut self,
        path: String,
        queue: Option<String>,
        polled: &Poll,
    ) -> Result<(), EnsureError> {
        ack_ready(self.link, self.session, path, queue, polled)
    }

    fn start<'a>(
        &'a mut self,
        journal: &'a Journal,
        path: String,
        queue: Option<String>,
        polled: &'a Poll,
        reservation: Option<LaunchReservation>,
    ) -> DispatchFuture<'a> {
        Box::pin(start(
            self.link,
            self.set_id,
            self.session,
            self.admin_token,
            journal,
            self.docker,
            path,
            queue,
            polled,
            reservation,
        ))
    }
}

async fn start(
    link: &mut Link,
    set_id: i64,
    session: &QueueSession,
    admin_token: &str,
    journal: &Journal,
    docker: &bollard::Docker,
    path: String,
    queue: Option<String>,
    polled: &Poll,
    reservation: Option<LaunchReservation>,
) -> Result<Option<Started>, EnsureError> {
    drive_ready(
        link,
        Ready {
            set_id,
            session,
            admin_token,
            path,
            queue,
            polled,
        },
        journal,
        docker,
        reservation,
    )
    .await
}
