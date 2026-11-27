// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{TempDir, hash_outcome, within_the_linear_time_limit};
use anyhow::Result;
use velnor_runner::expr::ExprEnv;

const V8_TEXT_RUN: usize = 32_767;
const NESTING_LIMIT: usize = 4_096;
const FITS: &str = "ok: ";
const TOO_LARGE: &str = "Regular expression too large";
const TOO_DEEP: &str = "nested unclosed character classes";

fn workspace() -> Result<(TempDir, ExprEnv)> {
    let dir = TempDir::new()?;
    dir.write("ws/a.txt", "a")?;
    dir.write("ws/sub/x.txt", "x")?;
    let env = dir.env();
    Ok((dir, env))
}

fn assert_fails_with(outcome: &str, reason: &str, case: &str) {
    assert!(
        outcome.ends_with(reason),
        "{case} should fail with {reason:?}, but the call ended: {outcome:?}"
    );
}

fn wildcards(count: usize) -> String {
    "?".repeat(count)
}

#[test]
fn expr_hash_files_fails_a_run_of_text_past_the_v8_regex_limit() -> Result<()> {
    let (_dir, mut env) = workspace()?;

    assert_eq!(hash_outcome(&mut env, &wildcards(V8_TEXT_RUN))?, FITS);
    assert_fails_with(
        &hash_outcome(&mut env, &wildcards(V8_TEXT_RUN + 1))?,
        TOO_LARGE,
        "one wildcard more than the V8 text run",
    );

    let letters = |count: usize| format!("{}*", "a".repeat(count));
    assert_eq!(hash_outcome(&mut env, &letters(V8_TEXT_RUN))?, FITS);
    assert_fails_with(
        &hash_outcome(&mut env, &letters(V8_TEXT_RUN + 1))?,
        TOO_LARGE,
        "one letter more than the V8 text run",
    );
    Ok(())
}

#[test]
fn expr_hash_files_measures_the_v8_limit_on_each_run_between_stars_and_lookaheads() -> Result<()> {
    let (_dir, mut env) = workspace()?;
    let half = wildcards(V8_TEXT_RUN / 2 + 1);

    assert_eq!(hash_outcome(&mut env, &format!("{half}*{half}"))?, FITS);
    assert_eq!(hash_outcome(&mut env, &format!("{half}[{half}"))?, FITS);
    Ok(())
}

#[test]
fn expr_hash_files_fails_the_v8_limit_only_when_a_path_reaches_the_segment() -> Result<()> {
    let (_dir, mut env) = workspace()?;
    let huge = wildcards(V8_TEXT_RUN + 1);

    assert_eq!(hash_outcome(&mut env, &format!("missing/{huge}"))?, FITS);
    assert_fails_with(
        &hash_outcome(&mut env, &format!("sub/{huge}"))?,
        TOO_LARGE,
        "an oversized segment under a directory that exists",
    );
    Ok(())
}

fn nested(count: usize, before: &str, after: &str) -> String {
    format!("*/{before}{}{after}", "[".repeat(count))
}

#[test]
fn expr_hash_files_caps_nested_unclosed_classes_without_crashing() -> Result<()> {
    let (_dir, mut env) = workspace()?;
    assert_eq!(
        hash_outcome(&mut env, &nested(NESTING_LIMIT, "", ""))?,
        FITS
    );
    assert_fails_with(
        &hash_outcome(&mut env, &nested(NESTING_LIMIT + 1, "", ""))?,
        TOO_DEEP,
        "one unclosed class more than the nesting limit",
    );

    let closed_early = format!("{}]", "a".repeat(1_000));
    let patterns = [
        nested(5_500, "", ""),
        nested(65_000, "", ""),
        nested(65_000, "", "\\]"),
        nested(65_000, "", "\\@]"),
        nested(60_000, &closed_early, ""),
    ];
    for pattern in patterns {
        let message = within_the_linear_time_limit("translating nested classes", move || {
            let (_dir, mut env) = workspace()?;
            hash_outcome(&mut env, &pattern)
        })?;
        assert_fails_with(
            &message,
            TOO_DEEP,
            "a pattern of thousands of unclosed classes",
        );
    }
    Ok(())
}
