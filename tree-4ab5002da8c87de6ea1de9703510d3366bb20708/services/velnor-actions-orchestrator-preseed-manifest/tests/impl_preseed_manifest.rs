use std::path::Path;
use std::process::Command as StdCommand;

use tempfile::TempDir;
use velnor_actions_orchestrator_preseed_manifest::MANIFEST_FILE;
use velnor_actions_orchestrator_preseed_manifest::PRESEED_BINARY_ENV;
use velnor_actions_orchestrator_preseed_manifest::PRESEED_MANIFEST_OP;
use velnor_actions_orchestrator_preseed_manifest::PRESEED_OUT_ENV;
use velnor_actions_orchestrator_preseed_manifest::PRESEED_TARGET_ENV;
use velnor_actions_orchestrator_preseed_manifest::PRESEED_TOOLCHAIN_ENV;
use velnor_actions_orchestrator_preseed_manifest::write_preseed_manifest_to;

/// Scratch dir unique to this process plus the test name.
fn scratch(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-preseed-{test}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

/// Valid writer inputs over a scratch tree with a fixture binary.
fn valid_inputs(
    root: &std::path::Path,
) -> (String, String, String, String, String, std::path::PathBuf) {
    let binary = root.join("velnor-actions");
    std::fs::write(&binary, "helper-bytes\n").expect("fixture");
    let temp = root.join("temp");
    let out = temp.join("velnor").join("preseed-output");
    (
        binary.display().to_string(),
        out.display().to_string(),
        "x86_64-unknown-linux-gnu".to_owned(),
        "rust@1.98.1".to_owned(),
        "a".repeat(40),
        temp,
    )
}

/// Writer emits the exact §4.4 shape with a real digest.
///
/// The digest below is the system `shasum -a 256` of the fixture
/// bytes, not this module's own output: an independent oracle.
#[test]
fn manifest_shape_is_exact_with_real_digest() {
    let root = scratch("shape");
    let (binary, out, target, toolchain, commit, temp) = valid_inputs(&root);
    write_preseed_manifest_to(&binary, &out, &target, &toolchain, &commit, &temp).expect("writer");
    let manifest = std::fs::read_to_string(Path::new(&out).join("preseed-manifest.json"))
        .expect("manifest written");
    assert_eq!(
        manifest,
        format!(
            "{{\"schema\":1,\"commit\":\"{commit}\",\"target\":\"{target}\",\"toolchain\":\"{toolchain}\",\"sha256\":\"b590a9ae60c41869ef1374d22f8f1cc8046d919250fd9ddf4e06a51c0eef5b1d\"}}"
        ),
        "byte-exact §4.4 shape"
    );
    assert_eq!(
        std::fs::read(Path::new(&out).join("velnor-actions")).expect("copy"),
        b"helper-bytes\n",
        "binary copied beside the manifest"
    );
}

/// Every invalid input fails closed with its own problem.
#[test]
fn invalid_inputs_fail_closed() {
    let root = scratch("closed");
    let (binary, out, target, toolchain, commit, temp) = valid_inputs(&root);
    let missing = root.join("missing").display().to_string();
    let elsewhere = root.join("elsewhere").display().to_string();
    let cases = [
        (
            "target",
            binary.clone(),
            out.clone(),
            "wasm32-unknown-unknown".to_owned(),
            toolchain.clone(),
            commit.clone(),
        ),
        (
            "commit",
            binary.clone(),
            out.clone(),
            target.clone(),
            toolchain.clone(),
            "xyz".to_owned(),
        ),
        (
            "toolchain",
            binary.clone(),
            out.clone(),
            target.clone(),
            "has\nnewline".to_owned(),
            commit.clone(),
        ),
        (
            "binary",
            missing,
            out.clone(),
            target.clone(),
            toolchain.clone(),
            commit.clone(),
        ),
        (
            "outside",
            binary.clone(),
            elsewhere,
            target.clone(),
            toolchain.clone(),
            commit.clone(),
        ),
        (
            "relative",
            binary.clone(),
            "relative/out".to_owned(),
            target.clone(),
            toolchain.clone(),
            commit.clone(),
        ),
    ];
    for (case, b, o, t, tc, c) in &cases {
        assert!(
            write_preseed_manifest_to(b, o, t, tc, c, &temp).is_err(),
            "{case} must fail closed"
        );
    }
    let upper = "A".repeat(40);
    for bad in ["", upper.as_str(), "ab cd"] {
        assert!(
            write_preseed_manifest_to(&binary, &out, &target, &toolchain, bad, &temp).is_err(),
            "commit {bad:?} must fail closed"
        );
    }
}

// F19 round-trip: fresh-binary manifest passes the kept-shell verifier.
//
// The writer (Rust) and the verifier (kept-shell exception) must agree
// byte-for-byte. The `sh` spawn cannot live in `src/` (structural
// scans forbid process-spawn tokens there, test modules included), so
// it runs here: a scrubbed child re-exec drives the public env-driven
// writer (child env needs no `unsafe`, which `unsafe_code = "forbid"`
// bars even in tests), then the parent runs the real verifier script
// over a real writer artifact plus a tampered twin.
type TestResult = Result<(), Box<dyn std::error::Error>>;

/// Marker proving the child already carries writer env.
const CHILD_ENV: &str = "VELNOR_TEST_PRESEED_CHILD";

/// Full test path used as the child re-exec filter (unique substring).
const TEST_PATH: &str = "impl_preseed_manifest::fresh_binary_manifest_passes_shell_verifier";

/// Writer artifact passes the verifier; a tampered twin fails closed.
#[test]
#[cfg(unix)]
fn fresh_binary_manifest_passes_shell_verifier() -> TestResult {
    if std::env::var(CHILD_ENV).is_ok() {
        return velnor_actions_orchestrator_preseed_manifest::write_preseed_manifest()
            .map_err(|err| format!("child writer: {err}").into());
    }
    let tmp = TempDir::new()?;
    let root = tmp.path();
    let runner = root.display().to_string();
    let binary = root.join("velnor-actions");
    std::fs::write(&binary, "helper-bytes\n")?;
    let out = root.join("out").display().to_string();
    let target = "x86_64-unknown-linux-gnu";
    let commit = "c".repeat(40);
    let output = StdCommand::new(std::env::current_exe()?)
        .arg(TEST_PATH)
        .env(CHILD_ENV, "1")
        .env("RUNNER_TEMP", &runner)
        .env("GITHUB_SHA", &commit)
        .env("VELNOR_PRESEED_BINARY", binary.display().to_string())
        .env("VELNOR_PRESEED_OUT", &out)
        .env("VELNOR_PRESEED_TARGET", target)
        .env("VELNOR_PRESEED_TOOLCHAIN", "rust@1.98.1")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "child: {stdout}{stderr}");
    assert!(stdout.contains("1 passed"), "child: {stdout}");
    // The verifier addresses the stage dir, not the output dir: relay
    // the artifact the way the upload/download round-trip would.
    let stage = root.join("velnor").join("preseed");
    std::fs::create_dir_all(&stage)?;
    std::fs::copy(
        root.join("out").join("velnor-actions"),
        stage.join("velnor-actions"),
    )?;
    std::fs::copy(
        root.join("out").join("preseed-manifest.json"),
        stage.join("preseed-manifest.json"),
    )?;
    let script = velnor_actions_workflow_jobs::preseed_manifest_verify_script(target);
    let run = || {
        StdCommand::new("sh")
            .args(["-c", &script])
            .env("RUNNER_TEMP", &runner)
            .env("GITHUB_SHA", &commit)
            .status()
            .map(|status| status.success())
    };
    assert!(run()?, "writer artifact must verify");
    std::fs::write(stage.join("velnor-actions"), "tampered-bytes\n")?;
    assert!(!run()?, "tampered binary must fail verification");
    Ok(())
}

