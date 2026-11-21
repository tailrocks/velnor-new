// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::waste::{primed_store, with_store};
use super::{ResultTest, fixture};
use serde_json::json;
use std::fs;

const REAL_MBX_1_30_LOG: &str = "\
mbx[cache]: object cache: 0 hits, 89 misses, 12 not looked up; 0 B downloaded, 0 B uploaded, 77.5 MiB stored locally
mbx[cache]: 27.42s session; 1.08s estimated compiler time avoided (summed across compilations)
mbx[cache]: 12 compilations had no usable prior inputs or matching prediction for a lookup
mbx[cache]: Cargo artifact reuse and CI cache archive transfers are not included above
mbx[cache]: object cache: 0 hits, 3 misses; 0 B downloaded, 0 B uploaded, 1.1 MiB stored locally
mbx[cache]: 662.5ms session; 0s estimated compiler time avoided (summed across compilations)
mbx[cache]: Cargo artifact reuse and CI cache archive transfers are not included above
mbx[warning]: prediction was not restored: cached action is missing blob af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262
";
const REAL_MBX_1_30_DETAIL_ONLY: &str = "\
mbx[cache]: 12 compilations had no usable prior inputs or matching prediction for a lookup
mbx[cache]: Cargo artifact reuse and CI cache archive transfers are not included above
";
const SINGULAR_SUMMARY: &str = "mbx[cache]: object cache: 3 hit, 1 miss; 0 B downloaded";
const DUPLICATE_HITS_SUMMARY: &str = "mbx[cache]: object cache: 3 hits, 5 misses, 0 hits; 0 B downloaded, 0 B uploaded, 0 B stored locally";
const OMITTED_ZERO_SUMMARY: &str = "mbx[cache]: object cache: 0 misses, 15 incremental; 0 B downloaded, 0 B uploaded, 0 B stored locally";
const BYPASSED_ONLY_SUMMARY: &str = "mbx[cache]: object cache: 0 hits, 0 misses, 4 bypassed; 0 B downloaded, 0 B uploaded, 0 B stored locally";
const ZERO_HIT_SUMMARY: &str = "mbx[cache]: object cache: 0 hits, 5 misses; 0 B downloaded";
const PARTIAL_SUMMARY: &str = "mbx[cache]: object cache: 4 hits, many misses; 0 B downloaded";

fn registry_compiled_unit() -> serde_json::Value {
    json!({
        "package_id":"registry+https://example.invalid/index#serde@1.0.0",
        "manifest_path":"/registry/serde/Cargo.toml",
        "target":{"name":"serde","kind":["lib"],"crate_types":["lib"],"src_path":"/registry/serde/src/lib.rs"},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":["target/debug/deps/libserde-hash.rmeta"],"fresh":false
    })
}

pub(super) fn lane_with_mbx_log(
    name: &str,
    log: &str,
    declared: bool,
    primed: bool,
) -> Result<velnor_ci::report::LaneMetrics, Box<dyn std::error::Error>> {
    let repo = fixture(name)?;
    if declared {
        repo.write("mise.toml", "[wrappers.cargo]\ncommand = \"mbx\"\n");
    }
    if primed {
        primed_store(&repo)?;
    }
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["argv"] = json!(["cargo", "build", "--workspace"]);
    invocation["started_at"] = json!(2_000_000);
    invocation["compilations"] = json!([]);
    invocation["progress"] = serde_json::Value::Null;
    invocation["log_start"] = json!(0);
    invocation["log_end"] = json!(log.len());
    invocation["artifacts"] = json!([registry_compiled_unit()]);
    fs::write(repo.root().join("velnor-run.log"), log)?;
    fs::write(
        repo.root().join("velnor-invocations.jsonl"),
        format!("{invocation}\n"),
    )?;
    with_store(&repo, primed)
}

fn unparsed_warning(metrics: &velnor_ci::report::LaneMetrics, first: &str) -> bool {
    metrics.verification_errors.iter().any(|line| {
        line.contains(first)
            && line.contains("MBX lookup summary not understood (1 line(s))")
            && line
                .to_lowercase()
                .contains("update the mbx summary parser")
    })
}

