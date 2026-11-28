// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{GenKey, GenScope, Generation, Timestamp, prune_plan};

use crate::gen_support::{DAY, NOW, generation, key, main, mg, pr, rust};

const HOUR: u64 = 3600;

fn doomed(all: &[Generation]) -> Vec<String> {
    prune_plan(Timestamp(NOW), all)
        .victims
        .iter()
        .map(|g| g.scope.path())
        .collect()
}

fn queued(all: &[Generation]) -> Vec<String> {
    prune_plan(Timestamp(NOW), all)
        .eviction_queue
        .iter()
        .map(|g| g.path())
        .collect()
}

fn at(key: &GenKey, scope: GenScope, published: u64) -> Generation {
    generation(key, scope, "L", published)
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
    assert_eq!(doomed(&all), paths);
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
    assert_eq!(doomed(&all), Vec::<String>::new());
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
    assert_eq!(doomed(&all), vec![main(1).path(), main(2).path()]);
    assert_eq!(doomed(&all[..2]), vec![main(1).path()]);
}

#[test]
fn nothing_published_under_six_hours_ago_is_removed() {
    let k = rust();
    let all = [
        born(&k, main(1), NOW - 9 * DAY, NOW - 6 * HOUR),
        born(&k, main(2), NOW - 9 * DAY, NOW - 6 * HOUR + 1),
        born(&k, main(3), NOW - 9 * DAY, NOW - HOUR),
    ];
    assert_eq!(doomed(&all), vec![main(1).path()]);
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
        doomed(&all),
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
    assert_eq!(doomed(&all), vec![mg(1).path(), "pr7/1-1".to_owned()]);
}

#[test]
fn merge_group_generations_do_not_count_towards_the_newest_eight_main() {
    let k = rust();
    let mut all: Vec<Generation> = (1..=8)
        .map(|n| generation(&k, main(n), "L", NOW - DAY + u64::from(n) * 100))
        .collect();
    all.extend((20..25).map(|n| generation(&k, mg(n), "L", NOW - 12 * HOUR)));
    assert_eq!(doomed(&all), Vec::<String>::new());
}

#[test]
fn main_published_long_ago_but_born_recently_stays() {
    let k = rust();
    let all = [
        born(&k, main(1), NOW - HOUR, NOW - 30 * DAY),
        born(&k, main(2), NOW - HOUR, NOW - 20 * DAY),
    ];
    assert_eq!(doomed(&all), Vec::<String>::new());
}

#[test]
fn an_age_doomed_main_among_the_newest_eight_does_not_use_up_a_count_slot() {
    let k = rust();
    let mut all: Vec<Generation> = (1..=9)
        .map(|n| born(&k, main(n), NOW - DAY, NOW - DAY + u64::from(n) * 100))
        .collect();
    all[7].born = Timestamp(NOW - 8 * DAY);
    assert_eq!(doomed(&all), vec![main(8).path()]);
}

#[test]
fn an_age_doomed_pull_request_run_among_the_newest_two_does_not_use_up_a_count_slot() {
    let k = rust();
    let all = [
        at(&k, pr(7, 1, 1), NOW - 2 * DAY),
        at(&k, pr(7, 2, 1), NOW - 8 * DAY),
        at(&k, pr(7, 3, 1), NOW - DAY),
    ];
    assert_eq!(doomed(&all), vec!["pr7/2-1".to_owned()]);
}

#[test]
fn the_eviction_queue_is_oldest_published_first_whatever_the_birth() {
    let k = rust();
    let all = [
        born(&k, main(1), NOW - HOUR, NOW - 3 * DAY),
        born(&k, main(2), NOW - 6 * DAY, NOW - 2 * DAY),
        born(&k, main(3), NOW - 5 * DAY, NOW - 4 * DAY),
        born(&k, main(4), NOW - 4 * DAY, NOW - DAY),
    ];
    let order: Vec<String> = [3, 1, 2]
        .map(|n| format!("{}/{}", k.dir_name(), main(n).path()))
        .to_vec();
    assert_eq!(queued(&all), order);
}

