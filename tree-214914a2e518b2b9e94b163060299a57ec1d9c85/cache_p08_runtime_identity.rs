//! Runtime identity gate for V2 Mise cache keys.

use std::collections::BTreeMap;

use velnor_actions_contract::{RunsOn, Step};

use crate::{RenderError, cache_p08, steps};

use super::ToolsCachePayload;

const SCRIPT: &str = concat!(
    "export LC_ALL=C; set -eu; umask 077; ",
    ": \"${GITHUB_OUTPUT:?missing GITHUB_OUTPUT}\"; ",
    "disable() { printf 'enabled=false\\nreason=%s\\n' \"$1\" >> \"$GITHUB_OUTPUT\"; exit 0; }; ",
    "runner_os=${RUNNER_OS-}; runner_arch=${RUNNER_ARCH-}; image_os=${ImageOS-}; ",
    "image_version=${ImageVersion-}; home=${HOME-}; runner_temp=${RUNNER_TEMP-}; ",
    "mise_rustup=${MISE_RUSTUP_HOME-}; rustup_home=${RUSTUP_HOME-}; ",
    "mise_cargo=${MISE_CARGO_HOME-}; cargo_home=${CARGO_HOME-}; ",
    "valid_path() { value=$1; case \"$value\" in /*) ;; *) return 1 ;; esac; ",
    "[ \"$value\" != / ] || return 1; ",
    "case \"$value\" in *[![:print:]]*|*//*|*/./*|*/../*|*/.|*/..) return 1 ;; esac; ",
    "remaining=${value#/}; prefix=; while [ -n \"$remaining\" ]; do ",
    "case \"$remaining\" in */*) part=${remaining%%/*}; remaining=${remaining#*/} ;; ",
    "*) part=$remaining; remaining= ;; esac; [ -n \"$part\" ] || return 1; ",
    "prefix=\"$prefix/$part\"; [ ! -L \"$prefix\" ] || return 1; done; }; ",
    "case \"$VELNOR_CACHE_LANE\" in scale-set:*) disable scale_set_image_unqualified ;; esac; ",
    "[ -n \"$runner_os\" ] || disable runner_os_missing; ",
    "[ \"$runner_os\" = Linux ] || disable runner_os_mismatch; ",
    "[ -n \"$runner_arch\" ] || disable runner_arch_missing; ",
    "[ \"$runner_arch\" = \"$VELNOR_CACHE_EXPECTED_ARCH\" ] || disable runner_arch_mismatch; ",
    "[ -n \"$image_os\" ] || disable image_os_missing; ",
    "[ \"$image_os\" = \"$VELNOR_CACHE_EXPECTED_IMAGE_OS\" ] || disable image_os_mismatch; ",
    "[ -n \"$image_version\" ] || disable image_version_missing; ",
    "case \"$image_version\" in *[!0-9.]*) disable image_version_invalid ;; esac; ",
    "case \"$image_version\" in *[0-9]*) ;; *) disable image_version_invalid ;; esac; ",
    "[ -n \"$home\" ] && [ -d \"$home\" ] || disable home_missing; ",
    "[ -n \"$runner_temp\" ] && [ -d \"$runner_temp\" ] || disable runner_temp_missing; ",
    "valid_path \"$home\" || disable home_aliased; ",
    "valid_path \"$runner_temp\" || disable runner_temp_aliased; ",
    "expected_mise=\"$home/.local/share/mise\"; effective_mise=${MISE_DATA_DIR-}; ",
    "if [ -z \"$effective_mise\" ]; then if [ -n \"${XDG_DATA_HOME-}\" ]; ",
    "then effective_mise=\"$XDG_DATA_HOME/mise\"; else effective_mise=$expected_mise; fi; fi; ",
    "valid_path \"$expected_mise\" || disable mise_root_aliased; ",
    "valid_path \"$effective_mise\" || disable mise_root_invalid; ",
    "[ \"$effective_mise\" = \"$expected_mise\" ] || disable mise_root_not_archived; ",
    "expected_rustup=\"$runner_temp/velnor/rustup\"; ",
    "expected_cargo=\"$runner_temp/velnor/cargo\"; ",
    "[ \"$mise_rustup\" = \"$expected_rustup\" ] || disable mise_rustup_home_mismatch; ",
    "[ \"$rustup_home\" = \"$expected_rustup\" ] || disable rustup_home_mismatch; ",
    "[ \"$mise_cargo\" = \"$expected_cargo\" ] || disable mise_cargo_home_mismatch; ",
    "[ \"$cargo_home\" = \"$expected_cargo\" ] || disable cargo_home_mismatch; ",
    "valid_path \"$expected_rustup\" || disable rustup_root_invalid; ",
    "valid_path \"$expected_cargo\" || disable cargo_root_invalid; ",
    "valid_path \"$runner_temp/velnor\" || disable cache_scratch_aliased; ",
    "mkdir -p \"$runner_temp/velnor\"; ",
    "identity_file=\"$runner_temp/velnor/tool-cache-identity\"; ",
    "sum_file=\"$runner_temp/velnor/tool-cache-identity-sum\"; ",
    ": > \"$identity_file\"; ",
    "for value in \"velnor-tools-runtime-v2\" \"$VELNOR_CACHE_STATIC_DIGEST\" ",
    "\"$VELNOR_CACHE_LANE\" \"$VELNOR_CACHE_TARGET\" \"$runner_os\" ",
    "\"$runner_arch\" \"$image_os\" \"$image_version\" \"$home\" ",
    "\"$runner_temp\" \"$effective_mise\" \"$expected_rustup\" ",
    "\"$expected_cargo\"; do printf '%s:%s\\n' \"${#value}\" \"$value\" >> \"$identity_file\"; done; ",
    "if command -v sha256sum >/dev/null 2>&1; then sha256sum < \"$identity_file\" > \"$sum_file\"; ",
    "elif command -v shasum >/dev/null 2>&1; then shasum -a 256 < \"$identity_file\" > \"$sum_file\"; ",
    "else disable sha256_tool_missing; fi; ",
    "IFS=' ' read -r fingerprint _ < \"$sum_file\" || disable fingerprint_missing; ",
    "[ ${#fingerprint} -eq 64 ] || disable fingerprint_invalid; ",
    "case \"$fingerprint\" in *[!0-9a-f]*) disable fingerprint_invalid ;; esac; ",
    "printf 'identity=%s\\nenabled=true\\nreason=qualified_hosted_image\\n' ",
    "\"$fingerprint\" >> \"$GITHUB_OUTPUT\""
);

