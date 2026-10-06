//! CLI gate coverage for the `write-preseed-manifest-v1` internal op.
//!
//! Split from `impl_cli_gate.rs` (alint `rust-max-lines`): gate parity for the
//! pre-seed writer op lives here; report-op parity stays in the gate file.
use std::error::Error;

use crate::impl_cli_gate::assert_identical;
use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, spawn_isolated};

#[test]
fn preseed_manifest_op_needs_runner_temp() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-preseed")?;
    let bare = spawn_isolated(&[], &[], &tmp)?;
    let gated = spawn_isolated(
        &[],
        &[("VELNOR_INTERNAL_OP", "write-preseed-manifest-v1")],
        &tmp,
    )?;
    assert_eq!(code(&gated), 2);
    assert_identical(&bare, &gated);
    cleanup(&tmp);
    Ok(())
}

#[test]
fn preseed_manifest_op_writes_manifest_and_copy() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-preseed-run")?;
    let runner = tmp.to_str().unwrap_or("/").to_owned();
    let binary = tmp.join("velnor-actions");
    std::fs::write(&binary, "helper-bytes\n")?;
    let out = tmp.join("velnor").join("preseed-output");
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-preseed-manifest-v1"),
            ("RUNNER_TEMP", runner.as_str()),
            ("GITHUB_SHA", &"c".repeat(40)),
            ("VELNOR_PRESEED_BINARY", binary.to_str().unwrap_or("/")),
            ("VELNOR_PRESEED_OUT", out.to_str().unwrap_or("/")),
            ("VELNOR_PRESEED_TARGET", "x86_64-unknown-linux-gnu"),
            (
                "VELNOR_PRESEED_TOOLCHAIN",
                "rust@1.98.1+mr-boxington@1.21.1",
            ),
        ],
        &tmp,
    )?;
    assert_eq!(
        code(&output),
        0,
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "internal stdout must stay empty");
    assert_eq!(
        std::fs::read_to_string(out.join("preseed-manifest.json"))?,
        format!(
            "{{\"schema\":1,\"commit\":\"{}\",\"target\":\"x86_64-unknown-linux-gnu\",\"toolchain\":\"rust@1.98.1+mr-boxington@1.21.1\",\"sha256\":\"b590a9ae60c41869ef1374d22f8f1cc8046d919250fd9ddf4e06a51c0eef5b1d\"}}",
            "c".repeat(40)
        )
    );
    assert_eq!(
        std::fs::read(out.join("velnor-actions"))?,
        b"helper-bytes\n"
    );
    cleanup(&tmp);
    Ok(())
}

#[test]
fn preseed_manifest_op_without_writer_env_fails_internal_silently() -> Result<(), Box<dyn Error>> {
    let tmp = fresh_tempdir("gate-preseed-fail")?;
    let runner = tmp.to_str().unwrap_or("/").to_owned();
    let output = spawn_isolated(
        &[],
        &[
            ("VELNOR_INTERNAL_OP", "write-preseed-manifest-v1"),
            ("RUNNER_TEMP", runner.as_str()),
        ],
        &tmp,
    )?;
    assert_eq!(code(&output), 1);
    assert!(output.stdout.is_empty(), "internal stdout must stay empty");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("internal request failed"), "{stderr}");
    assert!(!stderr.contains("write-preseed-manifest-v1"), "{stderr}");
    assert!(!stderr.contains("VELNOR_INTERNAL"), "{stderr}");
    cleanup(&tmp);
    Ok(())
}
