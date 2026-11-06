// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::Yaml;
use velnor_actions_mise::catalog::{
    COSIGN_LINUX_AMD64_BYTES, COSIGN_LINUX_AMD64_SHA256, COSIGN_VERSION, GH_VERSION,
};

pub(super) const PYTHON_VERSION: &str = "3.15.2";

pub(crate) fn acquire() -> Yaml {
    let base_version = env!("CARGO_PKG_VERSION");
    let script = include_str!("schema2_sdk_preview_bootstrap.py");
    let run = format!(
        r#"set -euo pipefail
umask 077
case "${{RUNNER_OS-}}/${{RUNNER_ARCH-}}" in Linux/X64) ;; *) exit 1 ;; esac
mise --no-config --no-env --no-hooks install 'gh@{GH_VERSION}' python@{PYTHON_VERSION}
python="$(mise --no-config --no-env --no-hooks which python --tool python@{PYTHON_VERSION})"
[ "$("$python" --version)" = 'Python {PYTHON_VERSION}' ]
VELNOR_BOOTSTRAP_GH="$(mise --no-config --no-env --no-hooks which gh --tool 'gh@{GH_VERSION}')"
export VELNOR_BOOTSTRAP_GH
bootstrap="$(mktemp -d "$RUNNER_TEMP/velnor-sdk-bootstrap.XXXXXXXX")"
trap 'rm -rf "$bootstrap"' EXIT
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 15 --max-time 120 --retry 5 --retry-all-errors --retry-max-time 180 --max-filesize {COSIGN_LINUX_AMD64_BYTES} 'https://github.com/sigstore/cosign/releases/download/v{COSIGN_VERSION}/cosign-linux-amd64' -o "$bootstrap/cosign"
printf '%s  %s\n' '{COSIGN_LINUX_AMD64_SHA256}' "$bootstrap/cosign" | sha256sum --check --strict -
[ "$(wc -c < "$bootstrap/cosign" | tr -d ' ')" = '{COSIGN_LINUX_AMD64_BYTES}' ]
chmod 0755 "$bootstrap/cosign"
export VELNOR_BOOTSTRAP_COSIGN="$bootstrap/cosign"
export VELNOR_SDK_BASE_VERSION="{base_version}"
"$python" - <<'VELNOR_SDK_PREVIEW_BOOTSTRAP'
{script}
VELNOR_SDK_PREVIEW_BOOTSTRAP
"#
    );
    Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Acquire independently verified exact-source SDK preview"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![(
                "GH_TOKEN".to_owned(),
                Yaml::str("${{ secrets.GITHUB_TOKEN }}"),
            )]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ])
}