/// Construct an identity step from the exact lane and payload digest.
/// # Errors
pub(super) fn step(payload: &ToolsCachePayload) -> Result<Step, RenderError> {
    let (expected_image_os, expected_arch) = lane_identity(&payload.runs_on, &payload.target);
    let env = BTreeMap::from([
        (
            "CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        (
            "VELNOR_CACHE_EXPECTED_ARCH".to_owned(),
            expected_arch.to_owned(),
        ),
        (
            "VELNOR_CACHE_EXPECTED_IMAGE_OS".to_owned(),
            expected_image_os.to_owned(),
        ),
        ("VELNOR_CACHE_LANE".to_owned(), payload.runs_on.clone()),
        (
            "VELNOR_CACHE_STATIC_DIGEST".to_owned(),
            payload.static_digest.clone(),
        ),
        ("VELNOR_CACHE_TARGET".to_owned(), payload.target.clone()),
    ]);
    steps::shell_step(
        cache_p08::TOOLS_CACHE_IDENTITY_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), SCRIPT.to_owned()],
        env,
    )
}

fn lane_identity(runs_on: &str, target: &str) -> (&'static str, &'static str) {
    if !matches!(target, "x86_64-unknown-linux-gnu") {
        return ("", "");
    }
    let Ok(RunsOn::Hosted(label)) = RunsOn::parse(runs_on) else {
        return ("", "");
    };
    let image = match label.as_str() {
        "ubuntu-22.04" => "ubuntu22",
        "ubuntu-24.04" => "ubuntu24",
        "ubuntu-26.04" => "ubuntu26",
        _ => "",
    };
    (image, "X64")
}

#[cfg(test)]
#[path = "cache_p08_runtime_identity_tests.rs"]
mod tests;
