//! Arg-parse behavior through the built binary: flags, positionals, codes.

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, git_init, spawn};

#[test]
fn unknown_flags_exit_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-flags")?;
    for args in [
        vec!["--root", "."],
        vec!["plan", "--format", "json"],
        vec!["init", "--force"],
        vec!["generate", "--check"],
        vec!["init", "--ignore"],
        vec!["plan", "--ignore"],
        vec!["generate", "--ignore", "rust"],
    ] {
        let output = spawn(&args, &[], &tmp)?;
        assert_eq!(code(&output), 2, "args {args:?} must be usage errors");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn extra_positionals_exit_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-pos")?;
    for args in [vec!["init", "extra"], vec!["plan", "extra"]] {
        let output = spawn(&args, &[], &tmp)?;
        assert_eq!(code(&output), 2, "args {args:?} must be usage errors");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn generate_missing_flag_value_exits_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-missing")?;
    let output = spawn(&["generate", "--output-dir"], &[], &tmp)?;
    assert_eq!(code(&output), 2);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn bare_invocation_and_unknown_commands_exit_two() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-bare")?;
    git_init(&tmp)?;
    for args in [Vec::<&str>::new(), vec!["bogus"], vec!["__internal"]] {
        let output = spawn(&args, &[], &tmp)?;
        assert_eq!(code(&output), 2, "args {args:?} must be usage errors");
    }
    cleanup(&tmp);
    Ok(())
}

#[test]
fn generate_without_output_dir_parses() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-gen-plain")?;
    git_init(&tmp)?;
    let output = spawn(&["generate"], &[], &tmp)?;
    assert_eq!(code(&output), 1, "parses, then fails on missing config");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn plan_without_config_exits_one() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-plan")?;
    git_init(&tmp)?;
    let output = spawn(&["plan"], &[], &tmp)?;
    assert_eq!(code(&output), 1);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn help_describes_stack_generic_generator() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-help")?;
    let output = spawn(&["--help"], &[], &tmp)?;
    assert_eq!(code(&output), 0);
    let help = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(help.contains("generate GitHub Actions workflows"), "{help}");
    assert!(!help.contains("rust"), "{help}");
    assert!(!help.contains("Rust"), "{help}");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn output_dir_help_guides_unique_tmp_choice() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-gen-help")?;
    let output = spawn(&["generate", "--help"], &[], &tmp)?;
    assert_eq!(code(&output), 0);
    let help = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(help.contains("--output-dir"), "{help}");
    assert!(help.contains("unique"), "{help}");
    assert!(help.contains("/tmp"), "{help}");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn generate_output_dir_flag_parses() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-gen")?;
    git_init(&tmp)?;
    let preview = tmp.join("preview");
    let output = spawn(
        &["generate", "--output-dir", preview.to_str().unwrap_or("/")],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn foundation_preview_requires_destination_and_rejects_mode() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-foundation")?;
    let missing = spawn(&["generate", "--foundation-qualification-only"], &[], &tmp)?;
    assert_eq!(code(&missing), 2);
    let missing_stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(missing_stderr.contains("--output-dir"), "{missing_stderr}");

    let preview_path = tmp.join("preview");
    let preview = preview_path.to_str().unwrap_or("/");
    let conflict = spawn(
        &[
            "generate",
            "--foundation-qualification-only",
            "--output-dir",
            preview,
            "--mode",
            "hosted",
        ],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&conflict), 2);
    let conflict_stderr = String::from_utf8_lossy(&conflict.stderr);
    assert!(conflict_stderr.contains("--mode"), "{conflict_stderr}");

    let accepted = spawn(
        &[
            "generate",
            "--foundation-qualification-only",
            "--output-dir",
            preview,
        ],
        &[],
        &tmp,
    )?;
    assert_ne!(code(&accepted), 2, "valid Foundation args must parse");
    cleanup(&tmp);
    Ok(())
}

#[test]
fn foundation_help_exposes_only_fixed_source_mode() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("args-foundation-help")?;
    let output = spawn(&["generate", "--help"], &[], &tmp)?;
    assert_eq!(code(&output), 0);
    let help = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(help.contains("--foundation-qualification-only"), "{help}");
    assert!(help.contains("--output-dir <PATH>"), "{help}");
    assert!(!help.contains("--foundation-action-ref"), "{help}");

    let unavailable_source = spawn(
        &[
            "generate",
            "--foundation-qualification-only",
            "--output-dir",
            "preview",
            "--foundation-action-ref",
            "caller/repository/action@main",
        ],
        &[],
        &tmp,
    )?;
    assert_eq!(code(&unavailable_source), 2);
    cleanup(&tmp);
    Ok(())
}
