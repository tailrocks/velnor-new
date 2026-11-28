// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Budget, GenKey, GenScope, Generation, Timestamp, prune_plan};

use crate::gen_support::{DAY, NOW, generation, key, main, mg, pr, rust};

const HOUR: u64 = 3600;

fn doomed(all: &[Generation], budget: &Budget) -> Vec<String> {
    prune_plan(Timestamp(NOW), all, budget)
        .iter()
        .map(|g| g.scope.path())
        .collect()
}

fn unlimited() -> Budget {
    Budget {
        high_bytes: u64::MAX,
        low_bytes: 0,
    }
}

fn sized(key: &GenKey, scope: GenScope, published: u64, bytes: u64) -> Generation {
    Generation {
        bytes,
        ..generation(key, scope, "L", published)
    }
}

fn born(key: &GenKey, scope: GenScope, born: u64, published: u64) -> Generation {
    Generation {
        born: Timestamp(born),
        ..generation(key, scope, "L", published)
    }
}

#[test]
fn main_keeps_the_newest_eight_per_key() {
    let k = rust();
    let all: Vec<Generation> = (1..=10)
        .map(|n| generation(&k, main(n), "L", NOW - DAY + u64::from(n) * 100))
        .collect();
    let paths: Vec<String> = [1, 2].map(|n| main(n).path()).to_vec();
    assert_eq!(doomed(&all, &unlimited()), paths);
}

#[test]
fn main_count_is_per_key() {
    let (a, b) = (rust(), key("lint", "root", "0123456789abcdef"));
    let all: Vec<Generation> = (1..=8)
        .flat_map(|n| {
            let t = NOW - DAY + u64::from(n) * 100;
            [
                generation(&a, main(n), "L", t),
                generation(&b, main(n), "L", t),
            ]
        })
        .collect();
    assert_eq!(doomed(&all, &unlimited()), Vec::<String>::new());
}

#[test]
fn main_born_over_seven_days_ago_goes_except_the_newest_of_its_key() {
    let k = rust();
    let all = [
        born(&k, main(1), NOW - 8 * DAY, NOW - 3 * DAY),
        born(&k, main(2), NOW - 8 * DAY, NOW - 2 * DAY),
        born(&k, main(3), NOW - 7 * DAY, NOW - DAY),
        born(&k, main(4), NOW, NOW - 12 * HOUR),
    ];
    assert_eq!(
        doomed(&all, &unlimited()),
        vec![main(1).path(), main(2).path()]
    );
    assert_eq!(doomed(&all[..2], &unlimited()), vec![main(1).path()]);
}

#[test]
fn nothing_published_under_six_hours_ago_is_removed() {
    let k = rust();
    let all = [
        born(&k, main(1), NOW - 9 * DAY, NOW - 6 * HOUR),
        born(&k, main(2), NOW - 9 * DAY, NOW - 6 * HOUR + 1),
        born(&k, main(3), NOW - 9 * DAY, NOW - HOUR),
    ];
    assert_eq!(doomed(&all, &unlimited()), vec![main(1).path()]);
}

#[test]
fn pull_request_keeps_the_two_highest_runs_whatever_the_publish_order() {
    let k = rust();
    let all = [
        generation(&k, pr(7, 4, 1), "L", NOW - DAY + 200),
        generation(&k, pr(7, 3, 1), "L", NOW - DAY + 300),
        generation(&k, pr(7, 5, 1), "L", NOW - DAY),
        generation(&k, pr(7, 4, 2), "L", NOW - DAY + 100),
        generation(&k, pr(8, 1, 1), "L", NOW - DAY),
    ];
    assert_eq!(
        doomed(&all, &unlimited()),
        vec!["pr7/3-1".to_owned(), "pr7/4-1".to_owned()]
    );
}

#[test]
fn pull_request_and_merge_group_generations_expire_by_publish_age() {
    let k = rust();
    let all = [
        born(&k, pr(7, 1, 1), NOW, NOW - 7 * DAY - 1),
        generation(&k, pr(8, 1, 1), "L", NOW - 7 * DAY),
        born(&k, mg(1), NOW, NOW - DAY - 1),
        generation(&k, mg(2), "L", NOW - DAY),
    ];
    assert_eq!(
        doomed(&all, &unlimited()),
        vec![mg(1).path(), "pr7/1-1".to_owned()]
    );
}

