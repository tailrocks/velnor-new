//! Transport lane that switches between GitHub administrative and queue origins.

use velnor_runner_github::{
    Exchange, Poll, QueueSession, SessionRequest, Transport, TransportFail,
};

use crate::EnsureError;
use crate::listen::Link;

use super::{Drive, Lane, steps_ack};

pub(super) fn ack_ready(
    link: &mut Link,
    session: &QueueSession,
    path: String,
    queue: Option<String>,
    polled: &Poll,
) -> Result<(), EnsureError> {
    let Poll::Batch(batch) = polled else {
        return Ok(());
    };
    let context = Drive {
        set_id: 0,
        queue_path: path,
        queue_token: session.token().to_owned(),
        admin_token: String::new(),
    };
    let admin = link.base().to_owned();
    let mut lane = HostLane::new(link, admin, queue);
    steps_ack::acknowledge(&mut lane, &context, batch)
}

pub(super) struct HostLane<'a> {
    link: &'a mut Link,
    admin: String,
    queue: Option<String>,
}

impl<'a> HostLane<'a> {
    pub(super) fn new(link: &'a mut Link, admin: String, queue: Option<String>) -> Self {
        Self { link, admin, queue }
    }
}

impl Transport for HostLane<'_> {
    fn exchange(&mut self, request: &SessionRequest) -> Result<Exchange, TransportFail> {
        self.link.transport().exchange(request)
    }
}

impl Lane for HostLane<'_> {
    fn on_admin(&mut self) -> Result<(), EnsureError> {
        let admin = self.admin.clone();
        self.link.set_base(&admin)
    }

    fn on_queue(&mut self) -> Result<(), EnsureError> {
        let Some(origin) = self.queue.clone() else {
            return Ok(());
        };
        self.link.set_base(&origin)
    }
}