/// Writer/renderer contract: one op tag, one manifest name, one env shape.
///
/// The renderer cannot import the orchestrator, so both sides spell
/// these constants; this test pins them equal.
#[test]
fn names_match_renderer_consts() {
    assert_eq!(
        MANIFEST_FILE,
        velnor_actions_workflow_jobs::PRESEED_MANIFEST_FILE
    );
    assert_eq!(
        PRESEED_MANIFEST_OP,
        velnor_actions_workflow_steps::WRITE_PRESEED_MANIFEST_OPERATION
    );
    assert_eq!(
        PRESEED_BINARY_ENV,
        velnor_actions_workflow_jobs::PRESEED_MANIFEST_BINARY_ENV
    );
    assert_eq!(
        PRESEED_OUT_ENV,
        velnor_actions_workflow_jobs::PRESEED_MANIFEST_OUT_ENV
    );
    assert_eq!(
        PRESEED_TARGET_ENV,
        velnor_actions_workflow_jobs::PRESEED_MANIFEST_TARGET_ENV
    );
    assert_eq!(
        PRESEED_TOOLCHAIN_ENV,
        velnor_actions_workflow_jobs::PRESEED_MANIFEST_TOOLCHAIN_ENV
    );
}

/// Operation tag spelling is the event-time contract.
#[test]
fn op_tag_spelling_is_pinned() {
    assert_eq!(PRESEED_MANIFEST_OP, "write-preseed-manifest-v1");
}

