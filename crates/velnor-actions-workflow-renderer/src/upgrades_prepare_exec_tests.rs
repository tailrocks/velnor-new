//! Finding 8 and the rest of finding 10, executed: prepare's Rust bump, its
//! toolchain mirror and its rollback, end to end through the patch.

use super::exec_support::{Sandbox, TestResult, text};

/// Files the prepared patch changes, sorted.
fn patched_files(sandbox: &Sandbox) -> TestResult<Vec<String>> {
    let patch = std::fs::read_to_string(sandbox.work.join("out/upgrade.patch"))?;
    let mut files: Vec<String> = patch
        .lines()
        .filter_map(|line| line.strip_prefix("diff --git a/"))
        .filter_map(|rest| rest.split_once(" b/").map(|(path, _)| path.to_owned()))
        .collect();
    files.sort();
    Ok(files)
}

#[test]
fn a_single_quoted_channel_follows_the_rust_bump() -> TestResult {
    // `channel = '1.98.1'` is valid TOML; the double-quote-only rewrite left
    // it behind, generate then failed on the mirror and shipped a half bump.
    let sandbox = Sandbox::new("single-quote")?;
    sandbox.write("rust-toolchain.toml", "[toolchain]\nchannel = '1.98.1'\n")?;
    sandbox.git(&["commit", "--quiet", "-am", "single-quoted channel"])?;
    let output = sandbox.prepare(&[("SANDBOX_RUST_BUMP", "1.99.1")])?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    let outputs = sandbox.outputs();
    assert_eq!(
        outputs.get("changed").map(String::as_str),
        Some("true"),
        "{log}"
    );
    assert_eq!(
        outputs.get("failed").map(String::as_str),
        Some("false"),
        "{log}"
    );
    assert_eq!(
        patched_files(&sandbox)?,
        ["mise.toml", "rust-toolchain.toml"]
    );
    let toolchain = std::fs::read_to_string(sandbox.repo.join("rust-toolchain.toml"))?;
    assert_eq!(toolchain, "[toolchain]\nchannel = '1.99.1'\n");
    Ok(())
}

#[test]
fn any_regenerate_failure_after_the_rust_bump_rolls_back_rust_alone() -> TestResult {
    // Only the catalogue error used to roll Rust back; any other failure
    // shipped the bump without a regenerated `.github/`.
    let sandbox = Sandbox::new("rollback-any")?;
    let output = sandbox.prepare(&[
        ("SANDBOX_RUST_BUMP", "1.99.1"),
        ("SANDBOX_GENERATE_FAILS_FOR", "1.99.1"),
        ("SANDBOX_CARGO_UPDATE_CHANGES", "1"),
    ])?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    let outputs = sandbox.outputs();
    assert_eq!(
        outputs.get("changed").map(String::as_str),
        Some("true"),
        "{log}"
    );
    assert_eq!(
        outputs.get("failed").map(String::as_str),
        Some("true"),
        "{log}"
    );
    assert_eq!(patched_files(&sandbox)?, ["Cargo.lock"], "{log}");
    assert!(
        log.contains(
            "regenerating with the Rust bump failed (velnor-actions: some_new_check_failed"
        ),
        "{log}"
    );
    Ok(())
}

#[test]
fn a_channel_layout_the_rewrite_cannot_follow_rolls_back_rust() -> TestResult {
    let sandbox = Sandbox::new("unrecognised-channel")?;
    // An inline `[toolchain]` table is valid TOML the line rewrite cannot
    // follow: the Rust scope must roll back as one, not ship a half bump.
    sandbox.write(
        "rust-toolchain.toml",
        "toolchain = { channel = \"1.98.1\" }\n",
    )?;
    sandbox.git(&["commit", "--quiet", "-am", "inline toolchain table"])?;
    let output = sandbox.prepare(&[("SANDBOX_RUST_BUMP", "1.99.1")])?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    let outputs = sandbox.outputs();
    assert_eq!(
        outputs.get("failed").map(String::as_str),
        Some("true"),
        "{log}"
    );
    assert_eq!(
        outputs.get("changed").map(String::as_str),
        Some("false"),
        "{log}"
    );
    assert!(log.contains("mise upgrade --bump rust failed"), "{log}");
    assert_eq!(
        std::fs::read_to_string(sandbox.repo.join("mise.toml"))?,
        "[tools]\nrust = \"1.98.1\"\n",
        "the Rust scope rolled back as one"
    );
    Ok(())
}

#[test]
fn a_prepared_patch_passes_the_verify_and_publish_jobs() -> TestResult {
    // Every job of the workflow, executed in order on one upgrade.
    let sandbox = Sandbox::new("end-to-end")?;
    sandbox.release("0.2.0")?;
    let output = sandbox.prepare(&[
        ("SANDBOX_RUST_BUMP", "1.99.1"),
        ("SANDBOX_CARGO_UPDATE_CHANGES", "1"),
    ])?;
    let log = text(&output);
    assert!(output.status.success(), "{log}");
    assert_eq!(
        patched_files(&sandbox)?,
        [
            ".github/workflows/ci.yml",
            ".velnor/release-manifest.json",
            "Cargo.lock",
            "mise.toml",
            "rust-toolchain.toml",
        ]
    );
    std::fs::copy(
        sandbox.work.join("out/upgrade.patch"),
        sandbox.work.join("upgrade.patch"),
    )?;
    std::fs::copy(
        sandbox.work.join("out/summary.md"),
        sandbox.work.join("summary.md"),
    )?;
    sandbox.outputs();
    let run = sandbox.pipeline()?;
    let publish = run.publish.as_ref().ok_or_else(|| run.log())?;
    assert!(publish.status.success(), "{}", run.log());
    assert_eq!(
        sandbox.pushed_file("rust-toolchain.toml")?,
        "[toolchain]\nchannel = \"1.99.1\"\n"
    );
    Ok(())
}