#[test]
fn the_eviction_queue_never_lists_a_victim() {
    let k = rust();
    let all = [
        born(&k, main(1), NOW - 9 * DAY, NOW - 4 * DAY),
        at(&k, main(2), NOW - 3 * DAY),
        at(&k, main(3), NOW - 2 * DAY),
        at(&k, pr(7, 1, 1), NOW - 8 * DAY),
        at(&k, mg(4), NOW - 2 * DAY),
    ];
    assert_eq!(
        doomed(&all),
        vec![main(1).path(), mg(4).path(), "pr7/1-1".to_owned()]
    );
    assert_eq!(
        queued(&all),
        vec![format!("{}/{}", k.dir_name(), main(2).path())]
    );
}

#[test]
fn the_eviction_queue_protects_the_newest_main_of_each_key() {
    let (a, b) = (rust(), key("lint", "root", "0123456789abcdef"));
    let all = [
        at(&a, main(1), NOW - 3 * DAY),
        at(&a, main(2), NOW - DAY),
        at(&b, main(3), NOW - 5 * DAY),
    ];
    assert_eq!(
        queued(&all),
        vec![format!("{}/{}", a.dir_name(), main(1).path())]
    );
}

#[test]
fn a_key_without_main_protects_its_newest_generation_from_space_only() {
    let k = rust();
    let all = [
        at(&k, pr(7, 1, 1), NOW - 3 * DAY),
        at(&k, pr(8, 1, 1), NOW - 2 * DAY),
        at(&k, mg(1), NOW - 12 * HOUR),
    ];
    assert_eq!(doomed(&all), Vec::<String>::new());
    assert_eq!(
        queued(&all),
        vec![
            format!("{}/pr7/1-1", k.dir_name()),
            format!("{}/pr8/1-1", k.dir_name()),
        ]
    );
}

#[test]
fn a_key_without_main_protects_the_newest_that_age_left_standing() {
    let k = rust();
    let all = [
        at(&k, pr(7, 1, 1), NOW - 3 * DAY),
        at(&k, pr(8, 1, 1), NOW - 2 * DAY),
        at(&k, pr(9, 1, 1), NOW - 8 * DAY),
    ];
    assert_eq!(doomed(&all), vec!["pr9/1-1".to_owned()]);
    assert_eq!(queued(&all), vec![format!("{}/pr7/1-1", k.dir_name())]);
}

#[test]
fn the_eviction_queue_skips_everything_under_the_six_hour_floor() {
    let k = rust();
    let all = [
        at(&k, pr(7, 1, 1), NOW - 10 * HOUR),
        at(&k, pr(8, 1, 1), NOW - HOUR),
        at(&k, main(1), NOW - 20 * HOUR),
        at(&k, pr(9, 1, 1), NOW - 6 * HOUR + 1),
    ];
    assert_eq!(queued(&all), vec![format!("{}/pr7/1-1", k.dir_name())]);
}

#[test]
fn the_plan_does_not_depend_on_the_input_order() {
    let k = rust();
    let all = [
        at(&k, main(1), NOW - 3 * DAY),
        at(&k, pr(7, 1, 1), NOW - 2 * DAY),
        at(&k, mg(2), NOW - DAY),
        at(&k, main(3), NOW - 8 * DAY),
    ];
    let mut reversed = all.clone();
    reversed.reverse();
    assert_eq!(doomed(&all), doomed(&reversed));
    assert_eq!(queued(&all), queued(&reversed));
}

#[test]
fn pull_request_count_never_removes_a_generation_under_the_six_hour_floor() {
    let k = rust();
    let all = [
        generation(&k, pr(7, 9, 1), "L", NOW - DAY),
        generation(&k, pr(7, 5, 1), "L", NOW - DAY),
        generation(&k, pr(7, 1, 1), "L", NOW - HOUR),
    ];
    assert_eq!(doomed(&all), Vec::<String>::new());
}

#[test]
fn a_key_without_main_protects_a_survivor_even_when_the_newest_is_age_doomed() {
    let k = rust();
    let all = [
        at(&k, pr(7, 1, 1), NOW - 3 * DAY),
        at(&k, mg(1), NOW - 2 * DAY),
    ];
    assert_eq!(doomed(&all), vec![mg(1).path()]);
    assert_eq!(queued(&all), Vec::<String>::new());
}

#[test]
fn a_key_with_main_protects_only_its_newest_main_from_space() {
    let k = rust();
    let all = [
        at(&k, main(1), NOW - 3 * DAY),
        at(&k, pr(7, 1, 1), NOW - DAY),
    ];
    assert_eq!(queued(&all), vec![format!("{}/pr7/1-1", k.dir_name())]);
}
