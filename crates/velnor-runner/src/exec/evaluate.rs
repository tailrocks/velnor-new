// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::panic::resume_unwind;

use serde_json::Value as Json;

use super::token;
use crate::expr::{ExprEnv, ExprError, evaluate_condition, evaluate_template};

async fn offload<T, F>(work: F) -> Result<T, ExprError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, ExprError> + Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => match error.try_into_panic() {
            Ok(payload) => resume_unwind(payload),
            Err(_) => Err(ExprError::evaluation("expression evaluation was abandoned")),
        },
    }
}

pub(super) async fn scalar(token: &Json, env: &ExprEnv) -> Result<String, ExprError> {
    let (token, env) = (token.clone(), env.clone());
    offload(move || token::scalar(&token, &env)).await
}

pub(super) async fn mapping(
    token: &Json,
    env: &ExprEnv,
) -> Result<Vec<(String, String)>, ExprError> {
    let (token, env) = (token.clone(), env.clone());
    offload(move || token::mapping(&token, &env)).await
}

pub(super) async fn input(
    inputs: Option<&Json>,
    name: &str,
    env: &ExprEnv,
) -> Result<Option<String>, ExprError> {
    let (inputs, name, env) = (inputs.cloned(), name.to_owned(), env.clone());
    offload(move || token::input(inputs.as_ref(), &name, &env)).await
}

pub(super) async fn boolean(token: &Json, env: &ExprEnv) -> Result<bool, ExprError> {
    let (token, env) = (token.clone(), env.clone());
    offload(move || token::boolean(&token, &env)).await
}

pub(super) async fn condition(source: &str, env: &ExprEnv) -> Result<bool, ExprError> {
    let (source, env) = (source.to_owned(), env.clone());
    offload(move || evaluate_condition(&source, &env)).await
}

pub(super) async fn template(source: &str, env: &ExprEnv) -> Result<String, ExprError> {
    let (source, env) = (source.to_owned(), env.clone());
    offload(move || evaluate_template(&source, &env)).await
}
