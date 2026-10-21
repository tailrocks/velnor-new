use crate::{config::RepositoryConnection, controller_ipc::ControlState, error::HostError};

pub(super) struct PollAttemptGuard<'a> {
    control: &'a ControlState,
    connection_id: String,
    active: bool,
}

impl<'a> PollAttemptGuard<'a> {
    pub(super) fn begin(
        control: &'a ControlState,
        binding: &RepositoryConnection,
        scale_set_id: i64,
        session_intent_id: i64,
        session_id: &str,
    ) -> Result<Self, HostError> {
        let deadline = std::time::Instant::now()
            .checked_add(std::time::Duration::from_secs(60))
            .ok_or(HostError::Endpoint)?;
        control.begin_session_poll(
            binding,
            scale_set_id,
            session_intent_id,
            session_id,
            deadline,
        )?;
        Ok(Self {
            control,
            connection_id: binding.connection_id.as_str().to_owned(),
            active: true,
        })
    }

    pub(super) fn complete(mut self) {
        self.active = false;
    }
}

impl Drop for PollAttemptGuard<'_> {
    fn drop(&mut self) {
        if self.active
            && let Err(error) = self.control.invalidate_session_poll(&self.connection_id)
        {
            eprintln!(
                "The controller could not invalidate the poll receipt for connection {}. Error: {error}.",
                self.connection_id
            );
        }
    }
}
