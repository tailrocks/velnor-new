//! Plan-report content: required sections, full fields, forbidden claims.

use std::error::Error;
use std::path::Path;

use crate::impl_cli_tmp::{
    add_crate_pair, cleanup, fresh_tempdir, ignore_rust, init_repo, plan_stdout,
};

/// Build an initialized repo with the `apple`/`zebra` crate pair.
fn pair_repo(prefix: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let tmp = fresh_tempdir(prefix)?;
    init_repo(&tmp)?;
    add_crate_pair(&tmp)?;
    Ok(tmp)
}

/// Machine-readable or volatile tokens that must never appear in a report.
const FORBIDDEN: [&str; 33] = [
    "{",
    "}",
    "```",
    "uses:",
    "run:",
    "jobs:",
    "steps:",
    "push:",
    "strategy:",
    "permissions:",
    "concurrency:",
    "pull_request",
    "fail-fast",
    "shell:",
    "Shell:",
    "$",
    "&&",
    "||",
    "args",
    "Args",
    "command",
    "--",
    "cache key",
    "Cache key",
    "cache-key",
    "digest",
    "sha256",
    "report",
    "Report",
    "202",
    "timestamp",
    "Timestamp",
    "run_key",
];

/// Absence claims about event-time behavior the report must never make.
const NO_CLAIMS: [&str; 11] = [
    "hit",
    "Hit",
    "coverage",
    "Coverage",
    "reuse",
    "Reuse",
    "selected for",
    "will run",
    "will execute",
    "exact PR",
    "PR selection",
];

/// Assert none of `tokens` appears in `text`.
fn assert_absent(text: &str, tokens: &[&str]) {
    for token in tokens {
        assert!(!text.contains(token), "report leaks {token:?}:\n{text}");
    }
}

/// Drop the `Repository:` echo: an OS path, not generator content, whose temp
/// segments could match volatile tokens (`report`, year digits) spuriously.
fn without_repository(text: &str) -> String {
    text.lines()
        .filter(|line| !line.starts_with("Repository:"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn plan_lists_sections_without_yaml_or_json() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-sections")?;
    let stdout = plan_stdout(&tmp)?;
    for section in [
        "Detected stacks",
        "Workflow to generate",
        "Jobs:",
        "Recommendations",
    ] {
        assert!(stdout.contains(section), "missing {section}:\n{stdout}");
    }
    assert_absent(&stdout, &FORBIDDEN[0..13]);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn plan_prints_no_machine_or_volatile_content() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-clean")?;
    assert_absent(&without_repository(&plan_stdout(&tmp)?), &FORBIDDEN);
    let empty = fresh_tempdir("report-clean-empty")?;
    init_repo(&empty)?;
    assert_absent(&without_repository(&plan_stdout(&empty)?), &FORBIDDEN);
    let ignored = pair_repo("report-clean-ignored")?;
    ignore_rust(&ignored)?;
    assert_absent(&without_repository(&plan_stdout(&ignored)?), &FORBIDDEN);
    cleanup(&tmp);
    cleanup(&empty);
    cleanup(&ignored);
    Ok(())
}

#[test]
fn plan_lists_full_field_set() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-fields")?;
    let stdout = plan_stdout(&tmp)?;
    for field in [
        "Detected stacks",
        "Repository:",
        "Rust: selected",
        "Workspace crates: 2",
        "apple (apple/Cargo.toml)",
        "zebra (zebra/Cargo.toml)",
        "Profile apple:",
        "compile driver",
        "test runner",
        ".github/actionlint.yaml",
        ".github/workflows/ci.yml",
        "Push branch: main",
        "Runner: ubuntu-",
        "Jobs:",
        "plan",
        "rust-apple",
        "rust-zebra",
        "required",
        "steps)",
        "2 Rust crate jobs",
        "Entries:",
        "Each:",
        "Parallel:",
        "Cache layers:",
        "Actionlint:",
        "Action pins:",
    ] {
        assert!(stdout.contains(field), "missing {field}:\n{stdout}");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn ignored_plan_keeps_ignore_reasons() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-ignored")?;
    ignore_rust(&tmp)?;
    let stdout = plan_stdout(&tmp)?;
    assert!(
        stdout.contains("Rust: ignored (config stacks.ignore)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("apple/Cargo.toml: ignored (stack_ignored)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("zebra/Cargo.toml: ignored (stack_ignored)"),
        "{stdout}"
    );
    assert!(stdout.contains("Workspace crates: 0"), "{stdout}");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn crates_list_in_stable_package_path_order() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-order")?;
    let first = plan_stdout(&tmp)?;
    let apple = first
        .find("- apple (apple/Cargo.toml)")
        .ok_or("apple line")?;
    let zebra = first
        .find("- zebra (zebra/Cargo.toml)")
        .ok_or("zebra line")?;
    assert!(apple < zebra, "crates out of order:\n{first}");
    assert_eq!(plan_stdout(&tmp)?, first, "plan not deterministic");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn plan_explains_pr_narrowing() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-narrow")?;
    let stdout = plan_stdout(&tmp)?;
    assert!(
        stdout.contains(
            "Pull-request execution narrows crate obligations through its event-time affected-work plan."
        ),
        "{stdout}"
    );
    cleanup(&tmp);
    Ok(())
}

#[test]
fn plan_claims_no_hits_coverage_or_selections() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-noclaim")?;
    assert_absent(&plan_stdout(&tmp)?, &NO_CLAIMS);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn empty_repo_reports_no_work_and_final_check() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("report-nowork")?;
    init_repo(&tmp)?;
    let stdout = plan_stdout(&tmp)?;
    assert!(stdout.contains("no-work workflow"), "{stdout}");
    assert!(stdout.contains("required"), "{stdout}");
    assert!(stdout.contains("Workspace crates: 0"), "{stdout}");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn plan_values_come_from_validated_config() -> Result<(), Box<dyn Error>> {
    let tmp = pair_repo("report-values")?;
    assert!(plan_stdout(&tmp)?.contains("Push branch: main"));
    rewrite_branch(&tmp, "trunk")?;
    let stdout = plan_stdout(&tmp)?;
    assert!(stdout.contains("Push branch: trunk"), "{stdout}");
    assert!(!stdout.contains("Push branch: main"), "{stdout}");
    ignore_rust(&tmp)?;
    assert!(plan_stdout(&tmp)?.contains("ignored (config stacks.ignore)"));
    cleanup(&tmp);
    Ok(())
}

/// Replace the pinned `default_branch` value in an initialized repo.
///
/// Uses the last occurrence: the sample's commented suggestions precede the
/// real pin and must stay untouched.
fn rewrite_branch(repo: &Path, branch: &str) -> Result<(), Box<dyn Error>> {
    let config = repo.join(".velnor").join("config.toml");
    let body = std::fs::read_to_string(&config)?;
    let (head, rest) = body.rsplit_once("default_branch = ").ok_or("branch line")?;
    let tail = rest.split_once('\n').ok_or("branch end")?.1;
    std::fs::write(
        config,
        format!("{head}default_branch = \"{branch}\"\n{tail}"),
    )?;
    Ok(())
}