pub(super) fn fails_closed(metrics: &velnor_ci::report::LaneMetrics) -> bool {
    velnor_ci::report::check(std::slice::from_ref(metrics)).is_err_and(|error| {
        error
            .to_string()
            .contains("MBX lookup summary not understood")
    })
}

fn waste_gates(metrics: &velnor_ci::report::LaneMetrics) -> Vec<&String> {
    metrics
        .violations
        .iter()
        .filter(|line| line.contains("MBX object cache") || line.contains("compiled without mbx"))
        .collect()
}

#[test]
fn a_singular_mbx_summary_is_accepted_and_an_absent_one_is_not_silent() -> ResultTest {
    let metrics = lane_with_mbx_log(
        "report-mbx-singular",
        &format!("{SINGULAR_SUMMARY}\n"),
        true,
        false,
    )?;
    assert_eq!(metrics.mbx.sessions.len(), 1, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].hits, 3, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].misses, 1, "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0, "{metrics:?}");
    let silent = lane_with_mbx_log("report-mbx-no-summary", "", true, false)?;
    assert_eq!(silent.mbx.unparsed, 0);
    assert!(
        silent
            .violations
            .iter()
            .any(|line| line.contains("compiled without mbx")),
        "{:?}",
        silent.violations
    );
    Ok(())
}

#[test]
fn partial_mbx_evidence_does_not_fire_the_zero_hit_gate() -> ResultTest {
    let whole = lane_with_mbx_log(
        "report-mbx-zero-hit",
        &format!("{ZERO_HIT_SUMMARY}\n"),
        true,
        true,
    )?;
    assert!(
        whole
            .violations
            .iter()
            .any(|line| line.contains("MBX object cache: 0 hits")),
        "{:?}",
        whole.violations
    );
    let partial = lane_with_mbx_log(
        "report-mbx-partial",
        &format!("{ZERO_HIT_SUMMARY}\n{PARTIAL_SUMMARY}\n"),
        true,
        true,
    )?;
    assert_eq!(partial.mbx.sessions.len(), 1, "{partial:?}");
    assert_eq!(partial.mbx.unparsed, 1, "{partial:?}");
    assert!(
        unparsed_warning(&partial, PARTIAL_SUMMARY),
        "{:?}",
        partial.verification_errors
    );
    assert!(fails_closed(&partial), "{partial:?}");
    assert!(waste_gates(&partial).is_empty(), "{:?}", partial.violations);
    Ok(())
}

#[test]
fn real_mbx_1_30_detail_lines_are_not_unparsed_and_both_gates_stay_active() -> ResultTest {
    let zero_hit = lane_with_mbx_log("report-mbx-real-zero-hit", REAL_MBX_1_30_LOG, true, true)?;
    assert_eq!(zero_hit.mbx.sessions.len(), 2, "{zero_hit:?}");
    assert_eq!(zero_hit.mbx.unparsed, 0, "{zero_hit:?}");
    assert!(
        zero_hit
            .violations
            .iter()
            .any(|line| line.contains("MBX object cache: 0 hits")),
        "{:?}",
        zero_hit.violations
    );
    let detail_only = lane_with_mbx_log(
        "report-mbx-real-detail-only",
        REAL_MBX_1_30_DETAIL_ONLY,
        true,
        false,
    )?;
    assert!(detail_only.mbx.sessions.is_empty(), "{detail_only:?}");
    assert_eq!(detail_only.mbx.unparsed, 0, "{detail_only:?}");
    assert!(
        detail_only
            .violations
            .iter()
            .any(|line| line.contains("compiled without mbx")),
        "{:?}",
        detail_only.violations
    );
    Ok(())
}

#[test]
fn a_truncated_labelled_summary_is_unparsed_and_fails_closed() -> ResultTest {
    let metrics = lane_with_mbx_log(
        "report-mbx-truncated-label",
        "mbx[cache]: object cache:\n",
        true,
        false,
    )?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    assert!(
        unparsed_warning(&metrics, "mbx[cache]: object cache:"),
        "{:?}",
        metrics.verification_errors
    );
    assert!(fails_closed(&metrics), "{metrics:?}");
    Ok(())
}

