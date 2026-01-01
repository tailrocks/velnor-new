//! Route launch requests between the admin service and the session queue.

use velnor_runner_github::{
    Exchange, QueueSession, SessionError, SessionRequest, Transport, TransportFail, WireError,
    refresh_queue_request,
};

use velnor_runner_host::listen::{Link, point_at_queue, restore_base};
use velnor_runner_host::scale_set::EnsureError;

use super::Lane;

pub(super) struct HostLane<'a> {
    pub(super) link: &'a mut Link,
    pub(super) admin: String,
    pub(super) queue: Option<String>,
    pub(super) queue_path: String,
    pub(super) session: Option<&'a mut QueueSession>,
    pub(super) set_id: i64,
    pub(super) admin_token: &'a str,
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
        if let Some(session) = self.session.as_deref() {
            let admin = self.admin.clone();
            self.link.set_base(&admin)?;
            let (saved, path) = point_at_queue(self.link, &session.message_queue_url)?;
            self.queue = saved.as_ref().map(|_| self.link.base().to_owned());
            self.queue_path = path;
            return Ok(());
        }
        let Some(origin) = self.queue.clone() else {
            return Ok(());
        };
        self.link.set_base(&origin)
    }

    fn message_queue_path(&self, fallback: &str) -> String {
        if self.queue_path.is_empty() {
            fallback.to_owned()
        } else {
            self.queue_path.clone()
        }
    }

    fn refresh_queue(
        &mut self,
        request: &mut SessionRequest,
        ack_suffix: Option<&str>,
    ) -> Result<(), SessionError> {
        self.link
            .set_base(&self.admin)
            .map_err(|_| SessionError::Wire(WireError::Malformed))?;
        let Some(session) = self.session.as_deref_mut() else {
            return Err(SessionError::Wire(WireError::Malformed));
        };
        let refreshed_url = refresh_queue_request(
            self.link.transport(),
            self.set_id,
            session,
            self.admin_token,
            request,
        )?
        .to_owned();
        let (saved, path) = point_at_queue(self.link, &refreshed_url)
            .map_err(|_| SessionError::Wire(WireError::Malformed))?;
        self.queue_path = path;
        self.queue = saved.as_ref().map(|_| self.link.base().to_owned());
        request.path = replay_path(&self.queue_path, &request.path, ack_suffix);
        if ack_suffix.is_none() {
            restore_base(self.link, saved).map_err(|_| SessionError::Wire(WireError::Malformed))?;
        }
        Ok(())
    }
}

fn replay_path(queue_path: &str, original_path: &str, ack_suffix: Option<&str>) -> String {
    match ack_suffix {
        Some(suffix) => format!("{}/{suffix}", queue_path.trim_end_matches('/')),
        None => original_path.to_owned(),
    }
}

#[cfg(test)]
mod tests;
