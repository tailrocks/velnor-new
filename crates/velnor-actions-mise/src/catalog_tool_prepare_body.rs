//! Installation source compiled only from exact selected catalog authority.

use velnor_actions_contract::ToolCacheDomain;

use super::{MiseError, argument, contract};

pub(super) fn source(version: &str) -> Result<String, MiseError> {
    composed_source(version, true)
}

pub(super) fn warm_source(version: &str) -> Result<String, MiseError> {
    composed_source(version, false)
}

fn composed_source(version: &str, install: bool) -> Result<String, MiseError> {
    let branches = [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
    .into_iter()
    .map(|domain| {
        format!(
            "  {}) root=\"{}\" ;;",
            argument(domain),
            domain.root().replace("${{ runner.temp }}", "$temp")
        )
    })
    .collect::<Vec<_>>()
    .join("\n");
    let worker = include_str!("catalog_tool_prepare_worker.py");
    let (reset, worker) = if install {
        (
            super::super::native_health::cold_prepare_script(),
            worker.to_owned(),
        )
    } else {
        (
            String::new(),
            format!(
                "__name__ = 'velnor_authenticated_native_preparation'\n{worker}\nmain(install=False)\n"
            ),
        )
    };
    let source = format!(
        "set -euo pipefail\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\n{PREFIX}\n{branches}\n{ROOTS}\n{HOST_VERIFICATION}\nconfig=${{1:?missing qualified tool configuration}}\nshift\n{}\n{CONFIGURATION}\n/usr/bin/python3 -I -S -c '{}' \"$root\" \"$mise\" \"$config\" \"$@\"\nprintf 'verified=true\\n' >> \"${{GITHUB_OUTPUT:?missing runner output file}}\"\n",
        reset,
        worker.replace('\'', "'\\''"),
    );
    velnor_actions_contract::generated_source(version, &source).map_err(|error| contract(&error))
}

const PREFIX: &str = r#"temp=${RUNNER_TEMP:?missing runner temp}
case "$temp" in /*) ;; *) exit 1 ;; esac
case "$temp/" in */../*|*/./*|*//*) exit 1 ;; esac
case "${1:?missing tool domain}" in"#;

const ROOTS: &str = r#"  *) exit 1 ;;
esac
shift
test "${MISE_DATA_DIR:?missing owned tool root}" = "$root"
parent=${root%/*}
for directory in "$temp" "$temp/velnor" "$parent" "$root" "$root/bin"; do
  test -d "$directory"
  test ! -L "$directory"
done
mise="$root/bin/mise"
test -f "$mise"
test -x "$mise"
test ! -L "$mise""#;

const CONFIGURATION: &str = r#"cd -P "$root"
export MISE_CEILING_PATHS="$PWD"
export MISE_CONFIG_DIR="$root/velnor-empty-config"
export MISE_SYSTEM_CONFIG_DIR="$root/velnor-empty-system-config"
for directory in "$MISE_CONFIG_DIR" "$MISE_SYSTEM_CONFIG_DIR"; do
  test ! -L "$directory"
  mkdir -p "$directory"
  test -d "$directory"
  for entry in "$directory"/* "$directory"/.[!.]* "$directory"/..?*; do
    test ! -e "$entry"
    test ! -L "$entry"
  done
done"#;

const HOST_VERIFICATION: &str = r#"case "${1:?missing qualified host}" in
  x86_64-unknown-linux-gnu) system=Linux; architecture=x86_64 ;;
  aarch64-unknown-linux-gnu) system=Linux; architecture=aarch64 ;;
  aarch64-apple-darwin) system=Darwin; architecture=arm64 ;;
  *) exit 1 ;;
esac
shift
test "$(/usr/bin/uname -s)" = "$system"
test "$(/usr/bin/uname -m)" = "$architecture"
sha=${VELNOR_MISE_SHA256:?missing qualified binary digest}
test "${#sha}" = 64
case "$sha" in *[!0-9a-f]*) exit 1 ;; esac
if test "$system" = Darwin; then
  printf '%s  %s\n' "$sha" "$mise" | shasum -a 256 -c -
else
  printf '%s  %s\n' "$sha" "$mise" | sha256sum -c -
fi"#;
