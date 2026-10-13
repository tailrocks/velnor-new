//! Explicit pre-seed mode: build-once helper for unseeded Velnor CI.
//!
//! Velnor policy without `.velnor/generator.lock` has no release digest to
//! verify (bootstrap §4.1 pre-seed is trust-on-review). Pre-seed mode makes
//! that explicit: the plan job builds the helper ONCE from the checked-out
//! source with the fixed §4 candidate vector, records the source commit in
//! a manifest, and shares the binary via an exactly-named artifact. Crate
//! and final jobs download that artifact; nothing rebuilds it (Gap A).
//! Every pre-seed step name carries the trust-on-review marker so the
//! weaker provenance is visible in CI logs, not silently implied.
//!
//! Downloaders verify the manifest before staging (same shape as the
//! candidate gate): schema 1, commit equal to this run's checked-out
//! `$GITHUB_SHA`, the generator-rendered expected target, and a sha256
//! recomputed over the downloaded binary. The commit anchor binds staged
//! bytes to this run's checkout (a swapped or cross-run artifact fails
//! closed) and the digest binds them across artifact transit; neither
//! reviews the source itself — a trojaned PR writes a consistent
//! manifest, so the trust root for pre-seed source stays human review
//! of the pre-seed source and workflow, exactly as for candidates.

use std::collections::BTreeMap;

use velnor_actions_contract_workflow::{Step, StepRole};

use velnor_actions_workflow_steps::{
    RenderError,
    artifact_paths::{PRESEED_OUTPUT_DIR_EXPR, PRESEED_STAGE_DIR_EXPR},
    steps,
};

/// Exact pre-seed helper artifact name (run-scoped, no wildcards).
pub const PRESEED_ARTIFACT_NAME: &str = "velnor-preseed-helper";
/// Directory receiving the downloaded pre-seed helper.
///
/// Shell `run:` spelling only; the download input uses [`PRESEED_STAGE_DIR_EXPR`].
pub const PRESEED_STAGE_DIR: &str = "$RUNNER_TEMP/velnor/preseed";
/// Manifest filename inside the pre-seed artifact.
pub const PRESEED_MANIFEST_FILE: &str = "preseed-manifest.json";
/// Fixed `mbx build --release` output, mirrored from the candidate path.
pub const PRESEED_BUILD_OUTPUT: &str = "target/release/velnor-actions";
/// Temporary output from the pinned MBX compile-route check.
const PRESEED_MBX_VERSION_OUTPUT: &str = "$RUNNER_TEMP/velnor/preseed-mbx-version";
/// Downloaded helper binary inside the pre-seed stage directory.
pub const PRESEED_DOWNLOADED_BINARY: &str = "$RUNNER_TEMP/velnor/preseed/velnor-actions";

/// Trust marker suffixed to every pre-seed step name.
const TRUST_MARK: &str = " (pre-seed trust-on-review)";
/// Display name of the one pre-seed helper build step.
pub const PRESEED_BUILD_NAME: &str = "Build helper (pre-seed trust-on-review)";
/// Display name of the MBX-compile verification step.
pub const PRESEED_VERIFY_NAME: &str = "Verify MBX compile (pre-seed trust-on-review)";
/// Display name of the pre-seed manifest step.
pub const PRESEED_MANIFEST_NAME: &str = "Write helper manifest (pre-seed trust-on-review)";
/// Display name of the pre-seed helper upload step.
pub const PRESEED_UPLOAD_NAME: &str = "Upload helper (pre-seed trust-on-review)";
/// Display name of the pre-seed helper download step.
pub const PRESEED_DOWNLOAD_NAME: &str = "Download helper (pre-seed trust-on-review)";
/// Display name of the pre-seed manifest verification step.
pub const PRESEED_VERIFY_MANIFEST_NAME: &str = "Verify helper manifest (pre-seed trust-on-review)";
/// Display name of the pre-seed helper staging step.
pub const PRESEED_STAGE_NAME: &str = "Stage helper (pre-seed trust-on-review)";
/// Env key carrying the fresh binary path to the manifest op.
pub const PRESEED_MANIFEST_BINARY_ENV: &str = "VELNOR_PRESEED_BINARY";
/// Env key carrying the expanded manifest output directory.
pub const PRESEED_MANIFEST_OUT_ENV: &str = "VELNOR_PRESEED_OUT";
/// Env key carrying the literal target triple.
pub const PRESEED_MANIFEST_TARGET_ENV: &str = "VELNOR_PRESEED_TARGET";
/// Env key carrying the toolchain identity from the build vector.
pub const PRESEED_MANIFEST_TOOLCHAIN_ENV: &str = "VELNOR_PRESEED_TOOLCHAIN";