#[test]
fn merge_group_generations_do_not_count_towards_the_newest_eight_main() {
    let k = rust();
    let mut all: Vec<Generation> = (1..=8)
        .map(|n| generation(&k, main(n), "L", NOW - DAY + u64::from(n) * 100))
        .collect();
    all.extend((20..25).map(|n| generation(&k, mg(n), "L", NOW - 12 * HOUR)));
    assert_eq!(doomed(&all, &unlimited()), Vec::<String>::new());
}

#[test]
fn space_evicts_oldest_first_until_under_the_low_mark_and_never_the_newest_of_a_key() {
    let (a, b) = (rust(), key("lint", "root", "0123456789abcdef"));
    let t = |n: u64| NOW - DAY + n * 100;
    let all = [
        sized(&b, main(10), t(0), 30),
        sized(&a, main(1), t(1), 40),
        sized(&a, main(2), t(3), 40),
        sized(&a, main(3), t(4), 40),
        sized(&a, pr(7, 1, 1), t(5), 10),
        sized(&a, main(4), t(6), 40),
    ];
    let budget = Budget {
        high_bytes: 150,
        low_bytes: 90,
    };
    assert_eq!(
        doomed(&all, &budget),
        vec![main(1).path(), main(2).path(), main(3).path()]
    );
}

#[test]
fn space_is_left_alone_while_the_total_does_not_exceed_the_high_mark() {
    let k = rust();
    let all = [
        sized(&k, main(1), NOW - DAY, 50),
        sized(&k, main(2), NOW - DAY + 1, 50),
    ];
    let budget = Budget {
        high_bytes: 100,
        low_bytes: 0,
    };
    assert_eq!(doomed(&all, &budget), Vec::<String>::new());
}

#[test]
fn space_never_evicts_anything_under_the_six_hour_floor() {
    let k = rust();
    let all = [
        sized(&k, pr(7, 1, 1), NOW - 10 * HOUR, 50),
        sized(&k, pr(8, 1, 1), NOW - HOUR, 50),
        sized(&k, main(1), NOW - 20 * HOUR, 50),
    ];
    let budget = Budget {
        high_bytes: 10,
        low_bytes: 0,
    };
    assert_eq!(doomed(&all, &budget), vec!["pr7/1-1".to_owned()]);
}

#[test]
fn default_budget_trims_from_above_250_gib_to_below_200_gib() {
    let k = rust();
    let gib = 1024 * 1024 * 1024;
    let all = [
        sized(&k, main(1), NOW - 3 * DAY, 90 * gib),
        sized(&k, main(2), NOW - 2 * DAY, 90 * gib),
        sized(&k, main(3), NOW - DAY, 90 * gib),
    ];
    assert_eq!(doomed(&all, &Budget::DEFAULT), vec![main(1).path()]);
    assert_eq!(doomed(&all[1..], &Budget::DEFAULT), Vec::<String>::new());
}

#[test]
fn space_keeps_evicting_until_strictly_below_the_low_mark() {
    let k = rust();
    let all = [
        sized(&k, main(1), NOW - 3 * DAY, 40),
        sized(&k, main(2), NOW - 2 * DAY, 40),
        sized(&k, main(3), NOW - DAY, 40),
    ];
    let budget = Budget {
        high_bytes: 100,
        low_bytes: 80,
    };
    assert_eq!(doomed(&all, &budget), vec![main(1).path(), main(2).path()]);
}

#[test]
fn pull_request_count_never_removes_a_generation_under_the_six_hour_floor() {
    let k = rust();
    let all = [
        generation(&k, pr(7, 9, 1), "L", NOW - DAY),
        generation(&k, pr(7, 5, 1), "L", NOW - DAY),
        generation(&k, pr(7, 1, 1), "L", NOW - HOUR),
    ];
    assert_eq!(doomed(&all, &unlimited()), Vec::<String>::new());
}
