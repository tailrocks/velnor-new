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
//! There is deliberately NO manifest self-verification step: the manifest
//! and the binary are built from the same PR source, so any digest check
//! the PR performs on its own bytes is circular trust — a trojaned merge
//! would always pass. The manifest is an audit record, not a gate; the
//! trust root is human review of the pre-seed source and workflow.

use velnor_actions_contract::{CRATE_JOB_ID_PREFIX, Job, Step};

use crate::{
    RenderError,
    artifact_paths::{PRESEED_OUTPUT_DIR_EXPR, PRESEED_STAGE_DIR_EXPR},
    render::{FINAL_JOB_ID, PLAN_JOB_ID},
    steps,
};

/// Exact pre-seed helper artifact name (run-scoped, no wildcards).
pub const PRESEED_ARTIFACT_NAME: &str = "velnor-preseed-helper";
/// Directory holding the built helper binary plus its manifest.
///
/// Shell `run:` spelling only; the upload input uses [`PRESEED_OUTPUT_DIR_EXPR`].
pub const PRESEED_OUTPUT_DIR: &str = "$RUNNER_TEMP/velnor/preseed-output";
/// Directory receiving the downloaded pre-seed helper.
///
/// Shell `run:` spelling only; the download input uses [`PRESEED_STAGE_DIR_EXPR`].
pub const PRESEED_STAGE_DIR: &str = "$RUNNER_TEMP/velnor/preseed";
/// Manifest filename inside the pre-seed artifact.
pub const PRESEED_MANIFEST_FILE: &str = "preseed-manifest.json";
/// Fixed `mbx build --release` output, mirrored from the candidate path.
pub const PRESEED_BUILD_OUTPUT: &str = "target/release/velnor-actions";
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
/// Display name of the pre-seed helper staging step.
pub const PRESEED_STAGE_NAME: &str = "Stage helper (pre-seed trust-on-review)";

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
/// so the pre-seed build is byte-identical to the candidate build.
/// # Errors
pub fn preseed_build_step(build: Vec<String>) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_BUILD_NAME.ends_with(TRUST_MARK));
    steps::shell_step(PRESEED_BUILD_NAME, build, std::collections::BTreeMap::new())
}

/// Fixed MBX-compile verification: output executable plus pinned-route proof.
///
/// Beyond the executable output, the step runs the caller-supplied
/// isolated `mbx --version` probe and requires its whole line to equal
/// `mbx <version>` (exact pinned catalog version, never `latest`), so a
/// wrong-toolchain compile fails here instead of uploading. The probe
/// resolves through Mise on every run, cold or warm; the script carries
/// no quotes, substitution, or variables, keeping shellcheck quoting safe.
/// # Errors
pub fn preseed_verify_step(probe: &[String], mbx_version: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_VERIFY_NAME.ends_with(TRUST_MARK));
    if !is_exact_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "preseed_bad_mbx_version:{mbx_version}"
        )));
    }
    check_probe_shape(probe)?;
    let script = format!(
        "test -x {PRESEED_BUILD_OUTPUT} && {} | grep -qxF \"mbx {mbx_version}\"",
        probe.join(" ")
    );
    steps::shell_step(
        PRESEED_VERIFY_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        std::collections::BTreeMap::new(),
    )
}

