// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::compare::compare;

use crate::support::recorded_metrics;

fn rendered(a: &velnor_ci::metrics::RunMetrics, b: &velnor_ci::metrics::RunMetrics) -> String {
    compare(a, b).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn runs_of_different_workloads_are_not_compared() {
    let a = recorded_metrics();
    for change in 0..3 {
        let mut b = recorded_metrics();
        match change {
            0 => b.repo = "tailrocks/other".to_owned(),
            1 => b.workflow = "Release".to_owned(),
            _ => b.event = "pull_request".to_owned(),
        }
        let message = compare(&a, &b).map(|_| ()).map_err(|e| e.to_string());
        assert!(
            message
                .as_ref()
                .is_err_and(|m| m.contains("cannot compare")),
            "{message:?}"
        );
    }
}

#[test]
fn faster_second_run_shows_negative_changes_and_the_changed_job() {
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    b.wall_seconds = Some(600);
    b.runner_seconds = Some(500);
    b.jobs[1].duration_seconds = Some(400);
    b.jobs[1].log.compiling = 0;
    b.jobs.remove(2);

    let table = rendered(&a, &b);
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
    let table = rendered(&a, &a);
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
        left.duration_seconds = Some(100);
        let mut right = left.clone();
        right.duration_seconds = Some(100 + index + 1);
        a.jobs.push(left);
        b.jobs.push(right);
    }
    let mut absent = template.clone();
    absent.name = "job-absent".to_owned();
    absent.duration_seconds = Some(500);
    a.jobs.push(absent);
    let table = rendered(&a, &b);
    assert!(table.contains("| job-absent | 500 | - | -500 |"), "{table}");
    assert!(table.contains("| job-11 | 100 | 112 | +12 |"), "{table}");
    assert!(table.contains("| job-03 | 100 | 104 | +4 |"), "{table}");
    assert!(!table.contains("| job-02 |"), "{table}");
    assert!(!table.contains("| job-01 |"), "{table}");
    assert!(!table.contains("| job-00 |"), "{table}");
}

#[test]
fn skipped_jobs_are_not_reported_as_unmeasured_changes() {
    let mut a = recorded_metrics();
    let mut b = recorded_metrics();
    let skipped = &mut a.jobs[0];
    skipped.conclusion = "skipped".to_owned();
    skipped.started = false;
    skipped.duration_seconds = None;
    b.jobs.remove(0);

    let table = rendered(&a, &b);
    assert!(!table.contains(&a.jobs[0].name), "{table}");
    assert!(
        table.contains(&format!(
            "| Jobs skipped | {} | {} | -1 |",
            b.jobs_skipped() + 1,
            b.jobs_skipped()
        )),
        "{table}"
    );
}

#[test]
fn a_changed_job_without_a_measurement_survives_the_display_limit() {
    let mut a = recorded_metrics();
    let mut b = recorded_metrics();
    let template = a.jobs[0].clone();
    for index in 0..12_u64 {
        let mut left = template.clone();
        left.name = format!("job-{index:02}");
        left.duration_seconds = Some(100);
        let mut right = left.clone();
        right.duration_seconds = Some(101 + index);
        a.jobs.push(left);
        b.jobs.push(right);
    }
    let mut lost_measurement = template;
    lost_measurement.name = "job-unmeasured".to_owned();
    lost_measurement.duration_seconds = Some(1_000);
    a.jobs.push(lost_measurement.clone());
    lost_measurement.duration_seconds = None;
    b.jobs.push(lost_measurement);

    let table = rendered(&a, &b);
    assert!(
        table.contains("| job-unmeasured | 1000 | unmeasured | - |"),
        "{table}"
    );
}

#[test]
fn metrics_without_a_schema_version_are_rejected() {
    use std::fs;

    use velnor_ci::io::read_metrics;

    let mut old_format =
        serde_json::to_value(recorded_metrics()).unwrap_or_else(|error| unreachable!("{error}"));
    old_format
        .as_object_mut()
        .unwrap_or_else(|| unreachable!("metrics must serialize as an object"))
        .remove("schema_version");
    let directory = crate::support::fresh_dir("velnor-ci-metrics", "old-schema");
    let path = directory.join("metrics.json");
    fs::write(
        &path,
        serde_json::to_vec(&old_format).unwrap_or_else(|error| unreachable!("{error}")),
    )
    .unwrap_or_else(|error| unreachable!("{error}"));

    let result = read_metrics(&path).map(|_| ());
    fs::remove_dir_all(&directory).unwrap_or_else(|error| unreachable!("{error}"));
    assert!(
        result
            .as_ref()
            .is_err_and(|error| error.to_string().contains("schema_version")),
        "{result:?}"
    );
}

#[test]
fn a_measured_zero_job_missing_from_the_other_run_is_not_a_change() {
    let mut a = recorded_metrics();
    let b = recorded_metrics();
    let mut zero = a.jobs[0].clone();
    zero.name = "job-zero-absent".to_owned();
    zero.duration_seconds = Some(0);
    a.jobs.push(zero);
    let table = rendered(&a, &b);
    assert!(!table.contains("job-zero-absent"), "{table}");
}

#[test]
fn jobs_with_the_same_name_are_compared_one_to_one() {
    let mut a = recorded_metrics();
    let mut b = recorded_metrics();
    let mut twin = a.jobs[0].clone();
    twin.duration_seconds = Some(100);
    a.jobs.push(twin.clone());
    twin.duration_seconds = Some(150);
    b.jobs.push(twin);
    let name = a.jobs[0].name.clone();
    let table = rendered(&a, &b);
    assert!(
        table.contains(&format!("| {name} (occurrence 2) | 100 | 150 | +50 |")),
        "{table}"
    );
    assert!(!table.contains(&format!("| {name} |")), "{table}");
}

#[test]
fn a_job_literally_named_like_a_duplicate_does_not_collide() {
    let mut a = recorded_metrics();
    let mut b = recorded_metrics();
    let mut build = a.jobs[0].clone();
    build.name = "build".to_owned();
    build.duration_seconds = Some(10);
    let mut literal = build.clone();
    literal.name = "build #2".to_owned();
    literal.duration_seconds = Some(20);
    a.jobs
        .extend([build.clone(), build.clone(), literal.clone()]);
    build.duration_seconds = Some(11);
    literal.duration_seconds = Some(25);
    let mut second = build.clone();
    second.duration_seconds = Some(10);
    b.jobs.extend([build, second, literal]);
    let table = rendered(&a, &b);
    assert!(table.contains("| build | 10 | 11 | +1 |"), "{table}");
    assert!(table.contains("| build #2 | 20 | 25 | +5 |"), "{table}");
    assert!(!table.contains("occurrence 2"), "{table}");
}

#[test]
fn a_pipe_in_a_job_name_does_not_add_a_table_column() {
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    b.jobs[0].name = "test | linux".to_owned();
    b.jobs[0].duration_seconds = b.jobs[0].duration_seconds.map(|seconds| seconds + 5);
    let table = rendered(&a, &b);
    assert!(table.contains("| test \\| linux | - | "), "{table}");
}
