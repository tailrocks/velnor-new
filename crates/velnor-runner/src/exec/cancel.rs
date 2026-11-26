// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use tokio::process::Child;
use tokio::sync::watch;

pub(super) const KILL_GRACE: Duration = Duration::from_secs(5);
const GROUP_POLL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub struct JobCancel {
    sender: Arc<watch::Sender<bool>>,
    receiver: watch::Receiver<bool>,
}

impl JobCancel {
    #[must_use]
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self {
            sender: Arc::new(sender),
            receiver,
        }
    }

    pub fn cancel(&self) {
        self.sender.send_replace(true);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    pub async fn cancelled(&self) {
        let mut receiver = self.receiver.clone();
        if receiver.wait_for(|cancelled| *cancelled).await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

impl Default for JobCancel {
    fn default() -> Self {
        Self::new()
    }
}

fn group_of(child: &Child) -> Option<Pid> {
    child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
}

fn signal_group(child: &Child, signal: Signal) {
    if let Some(pid) = group_of(child) {
        let _ = kill_process_group(pid, signal);
    }
}

#[derive(Debug, Default)]
pub(super) struct Groups {
    live: Mutex<Vec<Pid>>,
}

impl Groups {
    pub(super) fn track(&self, child: &Child) {
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.retain(|group| test_kill_process_group(*group).is_ok());
        live.extend(group_of(child));
    }

    pub(super) fn kill_all(&self) {
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        for group in live.drain(..) {
            let _ = kill_process_group(group, Signal::KILL);
        }
    }
}

impl Drop for Groups {
    fn drop(&mut self) {
        self.kill_all();
    }
}

fn group_alive(group: Pid) -> bool {
    test_kill_process_group(group).is_ok()
}

pub(super) async fn terminate(child: &mut Child) {
    let group = group_of(child);
    signal_group(child, Signal::TERM);
    let deadline = tokio::time::Instant::now() + KILL_GRACE;
    let _ = tokio::time::timeout_at(deadline, child.wait()).await;
    if let Some(group) = group {
        while group_alive(group) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(GROUP_POLL).await;
        }
        if group_alive(group) {
            let _ = kill_process_group(group, Signal::KILL);
        }
    }
    let _ = child.wait().await;
}