/// Probe shape: isolated `mise exec <spec@exact> -- mbx --version`.
///
/// First arg `mise`, at least one `spec@exact` tool spec between `exec`
/// and `--`, tail exactly `mbx --version`, every arg shell-plain (no
/// whitespace, quotes, expansions, or operators) so script embedding is
/// injection-free.
fn check_probe_shape(probe: &[String]) -> Result<(), RenderError> {
    let malformed = || RenderError::BadCommand("preseed_bad_mbx_probe".to_owned());
    if probe.len() < 6
        || probe.first().is_some_and(|arg| arg != "mise")
        || probe.iter().any(|arg| !is_probe_token(arg))
    {
        return Err(malformed());
    }
    let (Some(exec), Some(sep)) = (
        probe.iter().position(|arg| arg == "exec"),
        probe.iter().position(|arg| arg == "--"),
    ) else {
        return Err(malformed());
    };
    if exec == 0 || sep <= exec + 1 || probe[1..exec].iter().any(|arg| !arg.starts_with('-')) {
        return Err(malformed());
    }
    if probe[exec + 1..sep]
        .iter()
        .any(|arg| arg.starts_with('-') || !is_exact_tool_spec(arg))
    {
        return Err(malformed());
    }
    if probe[sep + 1..] != ["mbx".to_owned(), "--version".to_owned()] {
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

/// Tool specs pin an exact `major.minor.patch` after `@`.
fn is_exact_tool_spec(spec: &str) -> bool {
    spec.rsplit_once('@')
        .is_some_and(|(_, version)| is_exact_version(version))
}

/// Exact versions: three nonempty numeric dot parts, nothing else.
fn is_exact_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

/// Fixed script writing the pre-seed helper manifest JSON.
///
/// Emits exactly the §4.4 keys: recorded source commit, target triple,
/// toolchain identity, and binary SHA-256 (computed at runtime). The script
/// carries NO single quotes (double-quoted printf format with shell-escaped
/// inner quotes, `read` instead of `cut`): inner quotes would break
/// whole-script quoting and expose `$sha` to shellcheck as SC2154.
#[must_use]
pub fn preseed_manifest_script(target: &str, toolchain: &str) -> String {
    format!(
        "mkdir -p {PRESEED_OUTPUT_DIR} && cp {PRESEED_BUILD_OUTPUT} {PRESEED_OUTPUT_DIR}/velnor-actions && sha256sum {PRESEED_OUTPUT_DIR}/velnor-actions > {PRESEED_OUTPUT_DIR}/sha.txt && read sha rest < {PRESEED_OUTPUT_DIR}/sha.txt && printf \"{{\\\"schema\\\":1,\\\"commit\\\":\\\"%s\\\",\\\"target\\\":\\\"{target}\\\",\\\"toolchain\\\":\\\"{toolchain}\\\",\\\"sha256\\\":\\\"%s\\\"}}\" \"$GITHUB_SHA\" \"$sha\" > {PRESEED_OUTPUT_DIR}/{PRESEED_MANIFEST_FILE}"
    )
}

/// Pre-seed manifest step: toolchain from the fixed build vector.
///
/// Toolchain identity derives from the `tool@exact` specs in `build`
/// (never guessed); the target must be a supported release triple.
/// # Errors
pub fn preseed_manifest_step(build: &[String], target: &str) -> Result<Step, RenderError> {
    debug_assert!(PRESEED_MANIFEST_NAME.ends_with(TRUST_MARK));
    if !velnor_actions_contract::is_supported_target(target) {
        return Err(RenderError::BadCommand(format!(
            "preseed_unsupported_target:{target}"
        )));
    }
    let toolchain = crate::candidate::toolchain_identity(build)?;
    let script = preseed_manifest_script(target, &toolchain);
    steps::shell_step(
        PRESEED_MANIFEST_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        std::collections::BTreeMap::new(),
    )
}

/// Pre-seed helper upload step (exact artifact name, fails loud).
/// # Errors
pub fn preseed_upload_step() -> Result<Step, RenderError> {
    debug_assert!(PRESEED_UPLOAD_NAME.ends_with(TRUST_MARK));
    steps::action_step(
        PRESEED_UPLOAD_NAME,
        steps::UPLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PRESEED_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PRESEED_OUTPUT_DIR_EXPR.to_owned()),
            ("if-no-files-found".to_owned(), "error".to_owned()),
        ]),
    )
}

/// Pre-seed helper download step (exact artifact name, no wildcards).
/// # Errors
pub fn preseed_download_step() -> Result<Step, RenderError> {
    debug_assert!(PRESEED_DOWNLOAD_NAME.ends_with(TRUST_MARK));
    steps::action_step(
        PRESEED_DOWNLOAD_NAME,
        steps::DOWNLOAD_ARTIFACT_USES,
        std::collections::BTreeMap::from([
            ("name".to_owned(), PRESEED_ARTIFACT_NAME.to_owned()),
            ("path".to_owned(), PRESEED_STAGE_DIR_EXPR.to_owned()),
        ]),
    )
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
    steps::shell_step(
        PRESEED_STAGE_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        std::collections::BTreeMap::new(),
    )
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

/// Pre-seed closure: single plan build plus artifact sharing (Gap A).
///
/// In pre-seed mode the plan job must build, upload, and stage the helper
/// while every present task/final job downloads and stages it in that
/// order; anything less would rebuild per job or invoke an unstaged
/// helper. There is no verify step: self-verification would be circular
/// trust (see the module docs). Outside pre-seed mode there is nothing
/// to close over.
/// # Errors
pub(crate) fn check_preseed_closure(
    jobs: &std::collections::BTreeMap<String, Job>,
    preseed: bool,
) -> Result<(), RenderError> {
    if !preseed {
        return Ok(());
    }
    let Some(plan) = jobs.get(PLAN_JOB_ID) else {
        return Ok(());
    };
    for (name, kind) in [
        (PRESEED_BUILD_NAME, "build"),
        (PRESEED_UPLOAD_NAME, "upload"),
        (PRESEED_STAGE_NAME, "stage"),
    ] {
        if !plan.steps.iter().any(|step| step.name == name) {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_incomplete:{PLAN_JOB_ID}:{kind}"
            )));
        }
    }
    for (id, job) in jobs {
        if id != FINAL_JOB_ID && !id.starts_with(CRATE_JOB_ID_PREFIX) {
            continue;
        }
        let position = |name: &str| job.steps.iter().position(|step| step.name == name);
        let (Some(download_at), Some(stage_at)) = (
            position(PRESEED_DOWNLOAD_NAME),
            position(PRESEED_STAGE_NAME),
        ) else {
            let kind = if position(PRESEED_DOWNLOAD_NAME).is_none() {
                "download"
            } else {
                "stage"
            };
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_incomplete:{id}:{kind}"
            )));
        };
        if download_at >= stage_at {
            return Err(RenderError::InvalidWorkflow(format!(
                "preseed_misordered:{id}"
            )));
        }
    }
    Ok(())
}