#[test]
fn an_unknown_counter_makes_a_summary_unparsed_labelled_or_not() -> ResultTest {
    for (index, line) in [
        "mbx[cache]: 3 frobs, 1 hits, 0 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally",
        "mbx[cache]: object cache: 3 frobs, 1 hits, 0 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally",
    ]
    .into_iter()
    .enumerate()
    {
        let metrics = lane_with_mbx_log(
            &format!("report-mbx-unknown-counter-{index}"),
            &format!("{line}\n"),
            true,
            false,
        )?;
        assert!(metrics.mbx.sessions.is_empty(), "{line}: {metrics:?}");
        assert_eq!(metrics.mbx.unparsed, 1, "{line}: {metrics:?}");
        assert!(fails_closed(&metrics), "{line}: {metrics:?}");
    }
    Ok(())
}

#[test]
fn bypassed_and_incremental_counters_are_accepted() -> ResultTest {
    let line = "mbx[cache]: object cache: 0 hits, 2 misses, 1 bypassed, 3 incremental; 0 B downloaded, 0 B uploaded, 0 B stored locally";
    let metrics = lane_with_mbx_log(
        "report-mbx-bypassed-incremental",
        &format!("{line}\n"),
        true,
        false,
    )?;
    assert_eq!(metrics.mbx.sessions.len(), 1, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].misses, 2, "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0, "{metrics:?}");
    Ok(())
}

#[test]
fn a_repeated_mbx_counter_makes_the_summary_unparsed() -> ResultTest {
    let metrics = lane_with_mbx_log(
        "report-mbx-duplicate-counter",
        &format!("{DUPLICATE_HITS_SUMMARY}\n"),
        true,
        false,
    )?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    assert!(fails_closed(&metrics), "{metrics:?}");
    Ok(())
}

#[test]
fn an_omitted_zero_counter_is_zero_and_incremental_work_is_not_dropped() -> ResultTest {
    let metrics = lane_with_mbx_log(
        "report-mbx-omitted-zero",
        &format!("{OMITTED_ZERO_SUMMARY}\n"),
        true,
        false,
    )?;
    assert_eq!(metrics.mbx.sessions.len(), 1, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].hits, 0, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].misses, 0, "{metrics:?}");
    assert_eq!(metrics.mbx.sessions[0].not_looked_up, 15, "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0, "{metrics:?}");
    Ok(())
}

#[test]
fn undifferentiated_stderr_bypasses_do_not_trigger_the_zero_hit_gate() -> ResultTest {
    let metrics = lane_with_mbx_log(
        "report-mbx-bypassed-only",
        &format!("{BYPASSED_ONLY_SUMMARY}\n"),
        true,
        true,
    )?;
    assert_eq!(metrics.mbx.unparsed, 0, "{metrics:?}");
    assert!(waste_gates(&metrics).is_empty(), "{:?}", metrics.violations);
    Ok(())
}

#[test]
fn a_label_cut_before_its_colon_is_unparsed_and_fails_closed() -> ResultTest {
    for (index, line) in ["mbx[cache]: object cache", "mbx[cache]: object"]
        .into_iter()
        .enumerate()
    {
        let metrics = lane_with_mbx_log(
            &format!("report-mbx-cut-label-{index}"),
            &format!("{line}\n"),
            true,
            false,
        )?;
        assert!(metrics.mbx.sessions.is_empty(), "{line}: {metrics:?}");
        assert_eq!(metrics.mbx.unparsed, 1, "{line}: {metrics:?}");
        assert!(fails_closed(&metrics), "{line}: {metrics:?}");
    }
    Ok(())
}

