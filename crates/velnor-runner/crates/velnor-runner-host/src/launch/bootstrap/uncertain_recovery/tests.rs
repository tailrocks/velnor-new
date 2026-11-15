// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use super::recover_in_rotation;

const BUDGET: Duration = Duration::from_secs(30);
const INTERRUPTED: [i64; 4] = [10, 20, 30, 40];

async fn visited(ids: &[i64], pass: usize, work: Duration) -> Vec<i64> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    recover_in_rotation(ids, pass, BUDGET, move |id, _| {
        let record = Rc::clone(&record);
        async move {
            record.borrow_mut().push(id);
            tokio::time::sleep(work).await;
        }
    })
    .await;
    seen.take()
}

#[tokio::test(start_paused = true)]
async fn a_budget_that_fits_one_initializer_serves_each_in_turn() {
    let mut leaders = Vec::new();
    for pass in 0..2 * INTERRUPTED.len() {
        let served = visited(&INTERRUPTED, pass, BUDGET).await;
        assert_eq!(served.len(), 1, "pass {pass} served {served:?}");
        leaders.push(served[0]);
    }
    let expected: Vec<i64> = INTERRUPTED.iter().copied().cycle().take(8).collect();
    assert_eq!(leaders, expected);
}

#[tokio::test(start_paused = true)]
async fn every_initializer_leads_within_one_cycle_of_passes() {
    let mut leaders = Vec::new();
    for pass in 5..5 + INTERRUPTED.len() {
        leaders.push(visited(&INTERRUPTED, pass, BUDGET).await[0]);
    }
    leaders.sort_unstable();
    assert_eq!(leaders, INTERRUPTED);
}

#[tokio::test(start_paused = true)]
async fn a_full_budget_visits_each_initializer_once_from_the_rotated_start() {
    assert_eq!(
        visited(&INTERRUPTED, 1, Duration::ZERO).await,
        [20, 30, 40, 10]
    );
    assert_eq!(
        visited(&INTERRUPTED, 7, Duration::ZERO).await,
        [40, 10, 20, 30]
    );
}

#[tokio::test(start_paused = true)]
async fn no_initializers_is_a_no_op_on_any_pass() {
    assert_eq!(visited(&[], 3, Duration::ZERO).await, Vec::<i64>::new());
    assert_eq!(visited(&[7], 9, Duration::ZERO).await, [7]);
}
