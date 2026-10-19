//! Resolve project tool versions at run time (hard decision 5).
//!
//! The generated workflow names each project-declared version as
//! `${VELNOR_TOOL_<NAME>}`. One generated step per job, directly after Mise
//! setup, fills those variables from the project's own config. It reads
//! every location Mise reads `[tools]` from
//! ([`crate::config::project_tool_refs::PROJECT_MISE_CONFIG_PATTERNS`], `.tool-versions`, and for Rust the
//! toolchain files) with `mise --no-config --no-env --no-hooks config get`
//! (stored TOML values only: no config is loaded, so no template, hook,
//! env, task, plugin, setting, or wrapper from a fork's checkout is
//! evaluated). An unparseable source, a symlink, a loose or malformed
//! value, or two sources that disagree fail the step; otherwise it appends
//! `NAME=<version>` to `$GITHUB_ENV`. The digits-and-dots check runs before
//! the write, so a config value cannot inject another variable or line.

use std::collections::BTreeSet;

use super::project_tool_refs::{
    PROJECT_MISE_CONFIG_PATTERNS, PROJECT_TOOL_VERSIONS_FILE, RUST_TOOLCHAIN_SOURCE_FILES,
};

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
velnor_dir="$RUNNER_TEMP/velnor-project-tool"
velnor_db="$velnor_dir/declared"
velnor_tab=$(printf '\t')
velnor_safe() { printf '%s' "$1" | cut -c1-64 | tr -c 'A-Za-z0-9._/@+-' '?'; }
velnor_fail() { printf 'velnor: %s\n' "$1" >&2; exit 1; }
velnor_mise() { mise --no-config --no-env --no-hooks "$@"; }
rm -rf "$velnor_dir"
mkdir -p "$velnor_dir"
: > "$velnor_db"
velnor_regular() {
  if [ -L "$1" ] || [ ! -f "$1" ]; then velnor_fail "$1 must be a regular file"; fi
}
velnor_toml() {
  cp -- "$1" "$velnor_dir/src.toml"
  velnor_mise config get -f "$velnor_dir/src.toml" > /dev/null 2>&1 || velnor_fail "$1 is not valid TOML"
}
velnor_tools() {
  velnor_toml "$1"
  if velnor_mise config get -f "$velnor_dir/src.toml" tools > "$velnor_dir/tools" 2>/dev/null; then
    awk -v file="$1" '
function unq(s) {
  if (s ~ /^"[^"\\]*"$/ || s ~ /^\047[^\047]*\047$/) { return substr(s, 2, length(s) - 2) }
  return s
}
BEGIN { OFS = "\t"; cur = ""; multi = "" }
{
  line = $0
  if (multi != "") { if (index(line, multi) > 0) { multi = "" } next }
  if (line ~ /^[ \t]*(#|$)/) { next }
  if (line ~ /^[ \t]*\[/) {
    hdr = line
    sub(/^[ \t]*\[+[ \t]*/, "", hdr)
    sub(/[ \t]*\]+[ \t]*$/, "", hdr)
    cur = unq(hdr)
    if (cur == "") { cur = "?" }
    next
  }
  eq = index(line, " = ")
  if (eq == 0) { next }
  k = unq(substr(line, 1, eq - 1))
  v = substr(line, eq + 3)
  if (v ~ /^"""/ || v ~ /^\047\047\047/) {
    d = substr(v, 1, 3)
    if (index(substr(v, 4), d) == 0) { multi = d }
    v = d
  } else {
    v = unq(v)
  }
  if (cur == "") { print file, k, v } else if (k == "version") { print file, cur, v }
}' "$velnor_dir/tools" >> "$velnor_db"
  fi
}
velnor_tool_versions() {
  awk -v file="$1" '
BEGIN { OFS = "\t" }
{
  sub(/[ \t]*#.*$/, "")
  if ($0 ~ /^[ \t]*$/) { next }
  line = $0
  sub(/^[ \t]+/, "", line)
  k = line
  sub(/[ \t].*$/, "", k)
  v = line
  if (v == k) { v = "" } else { sub(/^[^ \t]+[ \t]+/, "", v); sub(/[ \t]+$/, "", v) }
  print file, k, v
}' "$1" >> "$velnor_db"
}
velnor_toolchain() {
  if grep -q '^[[:space:]]*\[' "$1"; then
    velnor_toml "$1"
    if velnor_mise config get -f "$velnor_dir/src.toml" toolchain.channel > "$velnor_dir/channel" 2>/dev/null; then
      awk -v file="$1" 'BEGIN { OFS = "\t" } NR == 1 { v = $0; next } { v = v " " $0 } END { print file, "rust", v }' "$velnor_dir/channel" >> "$velnor_db"
    fi
  else
    printf '%s\t%s\t%s\n' "$1" rust "$(tr -d ' \t\r\n' < "$1")" >> "$velnor_db"
  fi
}
velnor_exact() {
  case "$2" in
    ''|*[!0-9.]*|.*|*.|*..*|*.*.*.*|0[0-9]*|*.0[0-9]*) velnor_fail "$1 must declare one exact X.Y.Z version, not '$(velnor_safe "$2")'" ;;
    *.*.*) ;;
    *) velnor_fail "$1 must declare one exact X.Y.Z version, not '$(velnor_safe "$2")'" ;;
  esac
}
velnor_tool() {
  name=$1
  shift
  resolved=
  origin=
  while IFS="$velnor_tab" read -r file key value; do
    matched=0
    for want in "$@"; do
      if [ "$key" = "$want" ]; then matched=1; fi
    done
    if [ "$matched" = 0 ]; then continue; fi
    velnor_exact "$file $key" "$value"
    if [ -n "$resolved" ] && [ "$resolved" != "$value" ]; then
      velnor_fail "$name: $file $key declares $value, $origin declares $resolved"
    fi
    if [ -z "$resolved" ]; then origin="$file $key"; fi
    resolved=$value
  done < "$velnor_db"
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
    script.push_str("for velnor_file in ");
    script.push_str(&PROJECT_MISE_CONFIG_PATTERNS.join(" "));
    script.push_str("; do\n");
    script.push_str(
        "  if [ ! -e \"$velnor_file\" ] && [ ! -L \"$velnor_file\" ]; then continue; fi\n",
    );
    script.push_str("  velnor_regular \"$velnor_file\"\n  velnor_tools \"$velnor_file\"\ndone\n");
    for (file, loader) in std::iter::once((PROJECT_TOOL_VERSIONS_FILE, "velnor_tool_versions"))
        .chain(
            names
                .contains("VELNOR_TOOL_RUST")
                .then_some(RUST_TOOLCHAIN_SOURCE_FILES)
                .into_iter()
                .flatten()
                .map(|file| (file, "velnor_toolchain")),
        )
    {
        for part in [
            "if [ -e ",
            file,
            " ] || [ -L ",
            file,
            " ]; then\n  velnor_regular ",
            file,
            "\n  ",
            loader,
            " ",
            file,
            "\nfi\n",
        ] {
            script.push_str(part);
        }
    }
    for name in names {
        script.push_str("velnor_tool ");
        script.push_str(name);
        for key in project_tool_keys(name) {
            script.push_str(" '");
            script.push_str(key);
            script.push('\'');
        }
        script.push('\n');
    }
    script.push_str("rm -rf \"$velnor_dir\"\n");
    script
}
