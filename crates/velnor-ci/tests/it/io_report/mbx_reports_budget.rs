// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::{report, report_json};
use super::mbx_reports_sessions::plant_session;
use super::{Repo, ResultTest, fixture};

fn plant_padded(repo: &Repo, from: usize, count: usize, size: usize) -> ResultTest {
    let mut json = report_json(5, 1, 0, 0);
    json.push_str(&" ".repeat(size - json.len()));
    for index in from..from + count {
        plant_session(repo, &format!("{index:05}"), Some(&json))?;
    }
    Ok(())
}

fn overflow_after(count: usize, size: usize, name: &str) -> ResultTest {
    let repo = fixture(name)?;
    plant_padded(&repo, 0, count, size)?;
    assert_byte_overflow(&repo)
}

fn assert_byte_overflow(repo: &Repo) -> ResultTest {
    let metrics = report(repo)?;
    assert!(
        metrics.mbx.sessions.is_empty(),
        "{}",
        metrics.mbx.sessions.len()
    );
    assert_eq!(metrics.mbx.unparsed, 1);
    assert!(
        metrics
            .verification_errors
            .iter()
            .any(|line| line.contains("MBX stats reports exceed") && line.contains("32 MiB")),
        "{:?}",
        metrics.verification_errors
    );
    assert!(velnor_ci::report::check(std::slice::from_ref(&metrics)).is_err());
    Ok(())
}

#[test]
fn reports_totalling_past_the_byte_budget_are_unknown_evidence_that_fails_the_check() -> ResultTest
{
    overflow_after(40, 1_000_000, "mbx-reports-byte-flood")
}

#[test]
fn a_last_report_that_crosses_the_byte_budget_is_unknown_evidence_too() -> ResultTest {
    overflow_after(34, 1_000_000, "mbx-reports-byte-last")
}

#[test]
fn oversized_reports_count_toward_the_byte_budget() -> ResultTest {
    overflow_after(33, 1_048_577, "mbx-reports-byte-oversized")
}

#[test]
fn reports_totalling_exactly_the_byte_budget_are_all_read() -> ResultTest {
    let repo = fixture("mbx-reports-byte-exact")?;
    plant_padded(&repo, 0, 32, 1_048_576)?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions.len(), 32, "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}

#[test]
fn one_small_report_past_the_exact_byte_budget_is_unknown_evidence() -> ResultTest {
    let repo = fixture("mbx-reports-byte-exact-plus")?;
    plant_padded(&repo, 0, 32, 1_048_576)?;
    plant_session(&repo, "99999", Some(&report_json(5, 1, 0, 0)))?;
    assert_byte_overflow(&repo)
}

#[test]
fn reports_totalling_under_the_byte_budget_are_all_read() -> ResultTest {
    let repo = fixture("mbx-reports-byte-budget")?;
    plant_padded(&repo, 0, 33, 1_000_000)?;
    let metrics = report(&repo)?;
    assert_eq!(metrics.mbx.sessions.len(), 33, "{metrics:?}");
    assert_eq!(metrics.mbx.unparsed, 0);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(())
}
