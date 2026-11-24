// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Child, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use signal_hook::iterator::{Handle, Signals};

use super::early_list::EarlyList;

pub(super) fn forward(
    mut signals: Signals,
    pid: Option<Pid>,
    own_group: bool,
    early: Option<Arc<EarlyList>>,
    exited: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        for received in signals.forever() {
            if let Some(early) = &early {
                early.stop("velnor-ci received a signal");
            }
            if exited.load(Ordering::SeqCst) {
                continue;
            }
            if let (Some(pid), Ok(signal)) = (pid, Signal::try_from(received)) {
                if own_group {
                    killpg(pid, signal).ok();
                } else {
                    kill(pid, signal).ok();
                }
            }
        }
    });
}

pub(super) fn wait_for_exit(
    child: &mut Child,
    exited: &AtomicBool,
    close: Option<&Handle>,
) -> (std::io::Result<ExitStatus>, Instant) {
    let status = child.wait();
    exited.store(true, Ordering::SeqCst);
    if let Some(handle) = close {
        handle.close();
    }
    (status, Instant::now())
}
