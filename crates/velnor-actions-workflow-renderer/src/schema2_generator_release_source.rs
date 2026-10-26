// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::workflow_steps;
use crate::Yaml;

pub(super) fn qualification_step() -> Yaml {
    let mut step = workflow_steps::bash_step(
        "Fetch exact public qualification source without credentials",
        QUALIFICATION_SOURCE_PREPARE,
    );
    if let Yaml::Map(fields) = &mut step {
        fields.push(("timeout-minutes".to_owned(), Yaml::Int(2)));
    }
    step
}

pub(super) const QUALIFICATION_SOURCE_PREPARE: &str = r#"set -euo pipefail
[[ "$GITHUB_REPOSITORY" == tailrocks/velnor-new && "$GITHUB_SHA" =~ ^[0-9a-f]{40}$ ]] || {
  printf '%s\n' 'qualification source identity is invalid' >&2
  exit 1
}
[[ "$GITHUB_WORKSPACE" == /* && -d "$GITHUB_WORKSPACE" && ! -L "$GITHUB_WORKSPACE" ]]
cd -- "$GITHUB_WORKSPACE"
[[ "$(pwd -P)" == "$GITHUB_WORKSPACE" ]]
# A clean environment prevents inherited Git configuration, helpers, hooks,
# credentials, and OIDC state from reaching source acquisition.
/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 GIT_ALLOW_PROTOCOL=https /usr/bin/git -c credential.helper= -c core.hooksPath=/dev/null -c submodule.recurse=false -c protocol.file.allow=never -c http.lowSpeedLimit=1 -c http.lowSpeedTime=60 init --quiet --template=/dev/null
/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 GIT_ALLOW_PROTOCOL=https /usr/bin/git -c credential.helper= -c core.hooksPath=/dev/null -c submodule.recurse=false -c protocol.file.allow=never -c http.lowSpeedLimit=1 -c http.lowSpeedTime=60 fetch --quiet --depth=1 --no-tags --no-recurse-submodules https://github.com/tailrocks/velnor-new.git "$GITHUB_SHA"
/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 GIT_ALLOW_PROTOCOL=https /usr/bin/git -c credential.helper= -c core.hooksPath=/dev/null -c submodule.recurse=false -c protocol.file.allow=never -c http.lowSpeedLimit=1 -c http.lowSpeedTime=60 checkout --quiet --detach FETCH_HEAD
[[ "$(/usr/bin/env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TERMINAL_PROMPT=0 GIT_ALLOW_PROTOCOL=https /usr/bin/git -c credential.helper= -c core.hooksPath=/dev/null -c submodule.recurse=false -c protocol.file.allow=never -c http.lowSpeedLimit=1 -c http.lowSpeedTime=60 rev-parse --verify 'HEAD^{commit}')" == "$GITHUB_SHA" ]] || {
  printf '%s\n' 'qualification source does not match the requested commit' >&2
  exit 1
}"#;