/// Pre-seed staging source: local build output or downloaded artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreseedStageSource {
    /// Plan job: the binary just built by the fixed §4 vector.
    LocalBuild,
    /// Task/final jobs: the binary downloaded from the plan artifact.
    DownloadedArtifact,
}

impl PreseedStageSource {
    /// Fixed source path for this staging origin.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::LocalBuild => PRESEED_BUILD_OUTPUT,
            Self::DownloadedArtifact => PRESEED_DOWNLOADED_BINARY,
        }
    }
}

/// Pre-seed build step over the caller-supplied fixed §4 vector.
///
/// Argv arrives from the orchestrator's shared candidate-build constructor,
/// so the pre-seed build is byte-identical to the candidate build. Env
/// arrives from the caller too: the build must run under the same owned
/// homes as the fetch steps, or it resolves a split cargo home and the
/// restored sources never warm it. The build compiles PR source (build
/// scripts run) after the plan job's prepare step installed the pinned
/// toolchain, so the step carries the scrub overlay plus the unset
/// wrapper and needs no ambient auth.
/// # Errors
pub fn preseed_build_step(
    build: &[String],
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_BUILD_NAME.ends_with(TRUST_MARK));
    let mut step = steps::shell_step(PRESEED_BUILD_NAME, build.to_vec(), env.clone())?;
    step.role = Some(StepRole::PreseedBuild);
    Ok(step)
}

/// Fixed MBX-compile verification: output executable plus pinned-route proof.
///
/// Beyond the executable output, the step runs the caller-supplied
/// isolated `mbx --version` probe and requires its whole line to equal
/// `mbx <version>` (exact pinned catalog version, never `latest`), so a
/// wrong-toolchain compile fails here instead of uploading. The probe
/// resolves through Mise on every run, cold or warm. The script writes
/// probe output to a temporary file before comparing it, so a matching
/// partial response cannot mask a nonzero Mise exit.
/// # Errors
pub fn preseed_verify_step(
    probe: &[String],
    mbx_version: &str,
    env: &BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_VERIFY_NAME.ends_with(TRUST_MARK));
    if !is_exact_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "preseed_bad_mbx_version:{mbx_version}"
        )));
    }
    check_probe_shape(probe)?;
    let script = format!(
        "test -x {PRESEED_BUILD_OUTPUT} && mkdir -p \"$RUNNER_TEMP/velnor\" && {} > \"{PRESEED_MBX_VERSION_OUTPUT}\" && grep -qxF \"mbx {mbx_version}\" \"{PRESEED_MBX_VERSION_OUTPUT}\"",
        probe.join(" ")
    );
    let mut step = steps::shell_step(
        PRESEED_VERIFY_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env.clone(),
    )?;
    step.role = Some(StepRole::PreseedVerifyBuild);
    Ok(step)
}

/// Probe shape: isolated `mise exec rust@<exact> -- mbx --version`.
///
/// The native action owns MBX installation. This route selects only Rust
/// through Mise and inherits the action's PATH; every arg is shell-plain
/// so script embedding is injection-free.
fn check_probe_shape(probe: &[String]) -> Result<(), RenderError> {
    let malformed = || RenderError::BadCommand("preseed_bad_mbx_probe".to_owned());
    if probe.len() != 9
        || probe.iter().any(|arg| !is_probe_token(arg))
        || probe.iter().take(5).map(String::as_str).ne([
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
        ])
        || probe
            .iter()
            .skip(6)
            .map(String::as_str)
            .ne(["--", "mbx", "--version"])
    {
        return Err(malformed());
    }
    let Some(rust_spec) = probe[5].strip_prefix("rust@") else {
        return Err(malformed());
    };
    if !is_exact_version(rust_spec) {
        return Err(malformed());
    }
    Ok(())
}

