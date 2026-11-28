// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process_group, test_kill_process_group};
use tokio::process::Child;
use tokio::sync::watch;

use super::tracking::Tracking;

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

pub(super) fn group_of(child: &Child) -> Option<Pid> {
    child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .and_then(Pid::from_raw)
}

#[derive(Debug)]
pub(super) struct Groups {
    live: Mutex<Live>,
}

#[derive(Debug)]
struct Live {
    groups: Vec<Pid>,
    tracking: Option<Tracking>,
}

impl Groups {
    pub(super) fn new(tracking: Option<Tracking>) -> Self {
        Self {
            live: Mutex::new(Live {
                groups: Vec::new(),
                tracking,
            }),
        }
    }

    pub(super) fn track(&self, child: &Child) {
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.groups
            .retain(|group| test_kill_process_group(*group).is_ok());
        live.groups.extend(group_of(child));
    }

    fn take(&self) -> (Vec<Pid>, Option<Tracking>) {
        let mut live = self.live.lock().unwrap_or_else(PoisonError::into_inner);
        (std::mem::take(&mut live.groups), live.tracking.take())
    }

    pub(super) async fn finish(&self) {
        let (groups, tracking) = self.take();
        let Some(tracking) = tracking else {
            return;
        };
        kill_groups(&groups);
        let _ = tokio::task::spawn_blocking(move || tracking.reap()).await;
    }
}

fn kill_groups(groups: &[Pid]) {
    for group in groups {
        let _ = kill_process_group(*group, Signal::KILL);
    }
}

impl Drop for Groups {
    fn drop(&mut self) {
        let (groups, tracking) = self.take();
        kill_groups(&groups);
        if let Some(tracking) = tracking {
            tracking.reap();
        }
    }
}

pub(super) struct GroupGuard(Option<Pid>);

impl GroupGuard {
    pub(super) fn arm(group: Option<Pid>) -> Self {
        Self(group)
    }

    pub(super) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            let _ = kill_process_group(group, Signal::KILL);
        }
    }
}

fn group_alive(group: Pid) -> bool {
    test_kill_process_group(group).is_ok()
}

pub(super) async fn terminate(child: &mut Child, group: Option<Pid>) {
    if let Some(group) = group {
        let _ = kill_process_group(group, Signal::TERM);
    }
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
