//! Line-local text evidence scanning for settings and invocations.
//!
//! Covers the `mr_boxington` tool setting, `rustc-wrapper` Cargo config,
//! and driver/runner command mentions in tasks, scripts, and workflows.
//! Structurally resolved inputs (Mise wrappers, Nextest config) build
//! sightings in [`crate::evidence`] instead; this module never guesses
//! structure beyond one comment-stripped line.

use crate::evidence::{
    Evidence, EvidenceFile, EvidenceStrength, is_generated_output, strength_for,
};
use crate::scan::{
    has_adjacent, has_command, has_setting, line_no, snippet, starts_with_word, strip_comment,
};

/// Scan tool-config content for the Rust `mr_boxington` setting.
pub(crate) fn scan_tool_config(file: &EvidenceFile<'_>) -> Vec<Evidence> {
    if is_generated_output(file.content) {
        return Vec::new();
    }
    let strength = strength_for(file, EvidenceStrength::Durable);
    let mut out = Vec::new();
    for (index, line) in file.content.lines().enumerate() {
        let code = strip_comment(line);
        if has_setting(code, "mr_boxington", "true") {
            out.push(sighting(file, index, code, strength));
        }
    }
    out
}

/// Scan Cargo config content for `rustc-wrapper` naming MBX.
pub(crate) fn scan_cargo_config(file: &EvidenceFile<'_>) -> Vec<Evidence> {
    if is_generated_output(file.content) {
        return Vec::new();
    }
    let strength = strength_for(file, EvidenceStrength::Durable);
    let mut out = Vec::new();
    for (index, line) in file.content.lines().enumerate() {
        let code = strip_comment(line);
        if mentions_wrapper(code) && mentions_mbx(code) {
            out.push(sighting(file, index, code, strength));
        }
    }
    out
}

fn mentions_wrapper(code: &str) -> bool {
    code.contains("rustc-wrapper")
        || code.contains("rustc_wrapper")
        || code.contains("RUSTC_WRAPPER")
}

fn mentions_mbx(code: &str) -> bool {
    has_command(code, "mbx") || code.contains("boxington")
}

/// Scan task, script, or workflow text for driver and runner invocations.
///
/// Callers pass [`EvidenceStrength::Durable`] for executable tasks and
/// scripts, [`EvidenceStrength::Transient`] for hand-written workflows;
/// `.github`-rooted files are transient either way.
pub(crate) fn scan_command_text(
    file: &EvidenceFile<'_>,
    base: EvidenceStrength,
) -> (Vec<Evidence>, Vec<Evidence>, Vec<Evidence>) {
    if is_generated_output(file.content) {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let strength = strength_for(file, base);
    let mut mbx = Vec::new();
    let mut nextest = Vec::new();
    let mut cargo_test = Vec::new();
    for (index, line) in file.content.lines().enumerate() {
        let code = strip_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        if invokes_mbx(code) {
            mbx.push(sighting(file, index, code, strength));
        }
        if invokes_nextest(code) {
            nextest.push(sighting(file, index, code, strength));
        }
        if invokes_cargo_test(code) {
            cargo_test.push(sighting(file, index, code, strength));
        }
    }
    (mbx, nextest, cargo_test)
}

fn invokes_mbx(code: &str) -> bool {
    has_command(code, "mbx")
        || has_command(code, "mr-boxington")
        || has_command(code, "mr_boxington")
}

fn invokes_nextest(code: &str) -> bool {
    if has_adjacent(code, "cargo", "nextest") || has_command(code, "cargo-nextest") {
        return true;
    }
    let mut search = code;
    while let Some(pos) = crate::scan::find_word(search, "nextest") {
        let rest = search[pos + "nextest".len()..].trim_start();
        if starts_with_word(rest, "run")
            || starts_with_word(rest, "archive")
            || starts_with_word(rest, "list")
        {
            return true;
        }
        search = &search[pos + "nextest".len()..];
    }
    false
}

fn invokes_cargo_test(code: &str) -> bool {
    has_adjacent(code, "cargo", "test") || has_adjacent(code, "mbx", "test")
}

fn sighting(
    file: &EvidenceFile<'_>,
    index: usize,
    code: &str,
    strength: EvidenceStrength,
) -> Evidence {
    Evidence {
        path: file.path.to_owned(),
        line: line_no(index),
        command_or_setting: snippet(code),
        strength,
    }
}
