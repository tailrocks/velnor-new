// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{Scenario, codex_edit};
use serde_json::{Value, json};
use std::{error::Error, path::Path};

pub(super) fn write_responses(
    state: &Path,
    scenario: Scenario,
    sonar_body: &str,
    sonar_updated: &Value,
    sonar_edited: &Value,
    sonar_editor: &Value,
) -> Result<(), Box<dyn Error>> {
    let (updated_at, last_edited_at, editor, body) = codex_edit(scenario);
    let updated = json!(updated_at);
    let edited = json!(last_edited_at);
    let (codex_comment_id, sonar_comment_id) = comment_ids(scenario);
    let codex_legacy_id = if matches!(scenario, Scenario::CodexFullDatabaseId) {
        Value::Null
    } else {
        json!(codex_comment_id)
    };
    let sonar_legacy_id = if matches!(scenario, Scenario::SonarFullDatabaseId) {
        Value::Null
    } else {
        json!(sonar_comment_id)
    };
    let sonar_full_id = match scenario {
        Scenario::SonarMalformedFullDatabaseId => json!("not-a-decimal-id"),
        Scenario::SonarOverflowFullDatabaseId => json!("18446744073709551616"),
        Scenario::SonarZeroFullDatabaseId => json!("0"),
        Scenario::SonarMismatchedFullDatabaseId => json!("6445194483"),
        Scenario::SonarLeadingZeroFullDatabaseId => json!("06445194482"),
        Scenario::SonarNumericFullDatabaseId => json!(sonar_comment_id),
        _ => json!(sonar_comment_id.to_string()),
    };
    let codex_editor = json!({"__typename":"Bot","databaseId":editor});
    let codex_node = editor_node(body, &updated, &edited, 199_175_422, &codex_editor);
    put_editor_response(
        state,
        "codex-editor.json",
        "databaseId",
        codex_legacy_id,
        codex_node.clone(),
    )?;
    put_editor_response(
        state,
        "codex-full-editor.json",
        "fullDatabaseId",
        json!(codex_comment_id.to_string()),
        codex_node,
    )?;
    if scenario.has_sonar() {
        let sonar_node = editor_node(
            sonar_body,
            sonar_updated,
            sonar_edited,
            39_514_782,
            sonar_editor,
        );
        put_editor_response(
            state,
            "sonar-editor.json",
            "databaseId",
            sonar_legacy_id,
            sonar_node.clone(),
        )?;
        put_editor_response(
            state,
            "sonar-full-editor.json",
            "fullDatabaseId",
            sonar_full_id,
            sonar_node,
        )?;
    }
    Ok(())
}

fn put_editor_response(
    state: &Path,
    name: &str,
    id_field: &str,
    id: Value,
    mut node: Value,
) -> Result<(), Box<dyn Error>> {
    let invalid_integer = id_field == "databaseId" && id.is_null();
    if let Some(fields) = node.as_object_mut() {
        fields.insert(id_field.to_owned(), id);
    }
    let mut response = json!({"data":{"node":node}});
    if invalid_integer {
        response["errors"] =
            json!([{"message":"Int cannot represent this value","path":["node","databaseId"]}]);
    }
    super::put_json(state, name, &response)
}

fn editor_node(
    body: &str,
    updated_at: &Value,
    last_edited_at: &Value,
    author_id: u64,
    editor: &Value,
) -> Value {
    json!({"body":body,"updatedAt":updated_at,"lastEditedAt":last_edited_at,"author":{"__typename":"Bot","databaseId":author_id},"editor":editor})
}

pub(super) fn comment_ids(scenario: Scenario) -> (u64, u64) {
    if matches!(
        scenario,
        Scenario::SonarTargetWithForeignAssociation
            | Scenario::SonarDuplicateTargetAssociation
            | Scenario::SonarForeignAssociationOnly
    ) {
        (6_500, 6_501)
    } else {
        (6_405_682_853, 6_445_194_482)
    }
}
