//! Exact MBX and Rust toolchain PATH preflight.

use std::collections::BTreeMap;

use crate::RenderError;
use crate::steps::shell_step;
use velnor_actions_contract::Step;

/// Display name for the strict MBX and Rust PATH preflight.
pub const MBX_PREFLIGHT_NAME: &str = "Verify MBX and Rust toolchains";

/// Verify the installed catalog tools before exposing them to the MBX action.
///
/// The check resolves both exact Mise installs with configuration, env files,
/// and hooks disabled. It validates the executable versions and the Rustup
/// shim's explicit toolchain before adding either install root to
/// `GITHUB_PATH`. The caller supplies the isolated Mise environment from the
/// pinned tool identity.
/// # Errors
pub fn mbx_path_preflight_step(
    mbx_version: &str,
    rust_toolchain: &str,
    env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    validate_pins(mbx_version, rust_toolchain, &env)?;
    let env = with_rustup_process_homes(env)?;
    shell_step(
        MBX_PREFLIGHT_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            preflight_script(mbx_version, rust_toolchain),
        ],
        env,
    )
}

fn validate_pins(
    mbx_version: &str,
    rust_toolchain: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), RenderError> {
    if !super::is_exact_mbx_version(mbx_version) {
        return Err(RenderError::BadCommand(format!(
            "bad_mbx_version:{mbx_version}"
        )));
    }
    if !is_exact_rust_toolchain(rust_toolchain) {
        return Err(RenderError::BadCommand(format!(
            "bad_rust_toolchain:{rust_toolchain}"
        )));
    }
    if env.get("RUSTUP_TOOLCHAIN").map(String::as_str) != Some(rust_toolchain) {
        return Err(RenderError::BadCommand(
            "rust_toolchain_identity_mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn preflight_script(mbx_version: &str, rust_toolchain: &str) -> String {
    [
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
        format!("\"$rustc_bin\" '+{rust_toolchain}' -vV > \"$rustc_output_file\""),
        format!(
            "grep -Fqx 'release: {rust_toolchain}' \"$rustc_output_file\" || {{ printf 'Rustup did not report exact toolchain {rust_toolchain}\\n' >&2; exit 1; }}"
        ),
        "rm -f \"$mbx_probe\" \"$rust_probe\" \"$mbx_output_file\" \"$rustc_output_file\""
            .to_owned(),
        "rmdir \"$probe_dir\"".to_owned(),
        "printf '%s\\n' \"$rust_root\" \"$mbx_root\" >> \"$GITHUB_PATH\"".to_owned(),
    ]
    .join("; ")
}

/// Publish the matching native Rustup/Cargo homes beside Mise's tool homes.
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
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}
