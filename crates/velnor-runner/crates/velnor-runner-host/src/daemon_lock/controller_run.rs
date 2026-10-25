use std::sync::atomic::Ordering;

use super::DaemonLock;
use crate::error::HostError;

#[must_use = "the guard must stay alive for the duration of the controller run"]
pub(crate) struct ControllerRunGuard<'a> {
    lock: &'a DaemonLock,
}

impl DaemonLock {
    pub(crate) fn begin_controller_run(&self) -> Result<ControllerRunGuard<'_>, HostError> {
        self.controller_running
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| HostError::Lock)?;
        Ok(ControllerRunGuard { lock: self })
    }
}

impl Drop for ControllerRunGuard<'_> {
    fn drop(&mut self) {
        self.lock.controller_running.store(false, Ordering::Release);
    }
}

#[cfg(test)]
#[path = "controller_run/tests.rs"]
mod tests;
