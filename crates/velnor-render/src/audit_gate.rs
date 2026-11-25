// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::When;

use crate::gate::Setup;

const SCHEDULED_OR_DISPATCHED: &str =
    "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch'";
const EXPIRED: &str = "steps.restored.outputs.expired == 'true'";
const PULL_REQUEST: &str = "github.event_name == 'pull_request'";
const MERGE_GROUP: &str = "github.event_name == 'merge_group'";
const PUSH: &str = "github.event_name == 'push'";

pub(crate) fn events(when: When, setup: Setup) -> String {
    let mut terms = vec![SCHEDULED_OR_DISPATCHED];
    if matches!(
        when,
        When::FullAuditOrPush | When::FullAuditOrPushOrPullRequest
    ) {
        terms.push(PUSH);
    } else if setup != Setup::Before {
        terms.push(EXPIRED);
    }
    if matches!(
        when,
        When::FullAuditOrPullRequest | When::FullAuditOrPushOrPullRequest
    ) {
        terms.extend([PULL_REQUEST, MERGE_GROUP]);
    }
    format!("({})", terms.join(" || "))
}
