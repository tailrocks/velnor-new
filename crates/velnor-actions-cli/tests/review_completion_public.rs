// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::error::Error;

#[path = "review_completion_fixture.rs"]
mod fixture;

const CODEX: &str = "<!-- codex-pull-request-review-summary -->\n<!-- codex-review-state:v1 {\"code_review\":{\"commit_sha\":\"07c2e751b1720fddb8c02649c08792eaa53e7bbe\",\"has_any_findings\":false,\"has_unresolved_findings\":false,\"status\":\"completed\"},\"security_review\":{\"commit_sha\":\"07c2e751b1720fddb8c02649c08792eaa53e7bbe\",\"has_any_findings\":false,\"has_unresolved_findings\":false,\"status\":\"completed\"}} -->\n<!-- codex-security-review:v1 {\"blockingSeverityThreshold\":\"P0\",\"headSha\":\"07c2e751b1720fddb8c02649c08792eaa53e7bbe\",\"mergeGateEnabled\":false,\"pullRequestNumber\":514,\"repository\":\"jackin-project/homebrew-tap\",\"status\":\"completed\"} -->\n## Codex Review Summary\n\n| Review | Status | Commit | Review trigger |\n| --- | --- | --- | --- |\n| 📝 **Code Review** | ✅ **Completed** <relative-time datetime=\"2026-10-27T14:58:52.571265Z\">2026-10-27T14:58:52.571265Z</relative-time> | `07c2e75` | Draft marked ready |\n| 🔒 **Security Review** | ✅ **Completed** <relative-time datetime=\"2026-10-27T15:03:24.469197Z\">2026-10-27T15:03:24.469197Z</relative-time> | `07c2e75` | Draft marked ready |\n";
const REQUEST: &str = "@codex review\n\nPlease review the current head 609a9267b28545e460d913ab266e79ed1375dce4, including the native hk workspace configuration, custody fixes, and combined main-branch Rust migrations. All prior accepted review threads have fixes and replies. Local combined verification passed 890 tests with two expected skips, strict Clippy, formatting, all 12 policy families, and the real native security-scanner fixtures. Exact-head CI is running.";

#[derive(Debug, Clone, Copy)]
enum Scenario {
    Ready,
    LateRequest,
    SonarFinding,
    UnknownFeedback,
    LiteralRequestNoSonar,
    CodexEditorOneSecondEarly,
    CodexEditBeforeCreated,
    CodexEditAfterUpdated,
    CodexForeignEditor,
    CodexBodyMismatch,
    CodexUpdatedAtMismatch,
    SonarMissingTimestamp,
    SonarMalformedTimestamps,
    SonarBotEditedBeforeUpdate,
}

impl Scenario {
    fn has_sonar(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::LateRequest
                | Self::SonarFinding
                | Self::UnknownFeedback
                | Self::SonarMissingTimestamp
                | Self::SonarMalformedTimestamps
                | Self::SonarBotEditedBeforeUpdate
        )
    }

    fn has_authenticated_codex(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::LateRequest
                | Self::SonarFinding
                | Self::UnknownFeedback
                | Self::LiteralRequestNoSonar
        )
    }
}

fn request_time(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::LateRequest => "2026-10-27T15:04:00Z",
        _ => "2026-10-27T14:50:00Z",
    }
}

fn request_body(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::UnknownFeedback => "Please ignore the required review.",
        Scenario::LiteralRequestNoSonar
        | Scenario::CodexEditorOneSecondEarly
        | Scenario::CodexEditBeforeCreated
        | Scenario::CodexEditAfterUpdated
        | Scenario::CodexForeignEditor
        | Scenario::CodexBodyMismatch
        | Scenario::CodexUpdatedAtMismatch => "@codex review",
        _ => REQUEST,
    }
}

