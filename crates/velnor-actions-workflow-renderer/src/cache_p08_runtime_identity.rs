//! Runtime identity gate for V2 Mise cache keys.

use std::collections::BTreeMap;

use velnor_actions_contract::{RunsOn, Step, StepKind};

use crate::{RenderError, cache_p08, commands, marker, steps, tree::RenderedFile, yaml::Yaml};

const SCRIPT: &str = concat!(
    "export LC_ALL=C; set -eu; umask 077; ",
    ": \"${GITHUB_OUTPUT:?missing GITHUB_OUTPUT}\"; ",
    "scratch_root=; scratch_dir=; ",
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
    "cleanup_scratch() { [ -n \"$scratch_dir\" ] || return 0; ",
    "case \"$scratch_dir\" in \"$scratch_root\"/runtime-identity.*) ;; *) return 1 ;; esac; ",
    "[ -d \"$scratch_dir\" ] && [ ! -L \"$scratch_dir\" ] || return 1; ",
    "rm -rf -- \"$scratch_dir\" || return 1; scratch_dir=; }; ",
    "on_exit() { status=$1; trap - EXIT; ",
    "if ! cleanup_scratch; then status=1; fi; exit \"$status\"; }; ",
    "trap 'status=$?; on_exit \"$status\"' EXIT; ",
    "trap 'exit 129' HUP; trap 'exit 130' INT; trap 'exit 143' TERM; ",
    "disable() { reason=$1; if ! cleanup_scratch; then ",
    "printf 'enabled=false\\nreason=identity_scratch_cleanup_failed\\n' >> \"$GITHUB_OUTPUT\"; exit 1; fi; ",
    "printf 'enabled=false\\nreason=%s\\n' \"$reason\" >> \"$GITHUB_OUTPUT\"; exit 0; }; ",
    "case \"$VELNOR_CACHE_LANE\" in scale-set:*) disable scale_set_image_unqualified ;; esac; ",
    "case \"$VELNOR_CACHE_LANE\" in ubuntu-22.04) expected_image_os=ubuntu22 ;; ",
    "ubuntu-24.04) expected_image_os=ubuntu24 ;; ubuntu-26.04) expected_image_os=ubuntu26 ;; ",
    "*) disable lane_image_unqualified ;; esac; expected_arch=X64; ",
    "expected_target=x86_64-unknown-linux-gnu; ",
    "[ -n \"$runner_os\" ] || disable runner_os_missing; ",
    "[ \"$runner_os\" = Linux ] || disable runner_os_mismatch; ",
    "[ -n \"$runner_arch\" ] || disable runner_arch_missing; ",
    "[ \"$runner_arch\" = \"$expected_arch\" ] || disable runner_arch_mismatch; ",
    "[ -n \"$image_os\" ] || disable image_os_missing; ",
    "[ \"$image_os\" = \"$expected_image_os\" ] || disable image_os_mismatch; ",
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
    "scratch_root=\"$runner_temp/velnor\"; ",
    "valid_path \"$scratch_root\" || disable cache_scratch_aliased; ",
    "mkdir -p \"$scratch_root\" || disable cache_scratch_unavailable; ",
    "[ -d \"$scratch_root\" ] && [ ! -L \"$scratch_root\" ] || disable cache_scratch_invalid; ",
    "scratch_dir=\"$scratch_root/runtime-identity.$$\"; ",
    "if ! mkdir -m 700 \"$scratch_dir\"; then scratch_dir=; ",
    "disable identity_scratch_unavailable; fi; ",
    "case \"$scratch_dir\" in \"$scratch_root\"/runtime-identity.*) ;; *) disable cache_scratch_invalid ;; esac; ",
    "valid_path \"$scratch_dir\" && [ -d \"$scratch_dir\" ] && [ ! -L \"$scratch_dir\" ] ",
    "|| disable cache_scratch_invalid; ",
    "identity_file=\"$scratch_dir/preimage\"; sum_file=\"$scratch_dir/digest\"; ",
    "set -C; if ! : > \"$identity_file\"; then set +C; disable identity_file_unavailable; fi; ",
    "if ! : > \"$sum_file\"; then set +C; disable identity_file_unavailable; fi; set +C; ",
    "valid_path \"$identity_file\" && [ -f \"$identity_file\" ] && [ ! -L \"$identity_file\" ] ",
    "|| disable identity_file_invalid; ",
    "valid_path \"$sum_file\" && [ -f \"$sum_file\" ] && [ ! -L \"$sum_file\" ] ",
    "|| disable identity_file_invalid; ",
    "for value in \"velnor-tools-runtime-v2\" \"$VELNOR_CACHE_STATIC_DIGEST\" ",
    "\"$VELNOR_CACHE_LANE\" \"$expected_target\" \"$runner_os\" ",
    "\"$runner_arch\" \"$image_os\" \"$image_version\" \"$home\" ",
    "\"$runner_temp\" \"$effective_mise\" \"$expected_rustup\" ",
    "\"$expected_cargo\"; do if ! printf '%s:%s\\n' \"${#value}\" \"$value\" >> \"$identity_file\"; ",
    "then disable identity_file_write_failed; fi; done; ",
    "if command -v sha256sum >/dev/null 2>&1; then ",
    "if ! sha256sum < \"$identity_file\" > \"$sum_file\"; then disable identity_hash_failed; fi; ",
    "elif command -v shasum >/dev/null 2>&1; then ",
    "if ! shasum -a 256 < \"$identity_file\" > \"$sum_file\"; then disable identity_hash_failed; fi; ",
    "else disable sha256_tool_missing; fi; ",
    "if ! IFS=' ' read -r fingerprint _ < \"$sum_file\"; then disable fingerprint_missing; fi; ",
    "[ ${#fingerprint} -eq 64 ] || disable fingerprint_invalid; ",
    "case \"$fingerprint\" in *[!0-9a-f]*) disable fingerprint_invalid ;; esac; ",
    "if ! cleanup_scratch; then disable identity_scratch_cleanup_failed; fi; ",
    "printf 'identity=%s\\nenabled=true\\nreason=qualified_hosted_image\\n' ",
    "\"$fingerprint\" >> \"$GITHUB_OUTPUT\""
);

