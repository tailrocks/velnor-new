//! Run-time resolver for project-declared tool versions (hard decision 5).
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
//!
//! A `NAME_LOCK` reference (the tools-cache identity reads one per
//! installed project tool) also exports the SHA-256 of that tool's
//! [`crate::config::project_tool_refs::PROJECT_TOOL_LOCK_FILES`] `checksum`/`url`
//! rows for the runner platform at the resolved version, or `unlocked` when
//! no lock has one. A fixed `awk` program reads each lock as text: it is
//! never loaded or executed, and only the hex digest reaches `$GITHUB_ENV`.

use std::collections::BTreeSet;

use super::project_tool_refs::{
    PROJECT_MISE_CONFIG_PATTERNS, PROJECT_TOOL_LOCK_FILES, PROJECT_TOOL_VERSIONS_FILE,
    RUST_TOOLCHAIN_SOURCE_FILES, project_tool_lock_base, project_tool_lock_name,
};

/// `(variable, [tools] keys)` for every catalog tool; the first key is the
/// canonical spelling, the rest are other backends' spellings of the
/// same tool, which declare its version just the same.
pub const PROJECT_TOOL_KEYS: [(&str, &[&str]); 11] = [
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
    (
        "VELNOR_TOOL_CRANE",
        &["crane", "aqua:google/go-containerregistry"],
    ),
    ("VELNOR_TOOL_JAVA", &["java", "core:java"]),
];

/// Variable of the JDK declaration, whose grammar is the adapter-specific
/// HD-4a form instead of exact `X.Y.Z`.
pub const JAVA_TOOL_VARIABLE: &str = "VELNOR_TOOL_JAVA";

/// `[tools]` keys declaring the tool behind one reference variable.
#[must_use]
pub fn project_tool_keys(name: &str) -> &'static [&'static str] {
    PROJECT_TOOL_KEYS
        .iter()
        .find_map(|(env, keys)| (*env == name).then_some(*keys))
        .unwrap_or(&[])
}

const PRELUDE: &str = r##"set -eu
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
# shellcheck disable=SC2016 # awk program text: awk, not the shell, expands it
velnor_lock_awk='BEGIN { q = "\""; e1 = "[[tools." key "]]"; e2 = "[[tools." q key q "]]"; e3 = "[tools." key "]"; e4 = "[tools." q key q "]"; s1 = "[tools." key "."; s2 = "[tools." q key q "."; p1 = s1 q "platforms." platform q "]"; p2 = s2 q "platforms." platform q "]"; p3 = s1 "platforms." platform "]"; p4 = s2 "platforms." platform "]"; want = "version = " q version q; inline = q "platforms." platform q " = {"; state = 0 }
{ line = $0; sub(/^[ \t]+/, "", line); sub(/[ \t\r]+$/, "", line) }
line == "" || substr(line, 1, 1) == "#" { next }
substr(line, 1, 1) == "[" {
  matched = state == 2 || state == 3 || state == 4; other = state == 1 || state == 5
  if (line == e1 || line == e2 || line == e3 || line == e4) state = 1
  else if (line == p1 || line == p2 || line == p3 || line == p4) state = matched ? 3 : (other ? 5 : 0)
  else if (index(line, s1) == 1 || index(line, s2) == 1) state = matched ? 4 : (other ? 5 : 0)
  else state = 0
  next
}
state == 1 && line == want { state = 2; next }
state == 3 && (index(line, "checksum = ") == 1 || index(line, "url = ") == 1) { print lock " " key " " line; next }
state == 2 && index(line, inline) == 1 {
  if (match(line, /checksum = "[^"]*"/)) print lock " " key " " substr(line, RSTART, RLENGTH)
  if (match(line, /[{ ,]url = "[^"]*"/)) print lock " " key " " substr(line, RSTART + 1, RLENGTH - 1)
}'
velnor_lock() {
  name=$1
  shift
  case "${RUNNER_OS-}:${RUNNER_ARCH-}" in
    Linux:X64) platform=linux-x64 ;;
    Linux:ARM64) platform=linux-arm64 ;;
    macOS:X64) platform=macos-x64 ;;
    macOS:ARM64) platform=macos-arm64 ;;
    Windows:X64) platform=windows-x64 ;;
    Windows:ARM64) platform=windows-arm64 ;;
    *) velnor_fail "$name: unsupported runner platform '${RUNNER_OS-}:${RUNNER_ARCH-}'" ;;
  esac
  : > "$velnor_dir/lock"
  for lock in $velnor_locks; do
    if [ ! -e "$lock" ] && [ ! -L "$lock" ]; then continue; fi
    velnor_regular "$lock"
    for key in "$@"; do
      awk -v lock="$lock" -v key="$key" -v version="$resolved" -v platform="$platform" "$velnor_lock_awk" "$lock" >> "$velnor_dir/lock" || velnor_fail "$lock could not be read"
    done
  done
  lock_digest=unlocked
  if [ -s "$velnor_dir/lock" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
      lock_digest=$(sha256sum < "$velnor_dir/lock") || velnor_fail "$name: lock digest failed"
    else
      lock_digest=$(shasum -a 256 < "$velnor_dir/lock") || velnor_fail "$name: lock digest failed"
    fi
    lock_digest=${lock_digest%% *}
    case "$lock_digest" in
      *[!0-9a-f]*) velnor_fail "$name: lock digest is not hexadecimal" ;;
    esac
    [ "${#lock_digest}" -eq 64 ] || velnor_fail "$name: lock digest is not SHA-256"
  fi
  printf '%s=%s\n' "$name" "$lock_digest" >> "$GITHUB_ENV"
  printf 'velnor: %s=%s\n' "$name" "$lock_digest"
}
"##;

