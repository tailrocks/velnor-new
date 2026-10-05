//! Fixed shell writers for candidate artifact metadata.

/// Candidate manifest filename inside the uploaded artifact.
pub const CANDIDATE_MANIFEST_FILE: &str = "candidate-manifest.json";
/// Directory holding the built candidate binary plus its manifest.
pub const CANDIDATE_OUTPUT_DIR: &str = "$RUNNER_TEMP/velnor/candidate-output";
/// Directory receiving the downloaded candidate for qualification.
pub const CANDIDATE_STAGE_DIR: &str = "$RUNNER_TEMP/velnor/candidate";
/// Shell spelling of the run-key dir holding the downloaded plan.
pub const RUN_DIR_SHELL: &str = "$RUNNER_TEMP/velnor/r$GITHUB_RUN_ID-a$GITHUB_RUN_ATTEMPT";

/// Fixed script writing the per-target candidate manifest JSON.
///
/// Emits exactly the contract keys: source commit, target triple,
/// toolchain identity, and binary SHA-256 (computed at runtime).
///
/// Contract exception (shell writer, kept deliberately): no trusted
/// executor exists yet in the candidate job — the only Rust binary
/// present is the just-built, still-unverified candidate, which §3
/// verify-before-run forbids running before `candidate_manifest_verify_step`.
#[must_use]
pub fn candidate_manifest_script(target: &str, toolchain: &str) -> String {
    format!(
        "mkdir -p {CANDIDATE_OUTPUT_DIR} && cp target/release/velnor-actions {CANDIDATE_OUTPUT_DIR}/velnor-actions && sha256sum {CANDIDATE_OUTPUT_DIR}/velnor-actions | cut -d' ' -f1 > {CANDIDATE_OUTPUT_DIR}/sha.txt && read sha rest < {CANDIDATE_OUTPUT_DIR}/sha.txt && printf '{{\"schema\":1,\"commit\":\"%s\",\"target\":\"{target}\",\"toolchain\":\"{toolchain}\",\"sha256\":\"%s\"}}' \"$GITHUB_SHA\" \"$sha\" > {CANDIDATE_OUTPUT_DIR}/{CANDIDATE_MANIFEST_FILE}"
    )
}

/// Head-bound candidate attestation writer: `commit` is the plan head.
///
/// Reads the downloaded plan's `head` with the manifest-verify idiom
/// (first `"head":"` match, parameter expansion only, no command
/// substitution) and writes `{"schema":1,"commit":"<head>"}` beside
/// the candidate manifest so the same artifact carries it to the
/// final job. Fails closed when the plan is missing or the head is
/// empty/oversize; the merge re-checks `commit == plan.head` after
/// parsing both sides, so a truncation or misparse can only fail a
/// run, never forge a binding.
///
/// Contract exception (shell writer, kept deliberately): same job,
/// same bar — no trusted executor pre-verification, so the plan head
/// is scraped in shell and re-checked by the merge (see above).
#[must_use]
pub fn candidate_attestation_script() -> String {
    let attestation = velnor_actions_contract::CANDIDATE_ATTESTATION_FILENAME;
    format!(
        "line=; rest=; p=\"{RUN_DIR_SHELL}/plan.json\" && test -f \"$p\" && read line rest < \"$p\" || [ -n \"$line\" ] && h=${{line#*\\\"head\\\":\\\"}} && h=${{h%%\\\"*}} && [ -n \"$h\" ] && [ \"${{#h}}\" -le 64 ] && printf '{{\"schema\":1,\"commit\":\"%s\"}}' \"$h\" > {CANDIDATE_OUTPUT_DIR}/{attestation}"
    )
}
