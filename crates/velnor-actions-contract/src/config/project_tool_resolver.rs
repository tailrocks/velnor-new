//! Run-time resolver for project-declared tool versions (hard decision 4).
//!
//! The generated workflow names each project-declared version as
//! `${VELNOR_TOOL_<NAME>}`. One generated step per job, directly after Mise
//! setup, fills those variables from the project's own config: for every
//! referenced tool it reads each declaring source with `mise --no-config
//! --no-env --no-hooks config get` (stored TOML values only: no config is
//! loaded, so no template, hook, env, task, plugin, setting, or wrapper
//! from a fork's checkout is evaluated), requires every source to agree on
//! one exact `X.Y.Z` version, and appends `NAME=<version>` to
//! `$GITHUB_ENV`. The digits-and-dots check runs before the write, so a
//! config value cannot inject another variable or line.

use std::collections::BTreeSet;

/// Root Mise config files a project may declare tools in.
pub const PROJECT_MISE_CONFIG_FILES: [&str; 2] = ["mise.toml", ".mise.toml"];

/// Rust toolchain files Mise (idiomatic version files) and rustup both read.
pub const RUST_TOOLCHAIN_SOURCE_FILES: [&str; 2] = ["rust-toolchain.toml", "rust-toolchain"];

/// `(variable, [tools] keys)` for every catalog tool; the first key is the
/// canonical spelling, the rest are other backends' spellings of the
/// same tool, which declare its version just the same.
pub const PROJECT_TOOL_KEYS: [(&str, &[&str]); 9] = [
    ("VELNOR_TOOL_RUST", &["rust", "core:rust"]),
    (
        "VELNOR_TOOL_MR_BOXINGTON",
        &[
            "mr-boxington",
            "mbx",
            "github:jdx/mr-boxington",
            "packslip:github.com/jdx/mr-boxington",
        ],
    ),
    ("VELNOR_TOOL_GH", &["gh", "aqua:cli/cli"]),
    (
        "VELNOR_TOOL_ACTIONLINT",
        &["actionlint", "aqua:rhysd/actionlint"],
    ),
    (
        "VELNOR_TOOL_SHELLCHECK",
        &["shellcheck", "aqua:koalaman/shellcheck"],
    ),
    (
        "VELNOR_TOOL_ZIZMOR",
        &["zizmor", "aqua:zizmorcore/zizmor", "cargo:zizmor"],
    ),
    (
        "VELNOR_TOOL_NEXTEST",
        &[
            "aqua:nextest-rs/nextest/cargo-nextest",
            "cargo-nextest",
            "cargo:cargo-nextest",
            "github:nextest-rs/nextest",
            "ubi:nextest-rs/nextest",
        ],
    ),
    (
        "VELNOR_TOOL_OPENTOFU",
        &["opentofu", "aqua:opentofu/opentofu"],
    ),
    (
        "VELNOR_TOOL_RELEASE_PLZ",
        &["release-plz", "cargo:release-plz"],
    ),
];

/// `[tools]` keys declaring the tool behind one reference variable.
#[must_use]
pub fn project_tool_keys(name: &str) -> &'static [&'static str] {
    PROJECT_TOOL_KEYS
        .iter()
        .find_map(|(env, keys)| (*env == name).then_some(*keys))
        .unwrap_or(&[])
}

const PRELUDE: &str = r#"set -eu
: "${GITHUB_ENV:?missing GITHUB_ENV}"
: "${RUNNER_TEMP:?missing RUNNER_TEMP}"
velnor_scratch="$RUNNER_TEMP/velnor-project-tool"
velnor_fail() { printf 'velnor: %s\n' "$1" >&2; exit 1; }
velnor_get() { mise --no-config --no-env --no-hooks config get -f "$1" "$2" > "$velnor_scratch" 2>/dev/null; }
velnor_exact() {
  IFS= read -r value < "$velnor_scratch" || value=
  printf '%s\n' "$value" | cmp -s - "$velnor_scratch" || velnor_fail "$1 must declare one exact X.Y.Z version"
  case "$value" in
    ''|*[!0-9.]*|.*|*.|*..*|*.*.*.*) velnor_fail "$1 must declare one exact X.Y.Z version, not '$value'" ;;
    *.*.*) ;;
    *) velnor_fail "$1 must declare one exact X.Y.Z version, not '$value'" ;;
  esac
}
velnor_tool() {
  name=$1
  shift
  resolved=
  for source in "$@"; do
    file=${source%%#*}
    key=${source#*#}
    [ -e "$file" ] || continue
    if [ -L "$file" ] || [ ! -f "$file" ]; then velnor_fail "$file must be a regular file"; fi
    if [ "$key" = - ]; then
      { tr -d ' \t\r\n' < "$file" && echo; } > "$velnor_scratch"
    else
      velnor_get "$file" "$key" || continue
      if grep -q = "$velnor_scratch"; then
        velnor_get "$file" "$key.version" || velnor_fail "$file $key declares no version"
      fi
    fi
    velnor_exact "$file $key"
    if [ -n "$resolved" ] && [ "$resolved" != "$value" ]; then
      velnor_fail "$name: $file $key declares $value, another source declares $resolved"
    fi
    resolved=$value
  done
  [ -n "$resolved" ] || velnor_fail "$name: the project config no longer declares this tool; regenerate the workflow"
  printf '%s=%s\n' "$name" "$resolved" >> "$GITHUB_ENV"
  if [ "$name" = VELNOR_TOOL_RUST ] && [ "${velnor_export_rustup-0}" = 1 ]; then
    printf 'RUSTUP_TOOLCHAIN=%s\n' "$resolved" >> "$GITHUB_ENV"
  fi
  printf 'velnor: %s=%s\n' "$name" "$resolved"
}
"#;

/// Fixed resolver script for the referenced variables, in sorted order.
///
/// `export_rustup` also exports the resolved Rust channel as
/// `RUSTUP_TOOLCHAIN` (job env cannot read the `env` context).
#[must_use]
pub fn project_tool_resolver_script(names: &BTreeSet<String>, export_rustup: bool) -> String {
    let mut script = PRELUDE.to_owned();
    if export_rustup {
        script.push_str("velnor_export_rustup=1\n");
    }
    for name in names {
        script.push_str("velnor_tool ");
        script.push_str(name);
        for file in PROJECT_MISE_CONFIG_FILES {
            for key in project_tool_keys(name) {
                for part in [" '", file, "#tools.", key, "'"] {
                    script.push_str(part);
                }
            }
        }
        if name == "VELNOR_TOOL_RUST" {
            script.push_str(" 'rust-toolchain.toml#toolchain.channel' 'rust-toolchain#-'");
        }
        script.push('\n');
    }
    script.push_str("rm -f \"$velnor_scratch\"\n");
    script
}