/// Env key spellings are the plan-job contract.
#[test]
fn env_key_spellings_are_pinned() {
    assert_eq!(PRESEED_BINARY_ENV, "VELNOR_PRESEED_BINARY");
    assert_eq!(PRESEED_OUT_ENV, "VELNOR_PRESEED_OUT");
    assert_eq!(PRESEED_TARGET_ENV, "VELNOR_PRESEED_TARGET");
    assert_eq!(PRESEED_TOOLCHAIN_ENV, "VELNOR_PRESEED_TOOLCHAIN");
}

/// Manifest filename is the uploader/download contract.
#[test]
fn manifest_file_spelling_is_pinned() {
    assert_eq!(MANIFEST_FILE, "preseed-manifest.json");
}

/// Unsupported targets name the rejected triple.
#[test]
fn unsupported_target_names_the_triple() {
    let root = scratch("target-problem");
    let (binary, out, _, toolchain, commit, temp) = valid_inputs(&root);
    let err = write_preseed_manifest_to(
        &binary,
        &out,
        "wasm32-unknown-unknown",
        &toolchain,
        &commit,
        &temp,
    )
    .expect_err("unsupported target must fail");
    assert_eq!(
        err.to_string(),
        "internal: preseed_unsupported_target:wasm32-unknown-unknown"
    );
}

/// Non-lower-hex commits fail with the commit problem.
#[test]
fn bad_commit_problem_is_pinned() {
    let root = scratch("commit-problem");
    let (binary, out, target, toolchain, _, temp) = valid_inputs(&root);
    let err = write_preseed_manifest_to(&binary, &out, &target, &toolchain, "xyz", &temp)
        .expect_err("bad commit must fail");
    assert_eq!(err.to_string(), "internal: preseed_bad_commit");
}

/// Empty toolchains fail with the toolchain problem.
#[test]
fn empty_toolchain_problem_is_pinned() {
    let root = scratch("toolchain-problem");
    let (binary, out, target, _, commit, temp) = valid_inputs(&root);
    let err = write_preseed_manifest_to(&binary, &out, &target, "", &commit, &temp)
        .expect_err("empty toolchain must fail");
    assert_eq!(err.to_string(), "internal: preseed_bad_toolchain");
}

/// Missing binaries fail before any directory is created.
#[test]
fn missing_binary_problem_is_pinned() {
    let root = scratch("binary-problem");
    let (_, out, target, toolchain, commit, temp) = valid_inputs(&root);
    let missing = root.join("missing").display().to_string();
    let err = write_preseed_manifest_to(&missing, &out, &target, &toolchain, &commit, &temp)
        .expect_err("missing binary must fail");
    assert_eq!(err.to_string(), "internal: preseed_binary_not_a_file");
    assert!(!Path::new(&out).exists(), "no output dir on early failure");
}

/// Outputs outside runner temp fail with the scope problem.
#[test]
fn outside_temp_problem_is_pinned() {
    let root = scratch("outside-problem");
    let (binary, _, target, toolchain, commit, temp) = valid_inputs(&root);
    let elsewhere = root.join("elsewhere").display().to_string();
    let err = write_preseed_manifest_to(&binary, &elsewhere, &target, &toolchain, &commit, &temp)
        .expect_err("outside output must fail");
    assert_eq!(err.to_string(), "internal: preseed_outside_runner_temp");
}

/// Nested output dirs are created by the writer.
#[test]
fn nested_out_dir_is_created() {
    let root = scratch("nested");
    let (binary, _, target, toolchain, commit, temp) = valid_inputs(&root);
    let deep = temp.join("a").join("b").join("c").display().to_string();
    write_preseed_manifest_to(&binary, &deep, &target, &toolchain, &commit, &temp)
        .expect("nested out");
    assert!(
        Path::new(&deep).join(MANIFEST_FILE).is_file(),
        "manifest lands in the created dir"
    );
}

/// The digest tracks the binary bytes; the manifest parses as JSON.
#[test]
fn digest_tracks_binary_bytes() {
    let root = scratch("digest");
    let (binary, out, target, toolchain, commit, temp) = valid_inputs(&root);
    std::fs::write(&binary, "other-bytes\n").expect("rewrite fixture");
    write_preseed_manifest_to(&binary, &out, &target, &toolchain, &commit, &temp).expect("writer");
    let text =
        std::fs::read_to_string(Path::new(&out).join(MANIFEST_FILE)).expect("manifest written");
    let value: serde_json::Value = serde_json::from_str(&text).expect("manifest is JSON");
    assert_eq!(value["schema"], 1);
    assert_eq!(value["commit"], commit.as_str());
    assert_ne!(
        value["sha256"], "b590a9ae60c41869ef1374d22f8f1cc8046d919250fd9ddf4e06a51c0eef5b1d",
        "digest must differ from the other-fixture oracle"
    );
}
