// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! MBX's JSON build report, which MBX writes to the `MBX_STATS_REPORT` path when a cargo session
//! ends. MBX versions this report and may reword its `mbx[...]` stderr lines at any time, so a job
//! that sets [`SWITCH`] counts its sessions from the reports instead of the stderr summary.

use crate::waste_evidence::MbxSession;
use serde::Deserialize;
use serde_json::{Map, Value};

/// Set to `true` by the generated jobs: every cargo session that `velnor-ci` starts gets its own
/// report path under `$RUNNER_TEMP/`[`DIRECTORY`], and `velnor-ci report` reads them all. MBX
/// replaces the file at the path when a session ends, so sessions that shared one path would keep
/// only the last session.
pub const SWITCH: &str = "VELNOR_MBX_REPORTS";
pub const DIRECTORY: &str = "velnor-mbx-reports";
pub const VARIABLE: &str = "MBX_STATS_REPORT";
/// The report versions whose `hits`, `misses` and `unconsulted` counts this reader maps. MBX bumps
/// the version when the shape changes; a report of any other version is not understood.
pub const KNOWN_VERSIONS: &[u64] = &[5];

#[derive(Deserialize)]
struct Counts {
    hits: u64,
    misses: u64,
    unconsulted: u64,
    #[serde(default)]
    bypasses: Map<String, Value>,
}

/// The session one report records, or `None` for a session that compiled nothing through MBX (MBX
/// prints no stderr summary for it either).
///
/// # Errors
///
/// The reason the report is not understood: not JSON, an unknown `version`, or a missing count.
pub fn parse(report: &[u8]) -> Result<Option<MbxSession>, String> {
    let value: Value =
        serde_json::from_slice(report).map_err(|error| format!("not JSON ({error})"))?;
    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "no numeric version".to_owned())?;
    if !KNOWN_VERSIONS.contains(&version) {
        return Err(format!(
            "version {version}, while this reader knows {KNOWN_VERSIONS:?}"
        ));
    }
    let counts: Counts = serde_json::from_value(value)
        .map_err(|error| format!("version {version} without its counts ({error})"))?;
    let looked_at = counts.hits + counts.misses + counts.unconsulted > 0;
    Ok((looked_at || !counts.bypasses.is_empty()).then_some(MbxSession {
        hits: counts.hits,
        misses: counts.misses,
        not_looked_up: counts.unconsulted,
    }))
}
