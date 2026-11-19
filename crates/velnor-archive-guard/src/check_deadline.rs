// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::time::{Duration, Instant};

thread_local! {
    static OPERATION_DEADLINE: Cell<Option<CheckDeadline>> = const { Cell::new(None) };
    static CLOCK_PROVIDER: RefCell<Option<Rc<dyn Fn() -> Instant>>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineError(&'static str);

impl fmt::Display for DeadlineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "check_timeout: {}", self.0)
    }
}

impl Error for DeadlineError {}

struct RestoreDeadline(Option<CheckDeadline>);

impl Drop for RestoreDeadline {
    fn drop(&mut self) {
        OPERATION_DEADLINE.set(self.0);
    }
}

struct RestoreClock(Option<Rc<dyn Fn() -> Instant>>);

impl Drop for RestoreClock {
    fn drop(&mut self) {
        CLOCK_PROVIDER.with(|provider| {
            let _ = provider.replace(self.0.take());
        });
    }
}

fn now() -> Instant {
    let provider = CLOCK_PROVIDER.with(|current| current.borrow().clone());
    provider.map_or_else(Instant::now, |clock| clock())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckDeadline(Instant);

impl CheckDeadline {
    #[doc(hidden)]
    pub fn with_clock<T>(
        clock: impl Fn() -> Instant + 'static,
        operation: impl FnOnce() -> T,
    ) -> T {
        let restore =
            RestoreClock(CLOCK_PROVIDER.with(|provider| provider.replace(Some(Rc::new(clock)))));
        let result = operation();
        drop(restore);
        result
    }

    pub fn scope<T>(self, operation: impl FnOnce() -> T) -> Result<T, DeadlineError> {
        let effective = self.effective();
        effective.remaining()?;
        let restore = RestoreDeadline(OPERATION_DEADLINE.replace(Some(effective)));
        let result = operation();
        effective.remaining()?;
        drop(restore);
        Ok(result)
    }

    fn effective(self) -> Self {
        OPERATION_DEADLINE
            .get()
            .map_or(self, |parent| Self(self.0.min(parent.0)))
    }

    pub fn check_scope() -> Result<(), DeadlineError> {
        OPERATION_DEADLINE
            .get()
            .map_or(Ok(()), |deadline| deadline.remaining().map(|_| ()))
    }

    pub fn from_start(start: Instant, timeout: Duration) -> Result<Self, DeadlineError> {
        start
            .checked_add(timeout)
            .map(Self)
            .ok_or(DeadlineError("deadline_overflow"))
    }

    pub fn remaining(self) -> Result<Duration, DeadlineError> {
        self.effective()
            .0
            .checked_duration_since(now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(DeadlineError("deadline_exhausted"))
    }
}
