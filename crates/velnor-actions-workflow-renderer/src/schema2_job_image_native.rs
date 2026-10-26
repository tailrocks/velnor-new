// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{ProductReleasePins, generator_release};
use crate::{RenderError, commands::join_argv_for_run, yaml::Yaml};
use velnor_actions_contract::ReleaseTarget;

pub(super) const BINARY: &str = "$VELNOR_NATIVE_IMAGE_ACTIONS";
pub(super) const CLOSURE: &str = "job-image-native-source-closure.json";

pub(super) fn source_steps(pins: &ProductReleasePins) -> Result<Vec<Yaml>, RenderError> {
    let build = join_argv_for_run(&pins.build_argv)?;
    let script = format!(
        "set -euo pipefail\ntrusted_git() {{ /usr/bin/env -i PATH=/usr/bin:/bin GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_OPTIONAL_LOCKS=0 /usr/bin/git --git-dir=\"$GITHUB_WORKSPACE/.git\" --work-tree=\"$GITHUB_WORKSPACE\" -c core.fsmonitor=false \"$@\"; }}\nsource_sha=\"$(trusted_git rev-parse --verify HEAD)\"\ntest \"$source_sha\" = \"$GITHUB_SHA\"\ntest \"$GITHUB_WORKFLOW_SHA\" = \"$GITHUB_SHA\"\nif ! trusted_git diff --cached --quiet --no-ext-diff --no-textconv --ignore-submodules=none || ! trusted_git diff --quiet --no-ext-diff --no-textconv --ignore-submodules=none; then\n  printf '%s\\n' 'The source checkout is not clean.' >&2\n  exit 1\nfi\nstatus_file=\"$(mktemp \"$RUNNER_TEMP/velnor-image-source-status.XXXXXXXX\")\"\ntrusted_git status --porcelain=v1 --untracked-files=all --ignore-submodules=none -z > \"$status_file\"\nif [[ -s \"$status_file\" ]]; then\n  printf '%s\\n' 'The source checkout is not clean.' >&2\n  exit 1\nfi\nenv -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR {build}\ntest -f target/release/velnor-actions && test ! -L target/release/velnor-actions\numask 077\nimage_binary_dir=\"$(mktemp -d \"$RUNNER_TEMP/velnor-native-image-actions.XXXXXXXX\")\"\nimage_binary=\"$image_binary_dir/velnor-native-image-actions\"\n( set -o noclobber; cat target/release/velnor-actions > \"$image_binary\" )\nchmod 0500 \"$image_binary\"\nprintf 'VELNOR_NATIVE_IMAGE_ACTIONS=%s\\n' \"$image_binary\" >> \"$GITHUB_ENV\""
    );
    Ok(vec![
        generator_release::mise_setup_step(pins, ReleaseTarget::LinuxX86_64)?,
        generator_release::mise_install_step(
            "Install pinned Rust and MBX",
            &pins.install_build_tools_argv,
        )?,
        generator_release::verify_rust_step(pins)?,
        generator_release::bash_step("Build exact-source native image maintenance", &script),
    ])
}