pub(super) const SCRIPT_PATH: &str = ".github/scripts/velnor-tools-cache-identity.sh";
const SCRIPT_FILE_NAME: &str = "velnor-tools-cache-identity.sh";
pub(super) fn is_supported_lane(runs_on: &str, target: &str) -> bool {
    let (image, arch) = lane_identity(runs_on, target);
    !image.is_empty() && !arch.is_empty()
}

/// Emit the shared, version-marked identity script used by every job.
/// # Errors
pub(super) fn script_file(version: &str) -> Result<RenderedFile, RenderError> {
    let bytes = marker::with_marker(version, SCRIPT)?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: SCRIPT_PATH.to_owned(),
        bytes,
    })
}

/// Emit the one local composite action that owns identity env and script setup.
/// # Errors
pub(super) fn action_file(runs_on: &str, version: &str) -> Result<RenderedFile, RenderError> {
    let action_path = action_path(runs_on).ok_or_else(|| {
        RenderError::BadCommand("unsupported_tools_cache_identity_lane".to_owned())
    })?;
    let inner = inner_step(runs_on)?;
    let StepKind::Shell { run, env } = inner.kind else {
        return Err(RenderError::InvalidWorkflow(
            "tools_cache_identity_inner_step_not_shell".to_owned(),
        ));
    };
    let mut entries = vec![("name".to_owned(), Yaml::str(inner.name))];
    crate::step_ids::push_explicit_step_id(&mut entries, cache_p08::TOOLS_CACHE_IDENTITY_STEP_ID);
    entries.extend([
        (
            "env".to_owned(),
            crate::document_steps::string_map_yaml(&env),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "run".to_owned(),
            Yaml::str(commands::join_argv_for_run(&run)?),
        ),
    ]);
    let outputs = output_entries();
    let body = Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Velnor Mise cache identity")),
        (
            "description".to_owned(),
            Yaml::str("Qualify the hosted image and cache roots before V2 restore."),
        ),
        ("inputs".to_owned(), input_entries()),
        ("outputs".to_owned(), outputs),
        (
            "runs".to_owned(),
            Yaml::Map(vec![
                ("using".to_owned(), Yaml::str("composite")),
                ("steps".to_owned(), Yaml::Seq(vec![Yaml::Map(entries)])),
            ]),
        ),
    ]);
    let bytes = marker::with_marker(version, &crate::yaml::render_yaml(&body))?;
    steps::scan_for_private_subcommands(&bytes)?;
    Ok(RenderedFile {
        path: format!("{action_path}/action.yml"),
        bytes,
    })
}

