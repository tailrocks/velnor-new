// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{LaneJob, LaneRunner, LaneScope, ParallelLane, RustCiConfig, TestNeed};

use crate::config::runner;
use crate::error::Reason;

pub(crate) struct RawLaneJob<'a> {
    pub(crate) job: Option<&'a str>,
    pub(crate) runner: Option<&'a str>,
    pub(crate) hosted: Option<bool>,
    pub(crate) needs: &'a [String],
}

fn invalid(key: &'static str, value: &impl ToString) -> Reason {
    Reason::InvalidValue {
        key,
        value: value.to_string(),
    }
}

pub(crate) fn scope(filter: Option<String>, exclusive: Option<bool>) -> Result<LaneScope, Reason> {
    match (filter, exclusive.unwrap_or(false)) {
        (Some(filter), true) => Ok(LaneScope::Exclusive(filter)),
        (Some(filter), false) => Ok(LaneScope::Rerun(filter)),
        (None, true) => Err(invalid("test_lane.exclusive", &true)),
        (None, false) => Ok(LaneScope::Whole),
    }
}

pub(crate) fn parse(raw: &RawLaneJob<'_>) -> Result<LaneJob, Reason> {
    match raw.job {
        None | Some("inline") => inline(raw),
        Some("parallel") => parallel(raw).map(LaneJob::Parallel),
        Some(other) => Err(invalid("test_lane.job", &other)),
    }
}

fn inline(raw: &RawLaneJob<'_>) -> Result<LaneJob, Reason> {
    if let Some(label) = raw.runner {
        return Err(invalid("test_lane.runner", &label));
    }
    if let Some(hosted) = raw.hosted {
        return Err(invalid("test_lane.hosted", &hosted));
    }
    if !raw.needs.is_empty() {
        return Err(invalid("test_lane.needs", &raw.needs.join(",")));
    }
    Ok(LaneJob::Inline)
}

fn parallel(raw: &RawLaneJob<'_>) -> Result<ParallelLane, Reason> {
    let runner = match (raw.runner, raw.hosted) {
        (Some(label), hosted) => Some(LaneRunner {
            label: runner(Some(label.to_owned()), RustCiConfig::default().runner)
                .map_err(|_| invalid("test_lane.runner", &label))?,
            hosted,
        }),
        (None, Some(hosted)) => return Err(invalid("test_lane.hosted", &hosted)),
        (None, None) => None,
    };
    let mut needs = Vec::new();
    for need in raw.needs {
        match (need.as_str(), needs.contains(&TestNeed::Docker)) {
            ("docker", false) => needs.push(TestNeed::Docker),
            _ => return Err(invalid("test_lane.needs", &need)),
        }
    }
    Ok(ParallelLane { runner, needs })
}
