// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::compare::compare;

use crate::support::recorded_metrics;

#[test]
fn faster_second_run_shows_negative_changes_and_the_changed_job() {
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    b.wall_seconds = 600;
    b.runner_seconds = 500;
    b.jobs[1].duration_seconds = 400;
    b.jobs[1].log.compiling = 0;
    b.jobs.remove(2);

    let table = compare(&a, &b);
    assert!(
        table.contains("| Wall seconds | 963 | 600 | -363 |"),
        "{table}"
    );
    assert!(
        table.contains("| Runner seconds | 936 | 500 | -436 |"),
        "{table}"
    );
    assert!(table.contains("| Jobs run | 3 | 2 | -1 |"), "{table}");
    assert!(
        table.contains("| Crates compiled | 28 | 2 | -26 |"),
        "{table}"
    );
    assert!(
        table.contains("| Rust / velnor-actions-cli | 854 | 400 | -454 |"),
        "{table}"
    );
    assert!(
        table.contains("| Rust / velnor-runner-core | 70 | - | -70 |"),
        "{table}"
    );
    assert!(
        !table.contains("Cargo Machete"),
        "unchanged job listed: {table}"
    );
}

#[test]
fn identical_runs_list_no_changed_jobs() {
    let a = recorded_metrics();
    let table = compare(&a, &a);
    assert!(
        table.contains("| Wall seconds | 963 | 963 | +0 |"),
        "{table}"
    );
    assert!(!table.contains("| Job |"), "{table}");
}

#[test]
fn only_the_ten_largest_job_changes_are_listed() {
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    let template = b.jobs[0].clone();
    let mut a = a;
    for index in 0..12_u64 {
        let mut left = template.clone();
        left.name = format!("job-{index:02}");
        left.duration_seconds = 100;
        let mut right = left.clone();
        right.duration_seconds = 100 + index + 1;
        a.jobs.push(left);
        b.jobs.push(right);
    }
    let table = compare(&a, &b);
    assert!(table.contains("| job-11 | 100 | 112 | +12 |"), "{table}");
    assert!(table.contains("| job-02 | 100 | 103 | +3 |"), "{table}");
    assert!(!table.contains("| job-01 |"), "{table}");
    assert!(!table.contains("| job-00 |"), "{table}");
}

#[test]
fn jobs_with_the_same_name_are_compared_one_to_one() {
    let mut a = recorded_metrics();
    let mut b = recorded_metrics();
    let mut twin = a.jobs[0].clone();
    twin.duration_seconds = 100;
    a.jobs.push(twin.clone());
    twin.duration_seconds = 150;
    b.jobs.push(twin);
    let name = a.jobs[0].name.clone();
    let table = compare(&a, &b);
    assert!(
        table.contains(&format!("| {name} #2 | 100 | 150 | +50 |")),
        "{table}"
    );
    assert!(!table.contains(&format!("| {name} |")), "{table}");
}
