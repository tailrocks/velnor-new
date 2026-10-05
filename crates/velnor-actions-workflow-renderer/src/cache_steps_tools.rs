//! Canonical tool-cache payload, pins, and restore/save templates.
//!
//! One typed ordered path list drives both pinned cache actions. The
//! action-managed Mise bootstrap is a separate cached file and is verified
//! before Setup Mise can execute it.

use velnor_actions_contract::Step;

use crate::RenderError;

/// Pinned `actions/cache/restore` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_RESTORE_USES: &str =
    "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Pinned `actions/cache/save` ref (v6.1.0, qualified 2026-09-28).
pub const TOOLS_SAVE_USES: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
/// Display name of the tools restore step.
pub const TOOLS_RESTORE_NAME: &str = "Restore Mise tools";
/// Display name of the runner-image metadata step.
pub const TOOLS_IMAGE_IDENTITY_NAME: &str = "Resolve tool-cache image";
/// Display name of the tools save step.
pub const TOOLS_SAVE_NAME: &str = "Save Mise tools";
/// Owned image OS value in later action expressions.
pub const TOOLS_IMAGE_OS_ENV: &str = "VELNOR_CACHE_IMAGE_OS";
/// Owned image revision value in later action expressions.
pub const TOOLS_IMAGE_VERSION_ENV: &str = "VELNOR_CACHE_IMAGE_VERSION";
/// Owned compatibility flag. Only the literal `true` enables tool caching.
pub const TOOLS_CACHE_ELIGIBLE_ENV: &str = "VELNOR_CACHE_IMAGE_ELIGIBLE";
/// Exact restore predicate; an unknown image remains on the cold path.
pub const TOOLS_CACHE_RESTORE_CONDITION: &str = "env.VELNOR_CACHE_IMAGE_ELIGIBLE == 'true'";
/// Exact compatibility predicate appended to the shared trusted-save rule.
pub const TOOLS_CACHE_SAVE_CONDITION: &str = "env.VELNOR_CACHE_IMAGE_ELIGIBLE == 'true'";
/// Separate pinned-action home for its Mise bootstrap binary.
pub const TOOLS_MISE_BOOTSTRAP_DATA_DIR: &str = "${{ runner.temp }}/velnor/mise-bootstrap";
/// Complete isolated Mise data root used by every pinned tool invocation.
/// This includes installs, backend data, downloads, and required metadata.
pub const TOOLS_MISE_DATA_DIR: &str = "${{ runner.temp }}/velnor/mise";
/// Verified Mise bootstrap executable cached as one file.
pub const TOOLS_MISE_BOOTSTRAP_BINARY: &str = "${{ runner.temp }}/velnor/mise-bootstrap/bin/mise";
/// Prefix for the complete typed tool-cache payload and key schema.
pub const TOOLS_KEY_PREFIX: &str = "mise-v3";
/// Complete ordered tool payload. Restore and save consume this exact list.
pub const TOOLS_CACHE_PATHS: [&str; 6] = [
    TOOLS_MISE_BOOTSTRAP_BINARY,
    TOOLS_MISE_DATA_DIR,
    "${{ runner.temp }}/velnor/rustup",
    "${{ runner.temp }}/velnor/cargo/bin",
    "${{ runner.temp }}/velnor/cargo/.crates.toml",
    "${{ runner.temp }}/velnor/cargo/.crates2.json",
];

/// Tool paths as one stable multiline action input.
#[must_use]
pub fn tools_cache_path_input() -> String {
    TOOLS_CACHE_PATHS.join("\n")
}

