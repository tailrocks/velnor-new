use std::process::Child;
use std::sync::{Arc, Mutex, mpsc::Receiver};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use tokio::sync::OwnedSemaphorePermit;
use zeroize::Zeroize;

use super::super::readers::BodyReadError;
use super::{ChildState, poll_child};

pub(super) struct ReapResources {
    pub(super) child: Child,
    pub(super) body_rx: Receiver<Result<Vec<u8>, BodyReadError>>,
    pub(super) status_rx: Receiver<Result<Vec<u8>, BodyReadError>>,
    pub(super) config_thread: Option<JoinHandle<()>>,
    pub(super) body_thread: Option<JoinHandle<()>>,
    pub(super) status_thread: Option<JoinHandle<()>>,
    pub(super) _permit: Option<Arc<OwnedSemaphorePermit>>,
}

impl ReapResources {
    pub(super) fn reap_until_quiescent(mut self) {
        loop {
            let child_state = poll_child(Some(&mut self.child));
            let child_done = child_state == ChildState::Reaped;
            let readers_done = self.threads_finished();
            if child_done && readers_done {
                self.join_finished_threads();
                self.zeroize_pending_output();
                return;
            }
            self.kill_owned_child();
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn threads_finished(&self) -> bool {
        self.config_thread
            .as_ref()
            .is_none_or(JoinHandle::is_finished)
            && self
                .body_thread
                .as_ref()
                .is_none_or(JoinHandle::is_finished)
            && self
                .status_thread
                .as_ref()
                .is_none_or(JoinHandle::is_finished)
    }

    fn join_finished_threads(&mut self) {
        join_finished(&mut self.config_thread);
        join_finished(&mut self.body_thread);
        join_finished(&mut self.status_thread);
    }

    fn zeroize_pending_output(&self) {
        if let Ok(Ok(mut body)) = self.body_rx.try_recv() {
            body.zeroize();
        }
        if let Ok(Ok(mut status)) = self.status_rx.try_recv() {
            status.zeroize();
        }
    }

    fn kill_owned_child(&mut self) {
        let child_state = poll_child(Some(&mut self.child));
        if child_state != ChildState::Running {
            return;
        }
        let _killed = self.child.kill();
    }
}

pub(super) fn defer_reap(resources: ReapResources) {
    let shared = Arc::new(Mutex::new(Some(resources)));
    let thread_shared = Arc::clone(&shared);
    let spawned = thread::Builder::new()
        .name("velnor-curl-reaper".to_owned())
        .spawn(move || {
            let resources = thread_shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(resources) = resources {
                resources.reap_until_quiescent();
            }
        });
    if spawned.is_err() {
        // Keep the exact child, pipe readers, and worker permit quarantined if
        // the bounded worker pool cannot create its cleanup owner.
        let _quarantined = Arc::into_raw(shared);
    }
}

fn join_finished(thread: &mut Option<JoinHandle<()>>) {
    if thread.as_ref().is_some_and(JoinHandle::is_finished)
        && let Some(thread) = thread.take()
    {
        let _joined = thread.join();
    }
}
