// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use velnor_actions_mise::CheckDeadline;

pub(crate) fn expire_during<T>(
    operation: impl FnOnce() -> T,
) -> Result<T, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let deadline = CheckDeadline::from_start(start, Duration::from_secs(1))?;
    let calls = Rc::new(Cell::new(0));
    let clock_calls = Rc::clone(&calls);
    let mut result = None;
    CheckDeadline::with_clock(
        move || {
            let count = clock_calls.get();
            clock_calls.set(count + 1);
            if count == 0 {
                start
            } else {
                start + Duration::from_secs(2)
            }
        },
        || {
            let scoped = deadline.scope(|| result = Some(operation()));
            assert!(scoped.is_err());
        },
    );
    result.ok_or_else(|| "operation did not enter before deadline".into())
}
