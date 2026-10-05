//! MBX object action setup and exact Rust/MBX PATH preflight.

use std::collections::BTreeMap;

use super::{CompileDriver, MBX_ACTION_NAME, is_exact_mbx_version};
use crate::RenderError;
use crate::steps::{action_step_with_env, shell_step, validate_uses};
use velnor_actions_contract::cachekey::mbx_cache_generation;
use velnor_actions_contract::{Step, StepKind};

/// Display name for the strict MBX and Rust PATH preflight.
pub const MBX_PREFLIGHT_NAME: &str = "Verify MBX and Rust toolchains";

/// MBX preflight and objects-restore steps for MBX-selected profiles;
/// Cargo yields none.
///
/// The preflight verifies the exact Mise-installed MBX executable and
/// the exact Rustup shim invocation, then adds those installed paths to
/// `GITHUB_PATH`. The action receives no `version` input and normally
/// reuses that executable. Upstream still installs `latest` if its own
/// PATH lookup fails, so the immediately preceding preflight is required.
/// # Errors
pub fn mbx_steps_for_driver(
    uses: &str,
    driver: CompileDriver,
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Option<[Step; 2]>, RenderError> {
    match driver {
        CompileDriver::Cargo => Ok(None),
        CompileDriver::Mbx => {
            if !is_exact_rust_toolchain(rust_toolchain) {
                return Err(RenderError::BadCommand(format!(
                    "bad_rust_toolchain:{rust_toolchain}"
                )));
            }
            if env.contains_key(MBX_CACHE_MODE_ENV) {
                return Err(RenderError::InvalidWorkflow(
                    "mbx_cache_mode_override".to_owned(),
                ));
            }
            let action_env = with_rustup_process_homes(env)?;
            let preflight =
                mbx_path_preflight_step(mbx_version, rust_toolchain, action_env.clone())?;
            let restore = mbx_objects_action_step(uses, mbx_version, rust_toolchain, action_env)?;
            Ok(Some([preflight, restore]))
        }
    }
}

/// Env key the cache backend reads for its restore/save mode.
pub const MBX_CACHE_MODE_ENV: &str = "ACTIONS_CACHE_MODE";
/// Display name of the MBX objects restore step.
pub const MBX_RESTORE_NAME: &str = "Restore MBX objects";
/// Mode that skips the action post. `read` does not permit writes.
pub(crate) const MBX_ACTION_CACHE_MODE: &str = "read";

/// Exact MBX and Rust toolchain preflight before the action's PATH lookup.
///
/// `mise where` resolves both exact catalog installs without loading
/// consumer config. The exact MBX version and the Rustup `+toolchain`
/// probe must succeed before either directory is added to `GITHUB_PATH`.
/// Paths are quoted as single `printf` arguments so spaces remain intact.
/// # Errors
fn mbx_path_preflight_step(
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    let script = [
        "set -eu".to_owned(),
        "[ -n \"$GITHUB_PATH\" ] || { printf '%s\\n' 'GITHUB_PATH is required' >&2; exit 1; }"
            .to_owned(),
        "probe_dir=\"$RUNNER_TEMP/velnor/mbx-preflight\"".to_owned(),
        "mkdir -p \"$probe_dir\"".to_owned(),
        "mbx_probe=\"$probe_dir/mbx-root\"".to_owned(),
        "rust_probe=\"$probe_dir/rust-root\"".to_owned(),
        format!(
            "mise --no-config --no-env --no-hooks where 'mr-boxington@{mbx_version}' > \"$mbx_probe\""
        ),
        format!(
            "mise --no-config --no-env --no-hooks where 'rust@{rust_toolchain}' > \"$rust_probe\""
        ),
        "IFS= read -r mbx_root < \"$mbx_probe\" || { printf '%s\\n' 'Mise returned no MBX install path' >&2; exit 1; }"
            .to_owned(),
        "IFS= read -r rust_root < \"$rust_probe\" || { printf '%s\\n' 'Mise returned no Rust install path' >&2; exit 1; }"
            .to_owned(),
        "[ -n \"$mbx_root\" ] || { printf '%s\\n' 'Mise returned an empty MBX install path' >&2; exit 1; }"
            .to_owned(),
        "[ -n \"$rust_root\" ] || { printf '%s\\n' 'Mise returned an empty Rust install path' >&2; exit 1; }"
            .to_owned(),
        "case \"$mbx_root\" in /*) ;; *) printf '%s\\n' 'Mise returned a non-absolute MBX install path' >&2; exit 1 ;; esac"
            .to_owned(),
        "case \"$rust_root\" in /*) ;; *) printf '%s\\n' 'Mise returned a non-absolute Rust install path' >&2; exit 1 ;; esac"
            .to_owned(),
        "mbx_bin=\"$mbx_root/mbx\"".to_owned(),
        "rustc_bin=\"$rust_root/rustc\"".to_owned(),
        "[ -x \"$mbx_bin\" ] || { printf 'expected executable MBX at %s\\n' \"$mbx_bin\" >&2; exit 1; }"
            .to_owned(),
        "[ -x \"$rustc_bin\" ] || { printf 'expected Rustup shim at %s\\n' \"$rustc_bin\" >&2; exit 1; }"
            .to_owned(),
        "mbx_output_file=\"$probe_dir/mbx-version\"".to_owned(),
        "rustc_output_file=\"$probe_dir/rustc-version\"".to_owned(),
        "\"$mbx_bin\" --version > \"$mbx_output_file\"".to_owned(),
        "IFS= read -r mbx_output < \"$mbx_output_file\" || { printf '%s\\n' 'MBX returned no version' >&2; exit 1; }"
            .to_owned(),
        format!(
            "[ \"$mbx_output\" = 'mbx {mbx_version}' ] || {{ printf 'expected MBX {mbx_version}, got %s\\n' \"$mbx_output\" >&2; exit 1; }}"
        ),
        format!(
            "\"$rustc_bin\" '+{rust_toolchain}' -vV > \"$rustc_output_file\""
        ),
        format!(
            "grep -Fqx 'release: {rust_toolchain}' \"$rustc_output_file\" || {{ printf 'Rustup did not report exact toolchain {rust_toolchain}\\n' >&2; exit 1; }}"
        ),
        "rm -f \"$mbx_probe\" \"$rust_probe\" \"$mbx_output_file\" \"$rustc_output_file\"".to_owned(),
        "rmdir \"$probe_dir\"".to_owned(),
        "printf '%s\\n' \"$rust_root\" \"$mbx_root\" >> \"$GITHUB_PATH\"".to_owned(),
    ]
    .join("; ");
    shell_step(
        MBX_PREFLIGHT_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script],
        env,
    )
}

