// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{env_with, eval, text};
use anyhow::{Context, Result};
use std::fs;
use velnor_runner::expr::{ExprEnv, RootContext};

const PLACEHOLDERS: usize = 3_000_000;
const ALLOWED_GROWTH_KIB: u64 = 40 * 1024;
const EXCEEDING_TEXT_BYTES: usize = 48 * 1024 * 1024;
const PROJECTED_TEXT_BYTES: usize = 40 * 1024 * 1024;
const ALLOWED_EXTRA_GROWTH_KIB: u64 = 16 * 1024;
const RESET_PEAK_RESIDENT_SET: &str = "5";

fn peak_resident_kib() -> Result<u64> {
    let status = fs::read_to_string("/proc/self/status").context("read /proc/self/status")?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|rest| rest.trim().strip_suffix("kB")?.trim().parse().ok())
        .context("find VmHWM in /proc/self/status")
}

#[test]
fn expr_format_keeps_memory_flat_for_a_format_made_of_placeholders() -> Result<()> {
    let format = "{0}".repeat(PLACEHOLDERS);
    let env = env_with(&[(RootContext::Env, &format!(r#"{{"FORMAT":"{format}"}}"#))])?;
    drop(format);
    fs::write("/proc/self/clear_refs", RESET_PEAK_RESIDENT_SET).context("reset the peak")?;
    let before = peak_resident_kib()?;

    let result = text("format(env.FORMAT, '')", &env)?;

    let growth = peak_resident_kib()?.saturating_sub(before);
    assert_eq!(result, "");
    assert!(
        growth <= ALLOWED_GROWTH_KIB,
        "formatting {PLACEHOLDERS} placeholders raised the peak resident set by {growth} KiB, \
         above {ALLOWED_GROWTH_KIB} KiB"
    );
    Ok(())
}

fn peak_growth_kib(expression: &str, env: &ExprEnv) -> Result<(bool, u64)> {
    fs::write("/proc/self/clear_refs", RESET_PEAK_RESIDENT_SET).context("reset the peak")?;
    let before = peak_resident_kib()?;
    let evaluated = eval(expression, env).is_ok();
    Ok((evaluated, peak_resident_kib()?.saturating_sub(before)))
}

#[test]
fn expr_to_json_charges_a_large_string_before_it_builds_it() -> Result<()> {
    let env = env_with(&[(
        RootContext::Env,
        &format!(r#"{{"TEXT":"{}"}}"#, "a".repeat(EXCEEDING_TEXT_BYTES)),
    )])?;

    let (read, lookup) = peak_growth_kib("env.TEXT", &env)?;
    let (rejected, to_json) = peak_growth_kib("toJSON(env.TEXT)", &env)?;

    assert!(read);
    assert!(!rejected);
    assert!(
        to_json <= lookup + ALLOWED_EXTRA_GROWTH_KIB,
        "a rejected toJSON of {EXCEEDING_TEXT_BYTES} bytes raised the peak resident set by \
         {to_json} KiB, more than {ALLOWED_EXTRA_GROWTH_KIB} KiB above the {lookup} KiB the \
         lookup alone needs"
    );
    Ok(())
}

#[test]
fn expr_to_json_charges_a_large_object_key_before_it_copies_it() -> Result<()> {
    let key = "k".repeat(EXCEEDING_TEXT_BYTES);
    let env = env_with(&[(RootContext::Env, &format!(r#"{{"{key}":"v"}}"#))])?;
    drop(key);

    let (evaluated, growth) = peak_growth_kib("toJSON(env)", &env)?;

    assert!(!evaluated);
    assert!(
        growth <= ALLOWED_EXTRA_GROWTH_KIB,
        "a rejected toJSON of an object key of {EXCEEDING_TEXT_BYTES} bytes raised the peak \
         resident set by {growth} KiB, more than {ALLOWED_EXTRA_GROWTH_KIB} KiB"
    );
    Ok(())
}

#[test]
fn expr_wildcard_projection_charges_a_large_string_before_it_copies_it() -> Result<()> {
    let env = env_with(&[(
        RootContext::Env,
        &format!(r#"{{"TEXT":"{}"}}"#, "a".repeat(PROJECTED_TEXT_BYTES)),
    )])?;

    let (evaluated, growth) = peak_growth_kib("env.*", &env)?;

    assert!(!evaluated);
    assert!(
        growth <= ALLOWED_EXTRA_GROWTH_KIB,
        "a rejected projection of a string of {PROJECTED_TEXT_BYTES} bytes raised the peak \
         resident set by {growth} KiB, more than {ALLOWED_EXTRA_GROWTH_KIB} KiB"
    );
    Ok(())
}
