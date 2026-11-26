// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{empty_env, error_text, text, truthy};
use anyhow::{Context, Result};
use serde_json::json;
use velnor_runner::expr::ExprEnv;
use velnor_runner::job_message::AgentJobRequestMessage;

fn env_from_message(context_data: &serde_json::Value) -> Result<ExprEnv> {
    let message = AgentJobRequestMessage::from_value(json!({ "ContextData": context_data }))
        .context("parse message")?;
    let contexts = message.materialize_context_values()?;
    let mut env = empty_env();
    env.set_job_contexts(contexts.expose());
    Ok(env)
}

#[test]
fn expr_vars_reads_job_message() -> Result<()> {
    let env = env_from_message(&json!({
        "github": {"t": 2, "d": [{"k": "event_name", "v": "push"}]},
        "vars": {"t": 2, "d": [
            {"k": "VELNOR_CACHE_DEBUG", "v": "1"},
            {"k": "DEPS_APP_ID", "v": "Iv1.abc"},
        ]},
        "matrix": {"t": 2, "d": [{"k": "os", "v": "linux"}]},
    }))?;

    assert!(truthy("vars.VELNOR_CACHE_DEBUG == '1'", &env)?);
    assert_eq!(text("vars.DEPS_APP_ID", &env)?, "Iv1.abc");
    assert_eq!(text("vars.deps_app_id", &env)?, "Iv1.abc");
    assert_eq!(text("vars['DEPS_APP_ID']", &env)?, "Iv1.abc");
    assert_eq!(text("vars.MISSING", &env)?, "");
    assert!(!truthy("vars.MISSING == '1'", &env)?);
    assert!(truthy("github.event_name == 'push'", &env)?);
    assert_eq!(
        error_text("matrix.os", &env)?,
        "unsupported expression context matrix"
    );

    let without_vars = env_from_message(&json!({}))?;
    assert_eq!(text("vars.VELNOR_CACHE_DEBUG", &without_vars)?, "");
    assert!(!truthy("vars.VELNOR_CACHE_DEBUG == '1'", &without_vars)?);
    Ok(())
}

#[test]
fn expr_job_message_fills_only_service_owned_contexts() -> Result<()> {
    let mut wire: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/job-message-wire.json"))?;
    let context_data = wire["ContextData"]
        .as_object_mut()
        .context("fixture has no ContextData")?;
    context_data.insert(
        "runner".into(),
        json!({"t": 2, "d": [{"k": "os", "v": "Windows"}]}),
    );
    context_data.insert(
        "env".into(),
        json!({"t": 2, "d": [{"k": "X", "v": "from-message"}]}),
    );
    context_data.insert(
        "steps".into(),
        json!({"t": 2, "d": [{"k": "s", "v": {"t": 2, "d": [{"k": "outcome", "v": "success"}]}}]}),
    );
    context_data.insert("needs".into(), json!({"t": 2, "d": [{"k": "build", "v": {"t": 2, "d": [{"k": "result", "v": "success"}]}}]}));
    context_data.insert(
        "inputs".into(),
        json!({"t": 2, "d": [{"k": "mode", "v": "fast"}]}),
    );
    context_data.insert(
        "vars".into(),
        json!({"t": 2, "d": [{"k": "FLAG", "v": "1"}]}),
    );
    let message = AgentJobRequestMessage::from_value(wire).context("parse message")?;
    let contexts = message.materialize_context_values()?;
    let mut env = empty_env();
    env.set_job_contexts(contexts.expose());

    assert_eq!(text("secrets.DEPLOY_KEY", &env)?, "");
    assert_eq!(text("runner.os", &env)?, "");
    assert_eq!(text("env.X", &env)?, "");
    assert_eq!(text("steps.s.outcome", &env)?, "");
    assert_eq!(text("github.repository", &env)?, "tailrocks/velnor-new");
    assert_eq!(text("needs.build.result", &env)?, "success");
    assert_eq!(text("inputs.mode", &env)?, "fast");
    assert_eq!(text("vars.FLAG", &env)?, "1");
    Ok(())
}