#[test]
fn a_summary_cut_before_its_terminator_is_unparsed_not_a_zero_hit_session() -> ResultTest {
    for (index, line) in [
        "mbx[cache]: object cache: 5 misses",
        "mbx[cache]: object cache: 0 hits, 5 misses",
    ]
    .into_iter()
    .enumerate()
    {
        let metrics = lane_with_mbx_log(
            &format!("report-mbx-cut-terminator-{index}"),
            &format!("{line}\n"),
            true,
            false,
        )?;
        assert!(metrics.mbx.sessions.is_empty(), "{line}: {metrics:?}");
        assert_eq!(metrics.mbx.unparsed, 1, "{line}: {metrics:?}");
        assert!(fails_closed(&metrics), "{line}: {metrics:?}");
    }
    Ok(())
}

#[test]
fn a_non_numeric_hit_count_is_unparsed_not_dropped() -> ResultTest {
    let line = "mbx[cache]: object cache: unknown hits, 2 misses; 0 B downloaded, 0 B uploaded, 0 B stored locally";
    let metrics = lane_with_mbx_log(
        "report-mbx-non-numeric-hits",
        &format!("{line}\n"),
        true,
        false,
    )?;
    assert!(metrics.mbx.sessions.is_empty(), "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 1, "{metrics:?}");
    assert!(fails_closed(&metrics), "{metrics:?}");
    Ok(())
}

fn lane_with_mbx_report(
    name: &str,
    report: &str,
) -> Result<velnor_ci::report::LaneMetrics, Box<dyn std::error::Error>> {
    let repo = fixture(name)?;
    repo.write("mise.toml", "[wrappers.cargo]\ncommand = \"mbx\"\n");
    let root = fs::canonicalize(repo.root())?;
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["argv"] = json!(["cargo", "build", "--workspace"]);
    invocation["started_at"] = json!(2_000_000);
    invocation["compilations"] = json!([]);
    invocation["progress"] = serde_json::Value::Null;
    invocation["log_start"] = json!(0);
    invocation["log_end"] = json!(0);
    invocation["artifacts"] = json!([registry_compiled_unit()]);
    fs::write(repo.root().join("velnor-run.log"), "")?;
    fs::write(
        repo.root().join("velnor-invocations.jsonl"),
        format!("{invocation}\n"),
    )?;
    let reports = repo.root().join("velnor-mbx-reports");
    fs::create_dir_all(&reports)?;
    fs::write(reports.join("1-1-0.expected"), "")?;
    fs::write(reports.join("1-1-0.json"), report)?;
    with_store(&repo, false)
}

#[test]
fn a_lane_whose_only_mbx_reports_are_probes_still_compiled_without_mbx() -> ResultTest {
    let idle = r#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"bypasses":{"compiler-query":2,"standard-input":1}}"#;
    let probed = lane_with_mbx_report("report-mbx-probe-only", idle)?;
    assert_eq!(probed.mbx.sessions.len(), 1, "{probed:?}");
    assert_eq!(probed.mbx.sessions[0].bypasses.len(), 2, "{probed:?}");
    assert!(
        probed
            .violations
            .iter()
            .any(|line| line.contains("compiled without mbx")),
        "{:?}",
        probed.violations
    );
    let compiled =
        r#"{"version":5,"hits":0,"misses":3,"unconsulted":0,"bypasses":{"compiler-query":2}}"#;
    let real = lane_with_mbx_report("report-mbx-probe-and-compile", compiled)?;
    assert!(waste_gates(&real).is_empty(), "{:?}", real.violations);
    Ok(())
}

#[test]
fn a_lane_whose_only_mbx_report_is_incremental_compilations_is_not_compiled_without_mbx() -> ResultTest {
    let incremental =
        r#"{"version":5,"hits":0,"misses":0,"unconsulted":0,"incremental_compilations":15}"#;
    let metrics = lane_with_mbx_report("report-mbx-incremental-only", incremental)?;
    assert_eq!(metrics.mbx.sessions.len(), 1, "{metrics:?}");
    assert!(
        metrics
            .violations
            .iter()
            .all(|line| !line.contains("compiled without mbx")),
        "{:?}",
        metrics.violations
    );
    Ok(())
}
