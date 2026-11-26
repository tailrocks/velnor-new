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