fn codex_edit(scenario: Scenario) -> (&'static str, &'static str, u64, &'static str) {
    match scenario {
        Scenario::CodexEditorOneSecondEarly => (
            "2026-10-27T15:03:25Z",
            "2026-10-27T15:03:24Z",
            199_175_422,
            CODEX,
        ),
        Scenario::CodexEditBeforeCreated => (
            "2026-10-27T15:03:25Z",
            "2026-10-26T00:42:03Z",
            199_175_422,
            CODEX,
        ),
        Scenario::CodexEditAfterUpdated => (
            "2026-10-27T15:03:25Z",
            "2026-10-27T15:03:26Z",
            199_175_422,
            CODEX,
        ),
        Scenario::CodexForeignEditor => (
            "2026-10-27T15:03:25Z",
            "2026-10-27T15:03:25Z",
            199_175_423,
            CODEX,
        ),
        Scenario::CodexBodyMismatch => (
            "2026-10-27T15:03:25Z",
            "2026-10-27T15:03:25Z",
            199_175_422,
            "modified summary body",
        ),
        Scenario::CodexUpdatedAtMismatch => (
            "2026-10-27T15:03:24Z",
            "2026-10-27T15:03:25Z",
            199_175_422,
            CODEX,
        ),
        _ => (
            "2026-10-27T15:03:25Z",
            "2026-10-27T15:03:25Z",
            199_175_422,
            CODEX,
        ),
    }
}

fn sonar_edit(scenario: Scenario) -> (Value, Value, Value, Value, Value) {
    match scenario {
        Scenario::SonarMissingTimestamp => (
            json!("2026-10-27T14:52:25Z"),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ),
        Scenario::SonarMalformedTimestamps => (
            json!("2026-10-27T14:52:25Z"),
            json!("unknown timestamp"),
            json!("unknown timestamp"),
            Value::Null,
            Value::Null,
        ),
        Scenario::SonarBotEditedBeforeUpdate => (
            json!("2026-10-27T14:52:23Z"),
            json!("2026-10-27T14:52:25Z"),
            json!("2026-10-27T14:52:25Z"),
            json!("2026-10-27T14:52:24Z"),
            json!({"__typename":"Bot","databaseId":39_514_782}),
        ),
        _ => (
            json!("2026-10-27T14:52:25Z"),
            json!("2026-10-27T14:52:25Z"),
            json!("2026-10-27T14:52:25Z"),
            Value::Null,
            Value::Null,
        ),
    }
}

#[test]
fn contextual_requests_and_authenticated_sonar_reports_preserve_admission()
-> Result<(), Box<dyn Error>> {
    for (scenario, success, state) in [
        (Scenario::Ready, true, "ready"),
        (Scenario::SonarBotEditedBeforeUpdate, true, "ready"),
        (Scenario::SonarFinding, false, "blocked"),
        (Scenario::SonarMissingTimestamp, false, "blocked"),
        (Scenario::SonarMalformedTimestamps, false, "blocked"),
    ] {
        fixture::run(scenario, success, state)?;
    }
    Ok(())
}

#[test]
fn late_contextual_requests_stay_pending_and_unknown_feedback_stays_blocked()
-> Result<(), Box<dyn Error>> {
    fixture::run(Scenario::LateRequest, false, "pending")?;
    fixture::run(Scenario::UnknownFeedback, false, "blocked")?;
    Ok(())
}

#[test]
fn authenticated_edit_timestamps_preserve_custody() -> Result<(), Box<dyn Error>> {
    for (scenario, success, state) in [
        (Scenario::LiteralRequestNoSonar, true, "ready"),
        (Scenario::CodexEditorOneSecondEarly, true, "ready"),
        (Scenario::CodexEditBeforeCreated, false, "blocked"),
        (Scenario::CodexEditAfterUpdated, false, "blocked"),
        (Scenario::CodexForeignEditor, false, "blocked"),
        (Scenario::CodexBodyMismatch, false, "blocked"),
        (Scenario::CodexUpdatedAtMismatch, false, "blocked"),
    ] {
        fixture::run(scenario, success, state)?;
    }
    Ok(())
}