/// Check that an MBX action's immediately preceding pair is exactly constructed.
pub(super) fn matches_mbx_steps(
    preflight: &Step,
    action: &Step,
    mbx_version: &str,
    rust_toolchain: &str,
) -> bool {
    let StepKind::Shell { env, .. } = &preflight.kind else {
        return false;
    };
    let StepKind::Action { uses, .. } = &action.kind else {
        return false;
    };
    mbx_path_preflight_step(mbx_version, rust_toolchain, env.clone())
        .is_ok_and(|expected| expected == *preflight)
        && mbx_objects_action_step(uses, mbx_version, rust_toolchain, env.clone())
            .is_ok_and(|expected| expected == *action)
}

/// Objects-mode action restore; the prior preflight owns installation.
///
/// Omitting `version` makes the action first reuse the already-validated
/// MBX executable on `PATH`. Upstream action setup installs `latest` if
/// that second lookup misses; the immediately preceding preflight fails
/// for an absent/mismatched install and exposes the exact root for the
/// restore step. The explicit toolchain follows the build's catalog pin.
/// The step-level [`MBX_CACHE_MODE_ENV`] remains `read`, disabling the
/// action's implicit GitHub-cache post while Velnor owns explicit transport.
/// # Errors
fn mbx_objects_action_step(
    uses: &str,
    mbx_version: &str,
    rust_toolchain: &str,
    mut env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    if !uses.starts_with(&format!("{MBX_ACTION_NAME}@")) {
        return Err(RenderError::BadActionRef(format!("not_mbx_action:{uses}")));
    }
    if !is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    if !is_exact_rust_toolchain(rust_toolchain) {
        return Err(RenderError::BadCommand(format!(
            "bad_rust_toolchain:{rust_toolchain}"
        )));
    }
    let with = BTreeMap::from([
        ("github-cache-mode".to_owned(), "objects".to_owned()),
        ("toolchain".to_owned(), rust_toolchain.to_owned()),
        (
            "cache-generation".to_owned(),
            mbx_cache_generation(mbx_version),
        ),
    ]);
    env.insert(
        MBX_CACHE_MODE_ENV.to_owned(),
        MBX_ACTION_CACHE_MODE.to_owned(),
    );
    action_step_with_env(MBX_RESTORE_NAME, uses, with, env)
}

/// Expose native Rustup paths as well as Mise's selected tool homes.
fn with_rustup_process_homes(
    mut env: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, RenderError> {
    crate::toolchain_env::check_toolchain_homes(&env)?;
    let rustup_home = env
        .get("MISE_RUSTUP_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| RenderError::BadCommand("missing_mise_rustup_home".to_owned()))?;
    let cargo_home = env
        .get("MISE_CARGO_HOME")
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| RenderError::BadCommand("missing_mise_cargo_home".to_owned()))?;
    env.insert("RUSTUP_HOME".to_owned(), rustup_home);
    env.insert("CARGO_HOME".to_owned(), cargo_home);
    Ok(env)
}

/// Exact Rust toolchain selectors in the compiled catalog.
fn is_exact_rust_toolchain(toolchain: &str) -> bool {
    let parts: Vec<&str> = toolchain.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}