/// Probe args: nonempty alphanumerics plus `-_.@:/+` only.
fn is_probe_token(arg: &str) -> bool {
    !arg.is_empty()
        && arg.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@' | b':' | b'/' | b'+')
        })
}

/// Exact versions: three nonempty numeric dot parts, nothing else.
fn is_exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Pre-seed manifest step: the fresh helper writes its own manifest.
///
/// The plan job builds the helper from source, then runs the FRESH
/// binary (never a staged or downloaded one) with the manifest op: it
/// hashes its own file, stages the copy, and writes the §4.4 JSON
/// through serde. Writing is not embedding (bootstrap §2 bans only
/// compile-time bakes), so no self-hash paradox arises. Toolchain
/// identity derives from the `tool@exact` specs in `build` (never
/// guessed); the target must be a supported release triple; the output
/// directory uses the runner-temp expression form so no shell expansion
/// is needed. No JSON is composed in shell on this side.
/// # Errors
pub fn preseed_manifest_step(build: &[String], target: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_MANIFEST_NAME.ends_with(TRUST_MARK));
    if !velnor_actions_contract_release::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "preseed_unsupported_target:{target}"
        )));
    }
    let toolchain = crate::candidate::toolchain_identity(build)?;
    let mut step = steps::shell_step(
        PRESEED_MANIFEST_NAME,
        vec![PRESEED_BUILD_OUTPUT.to_owned()],
        std::collections::BTreeMap::from([
            (
                steps::INTERNAL_OP_ENV.to_owned(),
                steps::WRITE_PRESEED_MANIFEST_OPERATION.to_owned(),
            ),
            (
                PRESEED_MANIFEST_BINARY_ENV.to_owned(),
                PRESEED_BUILD_OUTPUT.to_owned(),
            ),
            (
                PRESEED_MANIFEST_OUT_ENV.to_owned(),
                PRESEED_OUTPUT_DIR_EXPR.to_owned(),
            ),
            (PRESEED_MANIFEST_TARGET_ENV.to_owned(), target.to_owned()),
            (PRESEED_MANIFEST_TOOLCHAIN_ENV.to_owned(), toolchain),
        ]),
    )?;
    step.role = Some(StepRole::PreseedManifest);
    Ok(step)
}

/// Pre-seed helper upload step (exact artifact name, fails loud).
/// # Errors
pub fn preseed_upload_step() -> Result<Step, RenderError> {
    debug_assert!(PRESEED_UPLOAD_NAME.ends_with(TRUST_MARK));
    let mut step = steps::action_step(
        PRESEED_UPLOAD_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PRESEED_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PRESEED_OUTPUT_DIR_EXPR.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
            (
                "retention-days".to_owned(),
                steps::ARTIFACT_RETENTION_DAYS.to_string(),
            ),
        ]),
    )?;
    step.role = Some(StepRole::PreseedUpload);
    Ok(step)
}

/// Pre-seed helper download step (exact artifact name, no wildcards).
/// # Errors
pub fn preseed_download_step() -> Result<Step, RenderError> {
    debug_assert!(PRESEED_DOWNLOAD_NAME.ends_with(TRUST_MARK));
    let mut step = steps::action_step(
        PRESEED_DOWNLOAD_NAME,
        steps::DOWNLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PRESEED_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PRESEED_STAGE_DIR_EXPR.to_owned()),
        ]),
    )?;
    step.role = Some(StepRole::PreseedDownload);
    Ok(step)
}

