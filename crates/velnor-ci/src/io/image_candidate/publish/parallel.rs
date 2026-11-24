// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

use crate::error::CiError;

const WORKERS: usize = 4;

pub(super) fn each<T: Sync>(
    items: &[T],
    work: &(dyn Fn(&T) -> Result<(), CiError> + Sync),
) -> Result<(), CiError> {
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let outcomes: Vec<Result<(), CiError>> = thread::scope(|scope| {
        let workers: Vec<_> = (0..WORKERS.min(items.len()))
            .map(|_| {
                scope.spawn(|| {
                    while !stop.load(Ordering::Relaxed) {
                        let Some(item) = items.get(next.fetch_add(1, Ordering::Relaxed)) else {
                            break;
                        };
                        if let Err(error) = work(item) {
                            stop.store(true, Ordering::Relaxed);
                            return Err(error);
                        }
                    }
                    Ok(())
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|_| Err(CiError::new("A registry worker panicked.")))
            })
            .collect()
    });
    outcomes.into_iter().collect()
}
