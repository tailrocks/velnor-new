// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io,
    sync::{Condvar, Mutex},
};

use velnor_actions_mise::CheckDeadline;

#[derive(Default)]
struct State {
    started: bool,
}

#[derive(Default)]
pub(crate) struct TokenGate {
    state: Mutex<State>,
    changed: Condvar,
}

impl TokenGate {
    pub(crate) fn wait_started(&self, deadline: CheckDeadline) -> io::Result<bool> {
        let mut state = self.state.lock().map_err(lock_error)?;
        while !state.started {
            let (next, timeout) = self
                .changed
                .wait_timeout(state, deadline.remaining().map_err(io::Error::other)?)
                .map_err(lock_error)?;
            state = next;
            if timeout.timed_out() && !state.started {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(super) fn wait_until(&self, deadline: CheckDeadline) -> io::Result<()> {
        let mut state = self.state.lock().map_err(lock_error)?;
        state.started = true;
        self.changed.notify_all();
        loop {
            let (next, timeout) = self
                .changed
                .wait_timeout(state, deadline.remaining().map_err(io::Error::other)?)
                .map_err(lock_error)?;
            state = next;
            if timeout.timed_out() {
                return Err(io::ErrorKind::TimedOut.into());
            }
        }
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> io::Error {
    io::Error::other("token gate lock is poisoned")
}
