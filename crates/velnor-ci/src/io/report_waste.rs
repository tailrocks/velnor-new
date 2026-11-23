// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::mbx_reports::{MAX_ENTRIES, MAX_SESSIONS, MAX_TOTAL_BYTES};
use crate::mbx_report::reports_session;
use crate::report::{Cache, Hit, LaneMetrics};
use crate::waste_evidence::{
    MbxEvidence, MbxSession, TOOLCHAIN_INSTALLS, mbx_sessions, mbx_unparsed_lines, seed_truncated,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROBE_LIMIT: usize = 20_000;
const OBJECT_CACHE: [&str; 2] = ["actions/cas", "actions/action-results"];

pub(super) fn collect_local(metrics: &mut LaneMetrics, log: &[u8], runner_temp: &Path) {
    let first_cargo = metrics
        .invocations
        .iter()
        .map(|invocation| invocation.started_at)
        .min();
    let store = std::env::var_os("MBX_CACHE_DIR").filter(|dir| !dir.is_empty());
    let (sessions, unparsed) = sessions(metrics, log, runner_temp);
    metrics.mbx = MbxEvidence {
        declared: crate::mbx_declared::declared_in(Path::new(".")),
        store_primed: store.zip(first_cargo).is_some_and(|(dir, start)| {
            OBJECT_CACHE
                .iter()
                .any(|part| has_entry_before(&Path::new(&dir).join(part), start))
        }),
        seed_truncated: seed_truncated(runner_temp),
        sessions,
        unparsed,
    };
    metrics.toolchain_installs = fs::read_to_string(runner_temp.join(TOOLCHAIN_INSTALLS))
        .map_or_default(|text| {
            text.lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        });
}

fn sessions(metrics: &mut LaneMetrics, log: &[u8], runner_temp: &Path) -> (Vec<MbxSession>, u64) {
    let (unrecorded, ledger_unknown) = unrecorded_ledger(metrics, runner_temp);
    let Some(reports) = super::mbx_reports::read(runner_temp) else {
        let (sessions, unparsed) = summaries(metrics, log);
        return (
            sessions,
            unparsed
                .saturating_add(unrecorded_sessions(metrics, unrecorded))
                .saturating_add(ledger_unknown),
        );
    };
    if reports.overflowed {
        metrics.verification_errors.push(format!(
            "MBX stats reports exceed the reader's bounds ({MAX_SESSIONS} sessions, {MAX_ENTRIES} entries or {} MiB in total); the MBX session counts are unknown.",
            MAX_TOTAL_BYTES >> 20
        ));
        return (Vec::new(), 1);
    }
    let mut unassigned = Vec::new();
    for invocation in &metrics.invocations {
        if !reports_session(&invocation.argv) {
            let slice = log
                .get(invocation.log_start..invocation.log_end.min(log.len()))
                .unwrap_or_default();
            unassigned.extend_from_slice(slice);
        }
        unassigned.push(b'\n');
    }
    let missing = reports.missing.len();
    if missing > 0 {
        metrics.verification_errors.push(format!(
            "MBX stats report missing for {missing} session(s) that were given a report path; the MBX session counts are incomplete. First: {}.",
            crate::escape::escape(&reports.missing[0], false)
        ));
    }
    if let Some(first) = reports.unreadable.first() {
        metrics.verification_errors.push(format!(
            "MBX stats report not understood ({} report(s)); first: {}. Update the MBX report reader (crates/velnor-ci/src/mbx_report.rs) for this report.",
            reports.unreadable.len(),
            crate::escape::escape(first, false)
        ));
    }
    if reports.unlisted > 0 {
        metrics.verification_errors.push(format!(
            "MBX stats directory or entry could not be read ({} failure(s)); the MBX session counts are incomplete.",
            reports.unlisted
        ));
    }
    let (text, unparsed) = summaries(metrics, &unassigned);
    let unknown =
        u64::try_from(reports.unreadable.len() + missing + reports.unlisted).unwrap_or(u64::MAX);
    (
        reports.sessions.into_iter().chain(text).collect(),
        unparsed
            .saturating_add(unknown)
            .saturating_add(unrecorded_sessions(metrics, unrecorded))
            .saturating_add(ledger_unknown),
    )
}

fn unrecorded_ledger(metrics: &mut LaneMetrics, runner_temp: &Path) -> (u64, u64) {
    match super::mbx_reports::unrecorded(runner_temp) {
        Ok(count) => (count, 0),
        Err(reason) => {
            metrics.verification_errors.push(format!(
                "MBX unrecorded-session ledger not understood ({reason}); the MBX session counts are unknown."
            ));
            (0, 1)
        }
    }
}

fn unrecorded_sessions(metrics: &mut LaneMetrics, count: u64) -> u64 {
    if count > 0 {
        metrics.verification_errors.push(format!(
            "MBX stats could not be recorded for {count} session(s): the stats directory could not be written. Those commands ran without a report; the MBX session counts are incomplete."
        ));
    }
    count
}

fn summaries(metrics: &mut LaneMetrics, log: &[u8]) -> (Vec<MbxSession>, u64) {
    let unparsed = mbx_unparsed_lines(log);
    if let Some(first) = unparsed.first() {
        metrics.verification_errors.push(format!(
            "MBX lookup summary not understood ({} line(s)); first: {}. Update the mbx summary parser (crates/velnor-ci/src/waste_evidence.rs) for this wording.",
            unparsed.len(),
            crate::escape::escape(first, false)
        ));
    }
    (
        mbx_sessions(log),
        u64::try_from(unparsed.len()).unwrap_or(u64::MAX),
    )
}

fn has_entry_before(store: &Path, start: u64) -> bool {
    let limit = UNIX_EPOCH + Duration::from_secs(start);
    let mut pending = vec![store.to_path_buf()];
    let mut seen = 0_usize;
    while let Some(directory) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            seen += 1;
            if seen > PROBE_LIMIT {
                return false;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file()
                && metadata
                    .modified()
                    .is_ok_and(|modified: SystemTime| modified < limit)
            {
                return true;
            }
        }
    }
    false
}

pub(super) fn entry_before_job(
    cache: &Cache,
    listing: &Value,
    refs: &[String],
    start: i64,
) -> Option<bool> {
    if cache.hit != Hit::Miss || cache.requested_key.is_empty() {
        return None;
    }
    let entries = listing["actions_caches"].as_array()?;
    let readable = |entry: &&Value| {
        entry["key"].as_str() == Some(cache.requested_key.as_str())
            && entry["ref"]
                .as_str()
                .is_some_and(|reference| refs.iter().any(|allowed| allowed == reference))
    };
    let created = |entry: &Value| {
        entry["created_at"]
            .as_str()
            .and_then(crate::time::parse_utc)
    };
    let versions_since_start: BTreeSet<Option<&str>> = entries
        .iter()
        .filter(readable)
        .filter(|entry| created(entry).is_some_and(|created| created >= start))
        .map(|entry| entry["version"].as_str())
        .collect();
    let Some(job_version) = (match versions_since_start.iter().copied().collect::<Vec<_>>()[..] {
        [Some(version)] => Some(version),
        _ => None,
    }) else {
        return Some(false);
    };
    Some(entries.iter().filter(readable).any(|entry| {
        entry["version"].as_str() == Some(job_version)
            && created(entry).is_some_and(|created| created < start)
    }))
}
