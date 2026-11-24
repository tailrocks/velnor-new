// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{DenyCheck, DenyLane, PolicyTable};

use crate::error::Reason;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPolicy {
    deny: Option<bool>,
    deny_checks: Option<Vec<String>>,
    gate_on_plan: Option<bool>,
}

pub(crate) fn parse(raw: Option<RawPolicy>) -> Result<Option<PolicyTable>, Reason> {
    raw.map(|raw| {
        Ok(PolicyTable {
            deny: match raw.deny {
                None => DenyLane::Auto,
                Some(true) => DenyLane::On,
                Some(false) => DenyLane::Off,
            },
            deny_checks: raw.deny_checks.as_deref().map(checks).transpose()?,
            gate_on_plan: raw.gate_on_plan.unwrap_or(false),
        })
    })
    .transpose()
}

fn checks(names: &[String]) -> Result<Vec<DenyCheck>, Reason> {
    let invalid = |name: &str| Reason::InvalidValue {
        key: "deny_checks",
        value: name.to_owned(),
    };
    let mut parsed = Vec::with_capacity(names.len());
    for name in names {
        let check = match name.as_str() {
            "advisories" => DenyCheck::Advisories,
            "bans" => DenyCheck::Bans,
            "licenses" => DenyCheck::Licenses,
            "sources" => DenyCheck::Sources,
            _ => return Err(invalid(name)),
        };
        if parsed.contains(&check) {
            return Err(invalid(name));
        }
        parsed.push(check);
    }
    if parsed.is_empty() {
        return Err(invalid("[]"));
    }
    parsed.sort();
    Ok(parsed)
}