fn input_entries() -> Yaml {
    let required_input = |description: &str| {
        Yaml::Map(vec![
            ("description".to_owned(), Yaml::str(description)),
            ("required".to_owned(), Yaml::Bool(true)),
        ])
    };
    Yaml::Map(vec![(
        cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT.to_owned(),
        required_input("Static V2 identity of the tool pins and owned paths."),
    )])
}

fn output_entries() -> Yaml {
    let output = |description: &str, key: &str| {
        Yaml::Map(vec![
            ("description".to_owned(), Yaml::str(description)),
            (
                "value".to_owned(),
                Yaml::str(format!(
                    "${{{{ steps.{}.outputs.{key} }}}}",
                    cache_p08::TOOLS_CACHE_IDENTITY_STEP_ID,
                )),
            ),
        ])
    };
    Yaml::Map(vec![
        (
            "enabled".to_owned(),
            output(
                "Whether the image and absolute roots qualify for restore.",
                "enabled",
            ),
        ),
        (
            "identity".to_owned(),
            output(
                "SHA-256 runtime fingerprint for the qualified host.",
                "identity",
            ),
        ),
    ])
}

fn inner_step(runs_on: &str) -> Result<Step, RenderError> {
    let cargo = "${{ runner.temp }}/velnor/cargo";
    let rustup = "${{ runner.temp }}/velnor/rustup";
    let env = BTreeMap::from([
        ("CARGO_HOME".to_owned(), cargo.to_owned()),
        ("MISE_CARGO_HOME".to_owned(), cargo.to_owned()),
        ("MISE_RUSTUP_HOME".to_owned(), rustup.to_owned()),
        ("RUSTUP_HOME".to_owned(), rustup.to_owned()),
        ("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned()),
        ("VELNOR_CACHE_LANE".to_owned(), runs_on.to_owned()),
        (
            "VELNOR_CACHE_STATIC_DIGEST".to_owned(),
            format!(
                "${{{{ inputs.{} }}}}",
                cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT
            ),
        ),
    ]);
    let run = vec![
        "bash".to_owned(),
        "-c".to_owned(),
        format!("bash \"$GITHUB_ACTION_PATH/../../scripts/{SCRIPT_FILE_NAME}\""),
    ];
    steps::composite_shell_step(cache_p08::TOOLS_CACHE_IDENTITY_NAME, run, env)
}

pub(super) fn action_uses(runs_on: &str) -> Option<&'static str> {
    match hosted_image(runs_on)? {
        "ubuntu22" => Some("./.github/actions/u22"),
        "ubuntu24" => Some("./.github/actions/u24"),
        "ubuntu26" => Some("./.github/actions/u26"),
        _ => None,
    }
}

fn action_path(runs_on: &str) -> Option<&'static str> {
    action_uses(runs_on)?.strip_prefix("./")
}

fn hosted_image(runs_on: &str) -> Option<&'static str> {
    let Ok(RunsOn::Hosted(label)) = RunsOn::parse(runs_on) else {
        return None;
    };
    match label.as_str() {
        "ubuntu-22.04" => Some("ubuntu22"),
        "ubuntu-24.04" => Some("ubuntu24"),
        "ubuntu-26.04" => Some("ubuntu26"),
        _ => None,
    }
}

fn lane_identity(runs_on: &str, target: &str) -> (&'static str, &'static str) {
    if !matches!(target, "x86_64-unknown-linux-gnu") {
        return ("", "");
    }
    (hosted_image(runs_on).unwrap_or(""), "X64")
}

#[cfg(test)]
#[path = "cache_p08_runtime_identity_tests.rs"]
mod tests;
