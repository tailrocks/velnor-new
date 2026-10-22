//! Cancellation state for the blocking Docker test server.

use std::{
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(test)]
#[path = "state/tests.rs"]
mod tests;

#[derive(Default)]
pub(super) struct ServerState {
    stopping: AtomicBool,
    active_stream: Mutex<Option<UnixStream>>,
}

impl ServerState {
    pub(super) fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    pub(super) fn request_stop(&self) {
        self.stopping.store(true, Ordering::Release);
        let active_stream = self.lock_active_stream().take();
        if let Some(stream) = active_stream {
            let _shutdown = stream.shutdown(Shutdown::Both);
        }
    }

    pub(super) fn register(&self, stream: &UnixStream) -> std::io::Result<bool> {
        if self.is_stopping() {
            let _shutdown = stream.shutdown(Shutdown::Both);
            return Ok(false);
        }

        let registered_stream = stream.try_clone()?;
        let mut active_stream = self.lock_active_stream();
        if self.is_stopping() {
            drop(active_stream);
            let _shutdown = stream.shutdown(Shutdown::Both);
            return Ok(false);
        }

        let previous_stream = active_stream.replace(registered_stream);
        drop(active_stream);
        drop(previous_stream);
        Ok(true)
    }

    pub(super) fn clear_active(&self) {
        let active_stream = self.lock_active_stream().take();
        drop(active_stream);
    }

    #[cfg(test)]
    pub(super) fn has_active_stream(&self) -> bool {
        self.lock_active_stream().is_some()
    }

    fn lock_active_stream(&self) -> MutexGuard<'_, Option<UnixStream>> {
        self.active_stream
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