/// Build the pre-restore image identity probe.
///
/// Missing, unknown, or malformed runner metadata writes an explicit false
/// eligibility flag. It never becomes a shared cache key; Setup Mise and
/// tool verification continue through the normal cold path.
/// # Errors
pub fn tools_cache_image_identity_step() -> Result<Step, RenderError> {
    let script = format!(
        "set -eu\ncache_env=\"${{GITHUB_ENV-}}\"\ncase \"$cache_env\" in /*) ;; *) printf '%s\\n' 'GITHUB_ENV unavailable; cannot safely gate tool cache' >&2; exit 1 ;; esac\nimage_os=\"${{ImageOS-}}\"\nimage_version=\"${{ImageVersion-}}\"\neligible=true\nfor component in \"$image_os\" \"$image_version\"; do case \"$component\" in ''|*[Uu][Nn][Kk][Nn][Oo][Ww][Nn]*|*[Uu][Nn][Oo][Bb][Ss][Ee][Rr][Vv][Ee][Dd]*|*[Ss][Ee][Ll][Ff]-[Hh][Oo][Ss][Tt][Ee][Dd]*|*[Ll][Aa][Tt][Ee][Ss][Tt]*|*[Ss][Tt][Aa][Bb][Ll][Ee]*|*[Cc][Uu][Rr][Rr][Ee][Nn][Tt]*|*[Dd][Ee][Ff][Aa][Uu][Ll][Tt]*|*[Uu][Nn][Aa][Vv][Aa][Ii][Ll][Aa][Bb][Ll][Ee]*|*[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-]*|[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789]*|*[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789]) eligible=false ;; esac; done\nif [ \"$eligible\" != true ]; then image_os=unknown; image_version=unknown; eligible=false; fi\nprintf '{}=%s\\n{}=%s\\n{}=%s\\n' \"$image_os\" \"$image_version\" \"$eligible\" >> \"$cache_env\"",
        TOOLS_IMAGE_OS_ENV, TOOLS_IMAGE_VERSION_ENV, TOOLS_CACHE_ELIGIBLE_ENV
    );
    crate::owned_script::owned_bash_script_step(
        TOOLS_IMAGE_IDENTITY_NAME,
        &script,
        std::collections::BTreeMap::new(),
    )
}

/// Tools restore step over the pinned restore action.
/// # Errors
pub fn tools_restore_step(key: &str) -> Result<Step, RenderError> {
    let step =
        super::tools_cache_action_step(true, TOOLS_RESTORE_USES, key, &tool_payload_paths())?;
    let mut step = rename_step(step, TOOLS_RESTORE_NAME)?;
    step.condition = Some(TOOLS_CACHE_RESTORE_CONDITION.to_owned());
    Ok(step)
}

/// Tools save step over the pinned save action.
/// # Errors
pub fn tools_save_step(key: &str) -> Result<Step, RenderError> {
    let step = super::tools_cache_action_step(false, TOOLS_SAVE_USES, key, &tool_payload_paths())?;
    let mut step = rename_step(step, TOOLS_SAVE_NAME)?;
    step.condition = Some(format!(
        "{} && {}",
        velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION,
        TOOLS_CACHE_SAVE_CONDITION
    ));
    Ok(step)
}

/// One conversion site for the shared typed payload.
fn tool_payload_paths() -> Vec<String> {
    TOOLS_CACHE_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect()
}

/// True only for one canonical owned tool-cache path.
#[must_use]
pub fn is_tools_cache_path(path: &str) -> bool {
    TOOLS_CACHE_PATHS.contains(&path)
}

/// True only for keys generated with all runner and image identity fields.
#[must_use]
pub fn is_tools_cache_key(key: &str) -> bool {
    let prefix = format!(
        "{}-${{{{ runner.os }}}}-${{{{ runner.arch }}}}-${{{{ env.VELNOR_CACHE_IMAGE_OS }}}}-${{{{ env.VELNOR_CACHE_IMAGE_VERSION }}}}-",
        TOOLS_KEY_PREFIX
    );
    let Some(suffix) = key.strip_prefix(&prefix) else {
        return false;
    };
    velnor_actions_contract::SUPPORTED_TARGETS
        .iter()
        .any(|target| {
            let Some(suffix) = suffix.strip_prefix(&format!("{target}-")) else {
                return false;
            };
            let Some((mise_and_sha, digest)) = suffix.rsplit_once('-') else {
                return false;
            };
            let Some((mise, sha256)) = mise_and_sha.rsplit_once('-') else {
                return false;
            };
            is_catalog_version(mise)
                && velnor_actions_contract::ids::is_lower_hex_len(sha256, 64)
                && digest.len() == 16
                && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}

/// Exact pin/version/digest validator shared with the key contract.
fn is_catalog_version(value: &str) -> bool {
    !value.is_empty()
        && value != "latest"
        && !value.contains("latest")
        && value.contains('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// Rename a built step; names are fixed by the caller contract.
fn rename_step(mut step: Step, name: &str) -> Result<Step, RenderError> {
    crate::steps::scan_for_private_subcommands(name)?;
    name.clone_into(&mut step.name);
    Ok(step)
}
