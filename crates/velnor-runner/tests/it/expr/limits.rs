// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{empty_env, error_text, truthy};
use anyhow::{Result, anyhow};
use std::thread;
use velnor_runner::expr::ExprEnv;

const SMALL_STACK_BYTES: usize = 512 * 1024;
const DEPTH_MESSAGE: &str = "Exceeded max expression depth 50";

fn on_small_stack<T: Send + 'static>(
    work: impl FnOnce(ExprEnv) -> Result<T> + Send + 'static,
) -> Result<T> {
    thread::Builder::new()
        .stack_size(SMALL_STACK_BYTES)
        .spawn(move || work(empty_env()))?
        .join()
        .map_err(|_| anyhow!("expression thread panicked"))?
}

#[test]
fn expr_deep_nesting_is_rejected_without_aborting() -> Result<()> {
    on_small_stack(|env| {
        let nots = format!("{}true", "!".repeat(20_990));
        assert_eq!(error_text(&nots, &env)?, DEPTH_MESSAGE);
        let indexes = format!("env.x{}", "[0]".repeat(6_990));
        assert_eq!(error_text(&indexes, &env)?, DEPTH_MESSAGE);
        let calls = format!("{}1{}", "format('{0}', ".repeat(1_300), ")".repeat(1_300));
        assert_eq!(error_text(&calls, &env)?, DEPTH_MESSAGE);
        let groups = format!("{}true{}", "(".repeat(5_000), ")".repeat(5_000));
        assert!(truthy(&groups, &env)?);
        Ok(())
    })
}

#[test]
fn expr_nesting_limit_is_fifty_levels() -> Result<()> {
    let env = empty_env();

    assert!(truthy(&format!("{}true", "!".repeat(48)), &env)?);
    assert!(!truthy(&format!("{}true", "!".repeat(49)), &env)?);
    assert_eq!(
        error_text(&format!("{}true", "!".repeat(50)), &env)?,
        DEPTH_MESSAGE
    );
    Ok(())
}

#[test]
fn expr_length_limit_counts_utf16_units() -> Result<()> {
    let env = empty_env();
    let within = format!("'{}' == ''", "😀".repeat(10_496));
    assert!(!truthy(&within, &env)?);
    let beyond = format!("'{}' == ''", "😀".repeat(10_497));
    assert_eq!(
        error_text(&beyond, &env)?,
        "Exceeded max expression length 21000"
    );
    Ok(())
}
