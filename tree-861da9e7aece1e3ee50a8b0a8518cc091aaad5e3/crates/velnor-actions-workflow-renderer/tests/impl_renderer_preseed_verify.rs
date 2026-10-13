//! Pre-seed manifest verification: fixed script shape plus live tamper cases.
//!
//! Split from `impl_renderer_preseed.rs` (alint `rust-max-lines`): these
//! tests pin the download-side verify script without plan/final fixtures.
use velnor_actions_workflow_renderer::{
    PRESEED_VERIFY_MANIFEST_NAME, RenderError, preseed_manifest_verify_script,
    preseed_manifest_verify_step,
};

/// Expected target triple the generator renders into the verify step.
const TARGET: &str = "x86_64-unknown-linux-gnu";

#[test]
fn preseed_manifest_verify_pins_target_and_digest() -> Result<(), RenderError> {
    let step = preseed_manifest_verify_step(TARGET)?;
    assert_eq!(step.name, PRESEED_VERIFY_MANIFEST_NAME);
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        panic!("verify must be a shell step");
    };
    assert_eq!((run[0].as_str(), run[1].as_str()), ("sh", "-c"));
    let script = preseed_manifest_verify_script(TARGET);
    for token in [
        "schema",
        "commit",
        "target",
        "toolchain",
        "sha256",
        "GITHUB_SHA",
        "sha256sum",
        "velnor/preseed",
        "preseed-manifest.json",
        "line=; rest=;",
        TARGET,
    ] {
        assert!(script.contains(token), "missing {token}:\n{script}");
    }
    for absent in ["$(", "`", "'", "sed", "python", "jq", "test -x"] {
        assert!(!script.contains(absent), "banned {absent}:\n{script}");
    }
    assert!(preseed_manifest_verify_step("not-a-target").is_err());
    Ok(())
}

#[test]
#[cfg(unix)]
fn preseed_verify_script_rejects_tampered_payload() -> Result<(), RenderError> {
    use std::process::Command;
    let script = preseed_manifest_verify_script(TARGET);
    let root = std::env::temp_dir().join(format!("velnor-preseed-verify-{}", std::process::id()));
    let dir = root.join("velnor/preseed");
    std::fs::create_dir_all(&dir)
        .map_err(|err| RenderError::InvalidWorkflow(format!("tmp:{err}")))?;
    let run_case = |commit: &str, target: &str, sha: &str| {
        let manifest = format!(
            "{{\"schema\":1,\"commit\":\"{commit}\",\"target\":\"{target}\",\"toolchain\":\"rust@1.98.1+mbx@1.0.0\",\"sha256\":\"{sha}\"}}"
        );
        std::fs::write(dir.join("preseed-manifest.json"), &manifest).expect("manifest fixture");
        std::fs::write(dir.join("velnor-actions"), []).expect("binary fixture");
        // No chmod: artifact downloads arrive without the exec bit, and the
        // good case must verify in exactly that state (CI run 36749240499).
        // Ambient `line` carries a valid-shaped manifest: the script's
        // `line=; rest=;` init must clear it, so the good case passing
        // below proves inherited shell state is never parsed.
        Command::new("sh")
            .args(["-c", &script])
            .env("RUNNER_TEMP", &root)
            .env("GITHUB_SHA", "f".repeat(40))
            .env(
                "line",
                "{\"schema\":1,\"commit\":\"ffffffffffffffffffffffffffffffffffffffff\",\"target\":\"x86_64-unknown-linux-gnu\",\"toolchain\":\"rust@1.98.1+mbx@1.0.0\",\"sha256\":\"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"}",
            )
            .status()
            .expect("sh")
            .success()
    };
    let run_raw = |body: Option<&str>| {
        std::fs::write(dir.join("velnor-actions"), []).expect("binary fixture");
        match body {
            Some(text) => {
                std::fs::write(dir.join("preseed-manifest.json"), text).expect("manifest fixture");
            }
            None => {
                std::fs::remove_file(dir.join("preseed-manifest.json")).ok();
            }
        }
        // Same ambient valid-shaped `line`: the empty case failing below
        // proves the init clears it (without init, `read` fails on the
        // empty file and the fallback would parse this ambient line).
        Command::new("sh")
            .args(["-c", &script])
            .env("RUNNER_TEMP", &root)
            .env("GITHUB_SHA", "f".repeat(40))
            .env(
                "line",
                "{\"schema\":1,\"commit\":\"ffffffffffffffffffffffffffffffffffffffff\",\"target\":\"x86_64-unknown-linux-gnu\",\"toolchain\":\"rust@1.98.1+mbx@1.0.0\",\"sha256\":\"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"}",
            )
            .status()
            .expect("sh")
            .success()
    };
    let empty_sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let good = run_case(&"f".repeat(40), TARGET, empty_sha);
    let tampered = run_case(&"f".repeat(40), TARGET, &"0".repeat(64));
    let wrong_target = run_case(&"f".repeat(40), "aarch64-apple-darwin", empty_sha);
    let wrong_commit = run_case(&"0".repeat(40), TARGET, empty_sha);
    let missing = run_raw(None);
    let empty = run_raw(Some(""));
    let bad_schema = run_raw(Some("{\"schema\":2}"));
    std::fs::remove_dir_all(&root).ok();
    assert!(good, "good manifest must verify");
    assert!(!tampered, "tampered sha must fail");
    assert!(!wrong_target, "wrong target must fail");
    assert!(!wrong_commit, "wrong commit must fail");
    assert!(!missing, "missing manifest must fail");
    assert!(!empty, "empty manifest must fail");
    assert!(!bad_schema, "schema 2 must fail");
    Ok(())
}
