// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{
    MBX_BSE_ENV, MBX_BSE_PRODUCER, MBX_BSE_SKELETON,
};
use velnor_actions_contract::workflow::cache_family_mbx::MBX_BUNDLE_ROOT_ENV;
use velnor_actions_contract::workflow::state_seal::STATE_PRODUCER_LANE_CONDITION;
use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::steps::shell_step;

pub const STATE_SKELETON_NAME: &str = "Stub workspace sources";
pub const STATE_SKELETON_RESTORE_NAME: &str = "Restore workspace sources";
pub const STATE_SKELETON_DIR: &str = "$RUNNER_TEMP/velnor/state-skeleton";

const GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

pub fn state_skeleton_step(
    rust_toolchain: &str,
    root: &str,
    toolchain_env: BTreeMap<String, String>,
) -> Result<Step, RenderError> {
    crate::cache_steps::validate_rust_toolchain(rust_toolchain)?;
    let script = format!(
        r#"set -eu
{GUARD}
d="{STATE_SKELETON_DIR}"
rm -rf "$d"
mkdir -p "$d"
root="${{{MBX_BUNDLE_ROOT_ENV}}}"
manifest="$root/Cargo.toml"
[ "$root" != . ] || manifest=Cargo.toml
ws="${{GITHUB_WORKSPACE%/}}"
cd "$ws"
mise --no-config --no-env --no-hooks exec rust@{rust_toolchain} -- cargo metadata --no-deps --locked --offline --format-version 1 --manifest-path "$manifest" > "$d/metadata.json"
empties='.kind | any(. == "lib" or . == "rlib" or . == "dylib" or . == "cdylib" or . == "staticlib" or . == "proc-macro" or . == "test" or . == "bench")'
jq -r ".packages[].targets[] | select($empties) | .src_path" "$d/metadata.json" > "$d/libs"
jq -r ".packages[].targets[] | select($empties | not) | .src_path" "$d/metadata.json" > "$d/bins"
: > "$d/stubbed"
stub() {{ case "$1" in "$ws"/*) rel=${{1#"$ws"/}} ;; *) printf 'state skeleton target outside the workspace: %s\n' "$1" >&2; exit 1 ;; esac; git ls-files --error-unmatch -- "$rel" > /dev/null; printf '%s\n' "$rel" >> "$d/stubbed"; }}
while IFS= read -r path; do stub "$path"; : > "$path"; done < "$d/libs"
while IFS= read -r path; do stub "$path"; printf '%s\n' 'fn main() {{}}' > "$path"; done < "$d/bins"
[ -s "$d/stubbed" ] || {{ printf '%s\n' 'state skeleton has no targets' >&2; exit 1; }}
printf '%s\n' '{MBX_BSE_ENV}={MBX_BSE_SKELETON}' >> "$GITHUB_ENV"
wc -l < "$d/stubbed" > "$d/count"; count=; read -r count < "$d/count"; printf 'velnor: state skeleton stubbed targets=%s\n' "$count"
"#
    );
    let mut env = toolchain_env;
    env.insert(MBX_BUNDLE_ROOT_ENV.to_owned(), root.to_owned());
    let mut step = shell_step(
        STATE_SKELETON_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        env,
    )?;
    step.condition = Some(STATE_PRODUCER_LANE_CONDITION.to_owned());
    step.role = Some(StepRole::StateSkeleton);
    Ok(step)
}

pub fn state_skeleton_restore_step() -> Result<Step, RenderError> {
    let script = format!(
        r#"set -eu
{GUARD}
d="{STATE_SKELETON_DIR}"
ws="${{GITHUB_WORKSPACE%/}}"
[ -s "$d/stubbed" ] || {{ printf '%s\n' 'state skeleton record is missing' >&2; exit 1; }}
cd "$ws"
while IFS= read -r rel; do git checkout -- "$rel"; done < "$d/stubbed"
git diff --quiet || {{ printf '%s\n' 'workspace sources differ after the skeleton phase' >&2; exit 1; }}
[ -n "${{MBX_CACHE_DIR-}}" ] && [ "$MBX_CACHE_DIR" = "$RUNNER_TEMP/velnor/mbx" ] || {{ printf '%s\n' 'MBX_CACHE_DIR is not the store path' >&2; exit 1; }}
rm -rf "$MBX_CACHE_DIR/incremental"
rm -rf "$RUNNER_TEMP/velnor/r${{GITHUB_RUN_ID:?}}-a${{VELNOR_RUN_KEY_ATTEMPT:?}}"/m-*
printf '%s\n' '{MBX_BSE_ENV}={MBX_BSE_PRODUCER}' >> "$GITHUB_ENV"
wc -l < "$d/stubbed" > "$d/count"; count=; read -r count < "$d/count"; printf 'velnor: state skeleton restored files=%s\n' "$count"
"#
    );
    let mut step = shell_step(
        STATE_SKELETON_RESTORE_NAME,
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            crate::state_script::one_line(&script),
        ],
        BTreeMap::new(),
    )?;
    step.condition = Some(STATE_PRODUCER_LANE_CONDITION.to_owned());
    step.role = Some(StepRole::StateSkeletonRestore);
    Ok(step)
}