/// Fixed script verifying the downloaded manifest before any staging.
///
/// Same shape as the candidate verification: schema 1, 40-hex commit
/// equal to the checked-out `$GITHUB_SHA`, exact expected target,
/// nonempty toolchain, 64-hex sha256 equal to the downloaded binary's
/// recomputed digest. Existence-only on the binary: artifact downloads
/// do not preserve the exec bit, so `test -x` here would fail closed on
/// every legitimate payload; executability is established by the staging
/// copy's `chmod +x` below. A tampered staged payload fails closed here,
/// so the staging copy below never runs on attacker bytes.
///
/// Contract exception (raw shell parse, kept deliberately): consumers
/// must verify the manifest BEFORE staging, and pre-staging they hold
/// no trusted executor — executing downloaded bytes to verify them
/// would defeat verification, and no pinned parser exists outside the
/// helper. The write side runs in the fresh helper (see
/// [`preseed_manifest_step`]); only this parse side stays shell, and it
/// executes nothing but `test`/`read`/`sha256sum` over fixed paths.
#[must_use]
pub fn preseed_manifest_verify_script(target: &str) -> String {
    format!(
        "line=; rest=; m=\"{PRESEED_STAGE_DIR}/{PRESEED_MANIFEST_FILE}\" && b=\"{PRESEED_STAGE_DIR}/velnor-actions\" && test -f \"$m\" && test -f \"$b\" && read line rest < \"$m\" || [ -n \"$line\" ] && v=${{line#*\\\"schema\\\":}} && v=${{v%%,*}} && [ \"$v\" = 1 ] && c=${{line#*\\\"commit\\\":\\\"}} && c=${{c%%\\\"*}} && [ \"${{#c}}\" = 40 ] && [ \"$c\" = \"$GITHUB_SHA\" ] && t=${{line#*\\\"target\\\":\\\"}} && t=${{t%%\\\"*}} && [ \"$t\" = \"{target}\" ] && tc=${{line#*\\\"toolchain\\\":\\\"}} && tc=${{tc%%\\\"*}} && [ -n \"$tc\" ] && s=${{line#*\\\"sha256\\\":\\\"}} && s=${{s%%\\\"*}} && [ \"${{#s}}\" = 64 ] && sha256sum \"$b\" > \"{PRESEED_STAGE_DIR}/got.txt\" && read got rest < \"{PRESEED_STAGE_DIR}/got.txt\" && [ \"$got\" = \"$s\" ]"
    )
}

/// Manifest verification step; must precede every downloaded staging.
///
/// The expected target is the literal triple the plan job builds for
/// (never the manifest's own claim).
/// # Errors
pub fn preseed_manifest_verify_step(target: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_VERIFY_MANIFEST_NAME.ends_with(TRUST_MARK));
    if !velnor_actions_contract_release::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "preseed_unsupported_target:{target}"
        )));
    }
    let mut step = steps::shell_step(
        PRESEED_VERIFY_MANIFEST_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            preseed_manifest_verify_script(target),
        ],
        std::collections::BTreeMap::new(),
    )?;
    step.role = Some(StepRole::PreseedVerifyManifest);
    Ok(step)
}

/// Pre-seed staging step: copy the helper to the staged binary path.
///
/// The staged path is the same fixed path internal steps invoke, so plan,
/// merge, and freshness steps work unchanged. Only the two fixed origins
/// exist; anything else is not a pre-seed stage.
/// # Errors
pub fn preseed_stage_step(source: PreseedStageSource, staged: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_STAGE_NAME.ends_with(TRUST_MARK));
    validate_staged_path(staged)?;
    let dir = staged.rsplit_once('/').map_or(staged, |(head, _)| head);
    let script = format!(
        "mkdir -p {dir} && cp {} {staged} && chmod +x {staged}",
        source.path()
    );
    let mut step = steps::shell_step(
        PRESEED_STAGE_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        std::collections::BTreeMap::new(),
    )?;
    step.role = Some(StepRole::PreseedStage);
    Ok(step)
}

/// Staged paths stay under the fixed helper prefix without traversal.
fn validate_staged_path(staged: &str) -> Result<(), RenderError> {
    match staged.strip_prefix(steps::STAGED_BINARY_PREFIX) {
        Some(rest)
            if !rest.is_empty()
                && !rest.split('/').any(|seg| seg.is_empty() || seg == "..")
                && !rest.chars().any(|ch| ch.is_whitespace() || ch.is_control())
                && !rest.contains("${{") =>
        {
            Ok(())
        }
        _ => Err(RenderError::BadCommand(format!(
            "preseed_unstaged_binary:{staged}"
        ))),
    }
}
