// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CronExpr, DocsRun, FullRun, OnMain};

use crate::error::Reason;

pub(crate) fn on_main(configured: Option<&str>, default: OnMain) -> Result<OnMain, Reason> {
    match configured {
        None => Ok(default),
        Some("affected") => Ok(OnMain::Affected),
        Some("everything") => Ok(OnMain::Everything),
        Some(other) => Err(Reason::InvalidValue {
            key: "on_main",
            value: other.to_owned(),
        }),
    }
}

pub(crate) fn full_run(value: String) -> Result<FullRun, Reason> {
    Ok(match value.as_str() {
        "off" => FullRun::Off,
        "hourly" => FullRun::Hourly,
        "nightly" => FullRun::Nightly,
        "weekly" => FullRun::Weekly,
        _ => FullRun::Cron(
            CronExpr::new(value.clone()).map_err(|_| Reason::InvalidValue {
                key: "full_run",
                value,
            })?,
        ),
    })
}

pub(crate) fn docs_run(configured: Option<&str>) -> Result<DocsRun, Reason> {
    match configured {
        None | Some("always") => Ok(DocsRun::Always),
        Some("post_merge") => Ok(DocsRun::PostMerge),
        Some(other) => Err(Reason::InvalidValue {
            key: "docs",
            value: other.to_owned(),
        }),
    }
}
