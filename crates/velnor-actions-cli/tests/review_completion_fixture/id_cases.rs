// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn issue_comment_full_database_ids_support_wide_values_and_reject_invalid_values()
-> Result<(), Box<dyn Error>> {
    fixture::with_test(
        "id_cases::issue_comment_full_database_ids_support_wide_values_and_reject_invalid_values",
        || {
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
        },
    )
}

#[test]
fn sonar_check_requires_one_matching_pr_association() -> Result<(), Box<dyn Error>> {
    fixture::with_test(
        "id_cases::sonar_check_requires_one_matching_pr_association",
        || {
            for (scenario, success, state) in [
                (Scenario::SonarTargetWithForeignAssociation, true, "ready"),
                (Scenario::SonarDuplicateTargetAssociation, false, "blocked"),
                (Scenario::SonarForeignAssociationOnly, false, "blocked"),
            ] {
                fixture::run(scenario, success, state)?;
            }
            Ok(())
        },
    )
}
