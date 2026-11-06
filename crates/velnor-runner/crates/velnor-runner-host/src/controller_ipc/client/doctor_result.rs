// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::wire::DoctorProbeReply;
use crate::readiness::Readiness;

pub(super) fn completed_result(
    reply: DoctorProbeReply,
    readiness: Option<Readiness>,
) -> Option<serde_json::Value> {
    if reply.state != "completed" {
        return None;
    }
    let report = reply.report?;
    let mut result = serde_json::json!({
        "request_id": reply.request_id,
        "state": reply.state,
        "report": report,
    });
    if let Some(readiness) = readiness {
        result["readiness"] = serde_json::json!(readiness);
    }
    Some(result)
}
