// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ResultTest;
use super::waste_mbx::{fails_closed, lane_with_mbx_log, lane_with_mbx_report};

#[test]
fn every_truncation_of_the_object_cache_label_is_unparsed_and_fails_closed() -> ResultTest {
    const LABEL: &str = "object cache:";
    for end in 1..=LABEL.len() {
        let line = format!("mbx[cache]: {}", &LABEL[..end]);
        let metrics = lane_with_mbx_log(
            &format!("report-mbx-label-prefix-{end}"),
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
fn a_summary_without_its_downloaded_tail_is_unparsed_even_with_every_counter() -> ResultTest {
    for (index, line) in [
        "mbx[cache]: object cache: 0 hits, 5 misses;",
        "mbx[cache]: object cache: 0 hits, 5 misses; 0 B",
        "mbx[cache]: object cache: 0 hits, 5 misses; 0 B uploaded, 0 B stored locally",
    ]
    .into_iter()
    .enumerate()
    {
        let metrics = lane_with_mbx_log(
            &format!("report-mbx-no-tail-{index}"),
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
fn an_omitted_counter_is_zero_only_when_the_downloaded_tail_is_present() -> ResultTest {
    let cut = lane_with_mbx_log(
        "report-mbx-omitted-no-tail",
        "mbx[cache]: object cache: 5 misses;\n",
        true,
        false,
    )?;
    assert!(cut.mbx.sessions.is_empty(), "{cut:?}");
    assert_eq!(cut.mbx.unparsed, 1, "{cut:?}");
    assert!(fails_closed(&cut), "{cut:?}");
    let whole = lane_with_mbx_log(
        "report-mbx-omitted-with-tail",
        "mbx[cache]: object cache: 5 misses; 0 B downloaded\n",
        true,
        false,
    )?;
    assert_eq!(whole.mbx.sessions.len(), 1, "{whole:?}");
    assert_eq!(whole.mbx.sessions[0].hits, 0, "{whole:?}");
    assert_eq!(whole.mbx.sessions[0].misses, 5, "{whole:?}");
    assert_eq!(whole.mbx.unparsed, 0, "{whole:?}");
    Ok(())
}

#[test]
fn a_lane_whose_only_mbx_report_is_incremental_compilations_is_not_compiled_without_mbx()
-> ResultTest {
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
