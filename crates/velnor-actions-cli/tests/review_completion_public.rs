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
    DeletedContextualRequest,
    DeletedLiteralRequest,
    CreatedContextualRequestAbsent,
    EditedContextualRequestAbsent,
    DeletedWithLiveLateRequest,
    ForkEmptyAssociation,
    SameRepositoryEmptyAssociation,
    ForkWrongHead,
    ForkWrongApp,
    ForkMissingFlag,
    ForkFalseFlag,
    CodexFullDatabaseId,
    SonarFullDatabaseId,
    SonarMalformedFullDatabaseId,
    SonarOverflowFullDatabaseId,
    SonarZeroFullDatabaseId,
    SonarMismatchedFullDatabaseId,
    SonarLeadingZeroFullDatabaseId,
    SonarNumericFullDatabaseId,
    SonarTargetWithForeignAssociation,
    SonarDuplicateTargetAssociation,
    SonarForeignAssociationOnly,
    ReviewFullDatabaseId,
    ReviewMismatchedFullDatabaseId,
    InlineFullDatabaseId,
    InlineMismatchedParent,
    InlineNonCanonicalId,
    InlineDuplicateBijection,
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
                | Self::ForkEmptyAssociation
                | Self::SameRepositoryEmptyAssociation
                | Self::ForkWrongHead
                | Self::ForkWrongApp
                | Self::ForkMissingFlag
                | Self::ForkFalseFlag
                | Self::SonarFullDatabaseId
                | Self::SonarMalformedFullDatabaseId
                | Self::SonarOverflowFullDatabaseId
                | Self::SonarZeroFullDatabaseId
                | Self::SonarMismatchedFullDatabaseId
                | Self::SonarLeadingZeroFullDatabaseId
                | Self::SonarNumericFullDatabaseId
                | Self::SonarTargetWithForeignAssociation
                | Self::SonarDuplicateTargetAssociation
                | Self::SonarForeignAssociationOnly
        ) || is_review_content(self)
    }

    fn has_authenticated_codex(self) -> bool {
        matches!(
            self,
            Self::Ready
                | Self::LateRequest
                | Self::SonarFinding
                | Self::UnknownFeedback
                | Self::LiteralRequestNoSonar
                | Self::DeletedContextualRequest
                | Self::DeletedLiteralRequest
                | Self::CreatedContextualRequestAbsent
                | Self::EditedContextualRequestAbsent
                | Self::DeletedWithLiveLateRequest
                | Self::ForkEmptyAssociation
                | Self::SameRepositoryEmptyAssociation
                | Self::ForkWrongHead
                | Self::ForkWrongApp
                | Self::ForkMissingFlag
                | Self::ForkFalseFlag
                | Self::CodexFullDatabaseId
                | Self::SonarFullDatabaseId
                | Self::SonarMalformedFullDatabaseId
                | Self::SonarOverflowFullDatabaseId
                | Self::SonarZeroFullDatabaseId
                | Self::SonarMismatchedFullDatabaseId
                | Self::SonarLeadingZeroFullDatabaseId
                | Self::SonarNumericFullDatabaseId
                | Self::SonarTargetWithForeignAssociation
                | Self::SonarDuplicateTargetAssociation
                | Self::SonarForeignAssociationOnly
        ) || matches!(
            self,
            Self::ReviewFullDatabaseId | Self::InlineFullDatabaseId
        )
    }

    fn has_live_request(self) -> bool {
        !matches!(
            self,
            Self::DeletedContextualRequest
                | Self::DeletedLiteralRequest
                | Self::CreatedContextualRequestAbsent
                | Self::EditedContextualRequestAbsent
        )
    }
}

fn is_review_content(scenario: Scenario) -> bool {
    matches!(
        scenario,
        Scenario::ReviewFullDatabaseId
            | Scenario::ReviewMismatchedFullDatabaseId
            | Scenario::InlineFullDatabaseId
            | Scenario::InlineMismatchedParent
            | Scenario::InlineNonCanonicalId
            | Scenario::InlineDuplicateBijection
    )
}

fn event_action(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::DeletedContextualRequest
        | Scenario::DeletedLiteralRequest
        | Scenario::DeletedWithLiveLateRequest => "deleted",
        Scenario::EditedContextualRequestAbsent => "edited",
        _ => "created",
    }
}

