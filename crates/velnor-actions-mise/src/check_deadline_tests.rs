// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::CheckDeadline;
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

#[test]
fn cleanup_split_keeps_the_original_endpoint_while_the_clock_advances()
-> Result<(), crate::MiseError> {
    let start = Instant::now();
    let requested = CheckDeadline::from_start(start, Duration::from_secs(4))?;
    let parent = CheckDeadline::from_start(start, Duration::from_secs(2))?;
    let reserve = Duration::from_millis(250);
    let clock = Rc::new(Cell::new(start));
    CheckDeadline::with_clock(
        move || {
            let current = clock.get();
            clock.set(current + Duration::from_millis(10));
            current
        },
        || {
            let direct = requested.split_cleanup(reserve)?;
            let scoped = parent.scope(|| requested.split_cleanup(reserve))??;
            for ((work, cleanup), expected) in [(direct, requested.0), (scoped, parent.0)] {
                assert_eq!(
                    work.0.checked_add(reserve),
                    Some(expected),
                    "the work deadline must keep the caller's full cleanup reserve"
                );
                assert_eq!(
                    cleanup, expected,
                    "cleanup must end at the original effective deadline"
                );
            }
            Ok(())
        },
    )
}
