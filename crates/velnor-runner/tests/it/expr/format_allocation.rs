// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::expr_support::{env_with, text};
use anyhow::{Context, Result};
use std::fs;
use velnor_runner::expr::RootContext;

const PLACEHOLDERS: usize = 3_000_000;
const ALLOWED_GROWTH_KIB: u64 = 40 * 1024;
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
