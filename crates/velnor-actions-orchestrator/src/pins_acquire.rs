//! Digest-bound helper acquisition and its closed path grammar.

use velnor_actions_contract::{ReleaseTarget, Step};
use velnor_actions_workflow_renderer::{
    HelperProvenance, STAGED_BINARY_PREFIX, provision_acquire_step,
};

use crate::OrchestratorError;

/// Digest-verified staging step: fetch URL, check SHA-256, record the source
/// commit, make executable. Both provenance paths supply the commit (F3).
pub(super) fn acquire_step(
    url: &str,
    sha: &str,
    commit: &str,
    staged: &str,
    target: ReleaseTarget,
) -> Result<Step, OrchestratorError> {
    let provenance = HelperProvenance::ReleaseAsset {
        url: url.to_owned(),
        sha256: sha.to_owned(),
        commit: commit.to_owned(),
    };
    Ok(provision_acquire_step(
        &provenance,
        acquire_argv(staged, target)?,
    )?)
}

/// Host path of the read-only generator seed. Not a job input.
const GENERATOR_SEED_ROOT: &str = "/opt/velnor/seed";

/// Fixed acquisition argv. A matching seed file is copied. Otherwise curl.
///
/// Curl stays HTTPS-only (`--proto '=https'`) over TLS 1.2+. Paths are
/// double-quoted, and a seed with the wrong digest is never copied. Curl
/// retries all failures up to five times, including a TLS EOF; certificate
/// verification stays enabled.
/// Both checks inline that target's native digest utility. A shared hash
/// prefix keeps each repeated step inside the 500_000-byte workflow cap.
/// The staged path stays a literal `$RUNNER_TEMP/velnor/bin/velnor-actions-`
/// prefix. `&&` still skips `chmod` when mkdir, copy, download, or the
/// digest check fails.
///
/// # Errors
///
/// Returns [`OrchestratorError::Contract`] when the seed root or staged
/// path is outside its closed safe-path grammar.
pub fn acquire_script_argv(
    staged: &str,
    seed_root: &str,
    target: ReleaseTarget,
) -> Result<Vec<String>, OrchestratorError> {
    let digest = match target {
        ReleaseTarget::LinuxX86_64 => "sha256sum -c -",
        ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "shasum -a 256 -c -",
    };
    if !absolute_token(seed_root) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_seed_root:{seed_root}"),
        });
    }
    if !staged_path_token(staged) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_staged_path:{staged}"),
        });
    }
    let name = staged.rsplit_once('/').map_or(staged, |(_, tail)| tail);
    if !file_token(name) {
        return Err(OrchestratorError::Contract {
            problem: format!("bad_staged_name:{name}"),
        });
    }
    let script = format!(
        "d=\"{staged}\"&&mkdir -p \"${{d%/*}}\"&&s=\"{seed_root}/generator/${{d##*/}}\"&&p=\"$VELNOR_ASSET_SHA256  \"&&if [ -f \"$s\" ]&&echo \"$p$s\"|{digest};then cp \"$s\" \"$d\";else curl -fsSL --retry 5 --retry-all-errors --proto '=https' --tlsv1.2 \"$VELNOR_ASSET_URL\" -o \"$d\"&&echo \"$p$d\"|{digest};fi&&chmod +x \"$d\""
    );
    Ok(vec!["sh".to_owned(), "-c".to_owned(), script])
}

fn acquire_argv(staged: &str, target: ReleaseTarget) -> Result<Vec<String>, OrchestratorError> {
    acquire_script_argv(staged, GENERATOR_SEED_ROOT, target)
}

fn absolute_token(value: &str) -> bool {
    value.starts_with('/')
        && !value.contains("..")
        && !value.contains("//")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
}

/// Allow static absolute test paths or the one emitted runner-temp prefix.
fn staged_path_token(value: &str) -> bool {
    value
        .strip_prefix(STAGED_BINARY_PREFIX)
        .map_or_else(|| absolute_token(value), file_token)
}

fn file_token(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
