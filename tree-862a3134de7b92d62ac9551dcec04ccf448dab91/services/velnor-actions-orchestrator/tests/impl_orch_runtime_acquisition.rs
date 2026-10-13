//! Execution-bound acquisition preserves the offline analysis boundary.

use crate::impl_common::TestResult;

pub(super) fn scrub_bound_acquisition(name: &str, code: &str) -> String {
    if name != "check_tool_acquire.rs" {
        return code.to_owned();
    }
    let mut output = String::new();
    let mut identifier = String::new();
    for character in code.chars() {
        // Non-ASCII stays attached, including Rust identifier combining marks.
        // Invalid non-ASCII tokens also fail closed rather than widening access.
        if !character.is_ascii() || character.is_ascii_alphanumeric() || character == '_' {
            identifier.push(character);
        } else {
            retain_unapproved_identifier(&identifier, &mut output);
            identifier.clear();
            output.push(character);
        }
    }
    retain_unapproved_identifier(&identifier, &mut output);
    output
}

fn retain_unapproved_identifier(identifier: &str, output: &mut String) {
    if !matches!(identifier, "fetch_extract" | "qualified_fetch_command") {
        output.push_str(identifier);
    }
}

#[test]
fn acquisition_exception_is_exact_and_runtime_only() {
    let approved = "fetch_extract qualified_fetch_command";
    assert_eq!(
        scrub_bound_acquisition("check_tool_acquire.rs", approved),
        " "
    );
    for name in ["deps.rs", "analyze.rs", "internal_plan.rs"] {
        assert_eq!(scrub_bound_acquisition(name, approved), approved);
    }
    assert!(scrub_bound_acquisition("check_tool_acquire.rs", "cargo fetch").contains("fetch"));
    for near_miss in [
        "unqualified_fetch_command",
        "qualified_fetch_command_unchecked",
        "fetch_extract_unchecked",
        "unchecked_fetch_extract",
        "λfetch_extract",
        "fetch_extract\u{0301}",
        "λqualified_fetch_command",
        "qualified_fetch_command\u{0301}",
    ] {
        assert_eq!(
            scrub_bound_acquisition("check_tool_acquire.rs", near_miss),
            near_miss
        );
    }
}

#[test]
fn acquisition_requires_bound_plan_and_verified_archive() -> TestResult {
    let runtime = std::fs::read_to_string(super::family_file("execute.rs")?)?;
    assert_order(
        &runtime,
        &[
            "velnor_actions_orchestrator_runtime_plan::binding::bind_check(",
            "let outcome = run_check(root, temp, &item, &plan, deadline);",
        ],
    )?;
    let preparation =
        std::fs::read_to_string(super::orch_src().join("check_runtime/preparation.rs"))?;
    assert!(preparation.contains("mod acquisition;"));
    assert!(preparation.contains("acquisition::acquire(&qualified, check, home, deadline)?;"));
    let acquisition = std::fs::read_to_string(
        super::orch_src().join("check_runtime/preparation/acquisition.rs"),
    )?;
    assert!(acquisition.contains("pub(super) fn acquire("));
    assert!(!acquisition.contains("std::process::Command"));
    assert_order(
        &acquisition,
        &[
            "handle.qualified_fetch_command(&tool.id, dependency, index)",
            "read_with_deadline(&downloaded, 1024 * 1024 * 1024, deadline)",
            "sha256_with_deadline(&bytes, deadline)? != artifact.sha256",
            "write_exclusive_until(",
            "archive::extract_archive(&verified, &root, &artifact.url, expanded, deadline)",
        ],
    )?;
    Ok(())
}

fn assert_order(source: &str, tokens: &[&str]) -> TestResult {
    let mut remaining = source;
    for token in tokens {
        let position = remaining
            .find(token)
            .ok_or_else(|| format!("missing ordered boundary: {token}"))?;
        remaining = &remaining[position + token.len()..];
    }
    Ok(())
}
