// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{Gate, GateNeed, When};

use crate::gate::{Setup, output, plan_read, plan_source};

const SCHEDULED_OR_DISPATCHED: &str =
    "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'";
const EXPIRED: &str = "steps.restored.outputs.expired == 'true'";
const PULL_REQUEST_OR_MERGE_GROUP: &str =
    "(github.event_name == 'pull_request' || github.event_name == 'merge_group')";
const PUSH: &str = "github.event_name == 'push'";

pub(crate) fn events(when: When, gate: Option<&Gate>, setup: Setup) -> String {
    let mut terms = vec![SCHEDULED_OR_DISPATCHED.to_owned()];
    if when == When::FullAuditOrPush {
        terms.push(PUSH.to_owned());
    } else if setup != Setup::Before {
        terms.push(EXPIRED.to_owned());
    }
    terms.extend(inputs_changed(gate));
    format!("({})", terms.join(" || "))
}

fn inputs_changed(gate: Option<&Gate>) -> Option<String> {
    let gate = gate.filter(|gate| gate.need == GateNeed::Inputs)?;
    let read = plan_read(
        &plan_source(&gate.from),
        &output(&gate.slug, GateNeed::Inputs),
        Setup::Before,
    );
    Some(format!(
        "({PULL_REQUEST_OR_MERGE_GROUP} && {read} == 'run')"
    ))
}
