// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Opt-in phase timing for `velnor-ci inputs build`, written to stderr only
//! when `VELNOR_AUDIT_TIMING=1`. It never changes any audit decision.

use std::cell::Cell;
use std::time::Instant;

const VARIABLE: &str = "VELNOR_AUDIT_TIMING";

thread_local! {
    static PARSES: Cell<usize> = const { Cell::new(0) };
}

fn enabled() -> bool {
    std::env::var_os(VARIABLE).is_some_and(|value| value == "1")
}

pub(super) fn phase<T>(name: &str, work: impl FnOnce() -> T) -> T {
    if !enabled() {
        return work();
    }
    let started = Instant::now();
    let result = work();
    eprintln!(
        "velnor-ci audit timing: {name} {:.3}s",
        started.elapsed().as_secs_f64()
    );
    result
}

pub(super) fn parsed() {
    PARSES.with(|count| count.set(count.get() + 1));
}

pub(super) fn parses() -> usize {
    PARSES.with(Cell::get)
}

pub(super) fn report() {
    if enabled() {
        eprintln!("velnor-ci audit timing: inventory parses {}", parses());
    }
}
