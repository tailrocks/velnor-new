// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::MiseError;
use crate::checks::invalid;
use std::time::{Duration, Instant};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

thread_local! {
    static OPERATION_DEADLINE: Cell<Option<CheckDeadline>> = const { Cell::new(None) };
    static CLOCK_PROVIDER: RefCell<Option<Rc<dyn Fn() -> Instant>>> = const { RefCell::new(None) };
}

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

    pub fn scope<T>(self, operation: impl FnOnce() -> T) -> Result<T, MiseError> {
        let effective = self.effective();
        effective.remaining()?;
        let restore = RestoreDeadline(OPERATION_DEADLINE.replace(Some(effective)));
        let result = operation();
        effective.remaining()?;
        drop(restore);
        Ok(result)
    }

    #[must_use]
    pub(crate) fn effective(self) -> Self {
        OPERATION_DEADLINE
            .get()
            .map_or(self, |parent| Self(self.0.min(parent.0)))
    }

    pub fn check_scope() -> Result<(), MiseError> {
        OPERATION_DEADLINE
            .get()
            .map_or(Ok(()), |deadline| deadline.remaining().map(|_| ()))
    }

    pub fn from_start(start: Instant, timeout: Duration) -> Result<Self, MiseError> {
        start
            .checked_add(timeout)
            .map(Self)
            .ok_or_else(|| invalid("check_timeout", "deadline_overflow"))
    }

    pub fn from_start_with_cleanup_reserve(
        start: Instant,
        budget: Duration,
    ) -> Result<Self, MiseError> {
        let work = budget
            .checked_sub(crate::IsolatedCommand::PROCESS_CLEANUP_GRACE)
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| invalid("check_timeout", "insufficient_cleanup_budget"))?;
        Self::from_start(start, work)
    }

    pub fn after(timeout: Duration) -> Result<Self, MiseError> {
        Self::from_start(now(), timeout)
    }

    pub fn remaining(self) -> Result<Duration, MiseError> {
        self.effective()
            .0
            .checked_duration_since(now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| invalid("check_timeout", "deadline_exhausted"))
    }
}