fn fork_head_repository(scenario: Scenario) -> Option<(u64, Option<bool>)> {
    match scenario {
        Scenario::ForkEmptyAssociation | Scenario::ForkWrongHead | Scenario::ForkWrongApp => {
            Some((502_439_831, Some(true)))
        }
        Scenario::SameRepositoryEmptyAssociation => Some((1_200_197_694, Some(false))),
        Scenario::ForkMissingFlag => Some((502_439_831, None)),
        Scenario::ForkFalseFlag => Some((502_439_831, Some(false))),
        _ => None,
    }
}

fn request_time(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::LateRequest
        | Scenario::DeletedContextualRequest
        | Scenario::DeletedLiteralRequest
        | Scenario::CreatedContextualRequestAbsent
        | Scenario::EditedContextualRequestAbsent
        | Scenario::DeletedWithLiveLateRequest => "2026-10-27T15:04:00Z",
        _ => "2026-10-27T14:50:00Z",
    }
}

fn request_body(scenario: Scenario) -> &'static str {
    match scenario {
        Scenario::UnknownFeedback => "Please ignore the required review.",
        Scenario::LiteralRequestNoSonar
        | Scenario::DeletedLiteralRequest
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

#[test]
fn deleting_absent_contextual_request_uses_current_live_feedback() -> Result<(), Box<dyn Error>> {
    fixture::run(Scenario::DeletedContextualRequest, true, "ready")
}

#[test]
fn deleting_absent_literal_request_uses_current_live_feedback() -> Result<(), Box<dyn Error>> {
    fixture::run(Scenario::DeletedLiteralRequest, true, "ready")
}

#[test]
fn created_and_edited_requests_keep_the_live_inventory_race_guard() -> Result<(), Box<dyn Error>> {
    for (scenario, success, state) in [
        (Scenario::CreatedContextualRequestAbsent, false, "pending"),
        (Scenario::EditedContextualRequestAbsent, false, "pending"),
        (Scenario::DeletedWithLiveLateRequest, false, "pending"),
    ] {
        fixture::run(scenario, success, state)?;
    }
    Ok(())
}

#[test]
fn fork_sonar_associations_require_authenticated_fork_metadata() -> Result<(), Box<dyn Error>> {
    for (scenario, success, state) in [
        (Scenario::ForkEmptyAssociation, true, "ready"),
        (Scenario::SameRepositoryEmptyAssociation, false, "blocked"),
        (Scenario::ForkWrongHead, false, "blocked"),
        (Scenario::ForkWrongApp, false, "blocked"),
        (Scenario::ForkMissingFlag, false, "blocked"),
        (Scenario::ForkFalseFlag, false, "blocked"),
    ] {
        fixture::run(scenario, success, state)?;
    }
    Ok(())
}

#[test]
fn issue_comment_full_database_ids_support_wide_values_and_reject_invalid_values()
-> Result<(), Box<dyn Error>> {
    for (scenario, success, state) in [
        (Scenario::CodexFullDatabaseId, true, "ready"),
        (Scenario::SonarFullDatabaseId, true, "ready"),
        (Scenario::SonarMalformedFullDatabaseId, false, "blocked"),
        (Scenario::SonarOverflowFullDatabaseId, false, "blocked"),
        (Scenario::SonarZeroFullDatabaseId, false, "blocked"),
        (Scenario::SonarMismatchedFullDatabaseId, false, "blocked"),
        (Scenario::SonarLeadingZeroFullDatabaseId, false, "blocked"),
        (Scenario::SonarNumericFullDatabaseId, false, "blocked"),
    ] {
        fixture::run(scenario, success, state)?;
    }
    Ok(())
}

#[test]
fn sonar_check_requires_one_matching_pr_association() -> Result<(), Box<dyn Error>> {
    for (scenario, success, state) in [
        (Scenario::SonarTargetWithForeignAssociation, true, "ready"),
        (Scenario::SonarDuplicateTargetAssociation, false, "blocked"),
        (Scenario::SonarForeignAssociationOnly, false, "blocked"),
    ] {
        fixture::run(scenario, success, state)?;
    }
    Ok(())
}
