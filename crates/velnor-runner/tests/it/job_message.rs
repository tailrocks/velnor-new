// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result};
use velnor_runner::context_value::ContextValue;
use velnor_runner::job_message::AgentJobRequestMessage;

const FIXTURE: &str = include_str!("../../fixtures/job-message-wire.json");

#[test]
fn job_message_parses_wire_fixture() -> Result<()> {
    let message = AgentJobRequestMessage::parse_json(FIXTURE).context("parse fixture")?;

    assert_eq!(message.job_id, "00000000-0000-0000-0000-000000000001");
    assert_eq!(message.plan.plan_id, "00000000-0000-0000-0000-000000000002");
    assert_eq!(message.steps.len(), 1);
    assert_eq!(message.steps[0].script_text(), Some("echo hello"));
    assert_eq!(message.mask.len(), 1);
    assert_eq!(message.mask[0].value.as_deref(), Some("token-value"));

    let token = message
        .resources
        .endpoints
        .first()
        .and_then(|endpoint| endpoint.authorization.as_ref())
        .and_then(|authorization| authorization.parameter_string("accesstoken"));
    assert_eq!(token, Some("system-token"));

    let contexts = message
        .materialize_context_values()
        .context("materialize context values")?;
    let github = contexts.get("github").context("github context")?;
    assert_eq!(
        github.get("repository"),
        Some(&ContextValue::String("tailrocks/velnor-new".to_owned()))
    );
    Ok(())
}
