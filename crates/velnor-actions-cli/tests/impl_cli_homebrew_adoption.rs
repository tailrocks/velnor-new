//! Homebrew source-policy, seed, and cutover argument surfaces.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn};

#[test]
fn source_policy_cli_modes_are_validated() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("homebrew-source-policy")?;
    let source_policy = spawn(
        &[
            "homebrew",
            "source-policy",
            "--root",
            "/tmp/project",
            "--output",
            "/tmp/project/.velnor/homebrew-release-policy.json",
            "--help",
        ],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&source_policy), 0, "{:?}", source_policy.stderr);

    let source_check = spawn(
        &[
            "homebrew",
            "source-policy",
            "--root",
            "/tmp/project",
            "--check",
            "--help",
        ],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&source_check), 0, "{:?}", source_check.stderr);

    let missing_mode = spawn(
        &["homebrew", "source-policy", "--root", "/tmp/project"],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&missing_mode), 2, "{:?}", missing_mode.stderr);
    let conflicting_modes = spawn(
        &[
            "homebrew",
            "source-policy",
            "--root",
            "/tmp/project",
            "--check",
            "--output",
            "/tmp/policy.json",
        ],
        &[],
        &tmp,
    )?;
    assert_eq!(
        code(&conflicting_modes),
        2,
        "{:?}",
        conflicting_modes.stderr
    );
    cleanup(&tmp);
    Ok(())
}

#[test]
fn exposes_seed_and_cutover_as_homebrew_subcommands() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("homebrew-adoption")?;
    let seed = spawn(
        &[
            "homebrew",
            "adoption-seed",
            "--transition",
            "jackin-initial-seed",
            "--sdk-manifest",
            "/tmp/release-manifest.json",
            "--output-dir",
            "/tmp/seed-overlay",
            "--help",
        ],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&seed), 0, "{:?}", seed.stderr);

    let cutover = spawn(
        &[
            "homebrew",
            "adoption-cutover",
            "--transition",
            "jackin-initial-seed",
            "--sdk-manifest",
            "/tmp/release-manifest.json",
            "--output-dir",
            "/tmp/cutover-overlay",
            "--help",
        ],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&cutover), 0, "{:?}", cutover.stderr);
    cleanup(&tmp);
    Ok(())
}