/// JDK resolution (HD-4a), appended only for jobs that reference the JDK.
///
/// `velnor_exact_java` accepts exactly
/// `<vendor>-<X.Y.Z>[+<build>[.<n>]*][.LTS]` over `[A-Za-z0-9.+-]`, so the
/// single-line `$GITHUB_ENV` write cannot be injected.
const JAVA_FUNCTIONS: &str = r#"velnor_exact_java() {
  v=$2
  case "$v" in
    ''|*[!A-Za-z0-9.+-]*) velnor_fail "$1 must declare an exact JDK such as oracle-graalvm-25.0.3, not '$(velnor_safe "$v")'" ;;
  esac
  rest=
  for vendor in temurin oracle-graalvm graalvm-community corretto zulu; do
    case "$v" in "$vendor"-*) rest=${v#"$vendor"-} ;; esac
  done
  [ -n "$rest" ] || velnor_fail "$1 must name a vendor (temurin, oracle-graalvm, graalvm-community, corretto, zulu), not '$(velnor_safe "$v")'"
  case "$rest" in *.LTS) rest=${rest%.LTS} ;; esac
  core=${rest%%+*}
  build=
  case "$rest" in *+*) build=${rest#*+}; [ -n "$build" ] || velnor_fail "$1 has an empty build in '$(velnor_safe "$v")'" ;; esac
  velnor_exact "$1" "$core"
  case "$build" in
    '') ;;
    *[!0-9.]*|.*|*.|*..*) velnor_fail "$1 has a malformed build in '$(velnor_safe "$v")'" ;;
  esac
}
velnor_tool_java() {
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
    velnor_exact_java "$file $key" "$value"
    if [ -n "$resolved" ] && [ "$resolved" != "$value" ]; then
      velnor_fail "$name: $file $key declares $value, $origin declares $resolved"
    fi
    if [ -z "$resolved" ]; then origin="$file $key"; fi
    resolved=$value
  done < "$velnor_db"
  [ -n "$resolved" ] || velnor_fail "$name: the project config no longer declares this tool; regenerate the workflow"
  printf '%s=%s\n' "$name" "$resolved" >> "$GITHUB_ENV"
  printf 'velnor: %s=%s\n' "$name" "$resolved"
}
"#;

/// Fixed resolver script for the referenced variables, in sorted order.
///
/// A `NAME_LOCK` reference resolves `NAME` too and then exports its lock
/// digest. `export_rustup` also exports the resolved Rust channel as
/// `RUSTUP_TOOLCHAIN` (job env cannot read the `env` context).
#[must_use]
pub fn project_tool_resolver_script(names: &BTreeSet<String>, export_rustup: bool) -> String {
    let tools: BTreeSet<&str> = names
        .iter()
        .map(|name| project_tool_lock_base(name).unwrap_or(name))
        .collect();
    let mut script = PRELUDE.to_owned();
    if tools.contains(JAVA_TOOL_VARIABLE) {
        script.push_str(JAVA_FUNCTIONS);
    }
    script.push_str("velnor_locks='");
    script.push_str(&PROJECT_TOOL_LOCK_FILES.join(" "));
    script.push_str("'\n");
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
            tools
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
    for name in tools {
        let lock = project_tool_lock_name(name);
        let lock = names.contains(&lock).then_some(lock);
        let tool_function = if name == JAVA_TOOL_VARIABLE {
            "velnor_tool_java "
        } else {
            "velnor_tool "
        };
        for (function, variable) in std::iter::once((tool_function, name))
            .chain(lock.as_deref().map(|lock| ("velnor_lock ", lock)))
        {
            script.push_str(function);
            script.push_str(variable);
            for key in project_tool_keys(name) {
                script.push_str(" '");
                script.push_str(key);
                script.push('\'');
            }
            script.push('\n');
        }
    }
    script.push_str("rm -rf \"$velnor_dir\"\n");
    script
}
