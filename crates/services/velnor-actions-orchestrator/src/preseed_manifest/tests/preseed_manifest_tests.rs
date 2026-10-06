use super::*;

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

// The writer→shell-verifier round-trip lives in
// `tests/impl_preseed_manifest.rs`: orchestrator `src/` must hold
// zero process-spawn tokens (structural scans), so the `sh` spawn
// runs from the integration binary, which drives the public
// env-driven writer through a scrubbed child re-exec.

/// Writer/renderer contract: one op tag, one manifest name, one env shape.
///
/// The renderer cannot import the orchestrator, so both sides spell
/// these constants; this test pins them equal.
#[test]
fn names_match_renderer_consts() {
    assert_eq!(
        MANIFEST_FILE,
        velnor_actions_workflow_renderer::PRESEED_MANIFEST_FILE
    );
    assert_eq!(
        PRESEED_MANIFEST_OP,
        velnor_actions_workflow_renderer::WRITE_PRESEED_MANIFEST_OPERATION
    );
    assert_eq!(
        PRESEED_BINARY_ENV,
        velnor_actions_workflow_renderer::PRESEED_MANIFEST_BINARY_ENV
    );
    assert_eq!(
        PRESEED_OUT_ENV,
        velnor_actions_workflow_renderer::PRESEED_MANIFEST_OUT_ENV
    );
    assert_eq!(
        PRESEED_TARGET_ENV,
        velnor_actions_workflow_renderer::PRESEED_MANIFEST_TARGET_ENV
    );
    assert_eq!(
        PRESEED_TOOLCHAIN_ENV,
        velnor_actions_workflow_renderer::PRESEED_MANIFEST_TOOLCHAIN_ENV
    );
}
