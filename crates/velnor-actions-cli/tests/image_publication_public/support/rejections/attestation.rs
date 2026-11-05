// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use serde_json::{Value, json};

use super::Fixture;

pub(crate) fn current_attestation(fixture: &Fixture) -> Result<Value, Box<dyn Error>> {
    fixture
        .attestation_output()?
        .as_array()
        .and_then(|entries| entries.first())
        .cloned()
        .ok_or_else(|| "fixture has no current attestation".into())
}

pub(crate) fn with_invocation(attestation: &Value, run_id: u64, run_attempt: u32) -> Value {
    with_uri(
        attestation,
        &format!(
            "https://github.com/tailrocks/velnor-new/actions/runs/{run_id}/attempts/{run_attempt}"
        ),
    )
}

pub(crate) fn with_uri(attestation: &Value, uri: &str) -> Value {
    let mut historical = attestation.clone();
    historical["verificationResult"]["signature"]["certificate"]["runInvocationURI"] = json!(uri);
    historical
}

pub(crate) fn invalid_invocation_uri(fixture: &Fixture, case: &str) -> String {
    let prefix = "https://github.com/tailrocks/velnor-new/actions/runs";
    let older_run = fixture.profile.run_id() - 1;
    match case {
        "foreign" => "https://github.com/other/repo/actions/runs/1/attempts/1".to_owned(),
        "query" => format!("{prefix}/{older_run}/attempts/1?query=1"),
        "fragment" => format!("{prefix}/{older_run}/attempts/1#fragment"),
        "trailing_slash" => format!("{prefix}/{older_run}/attempts/1/"),
        "zero_run" => format!("{prefix}/0/attempts/1"),
        "zero_attempt" => format!("{prefix}/{older_run}/attempts/0"),
        "leading_zero_run" => format!("{prefix}/0{older_run}/attempts/1"),
        "leading_zero_attempt" => format!("{prefix}/{older_run}/attempts/01"),
        "overflow_run" => format!("{prefix}/18446744073709551616/attempts/1"),
        "overflow_attempt" => format!("{prefix}/{older_run}/attempts/4294967296"),
        _ => String::new(),
    }
}
