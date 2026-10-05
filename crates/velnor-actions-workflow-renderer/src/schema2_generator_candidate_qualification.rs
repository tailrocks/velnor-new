//! Validate same-run candidate bytes and execute the qualification CLI.

use super::{
    ASSET_DIR, AssetNames, LINUX_TARGET, MACOS_ARM_TARGET, MACOS_X64_TARGET,
    RELEASE_MANIFEST_FILENAME,
};

const MANIFEST_DIR: &str = "manifest-assets";
pub(super) fn verify_candidate_script(
    target: &str,
    asset: &str,
    sidecar: &str,
    provenance: &str,
    version: &str,
    os: &str,
    assets: &AssetNames,
) -> String {
    let (sum, host, file_pattern, native_check) = match (target, os) {
        (LINUX_TARGET, "linux") => ("sha256sum --", "Linux:x86_64", "*ELF*x86-64*", ":"),
        (MACOS_ARM_TARGET, "macos-arm64") => (
            "shasum -a 256",
            "macOS:arm64",
            "*Mach-O*arm64*",
            "test \"$(sysctl -in sysctl.proc_translated 2>/dev/null || printf 0)\" != 1 && test \"$(lipo -archs \"$candidate\")\" = arm64",
        ),
        (MACOS_X64_TARGET, "macos-x64") => (
            "shasum -a 256",
            "macOS:x86_64",
            "*Mach-O*x86_64*",
            "test \"$(sysctl -in sysctl.proc_translated 2>/dev/null || printf 0)\" != 1 && test \"$(lipo -archs \"$candidate\")\" = x86_64",
        ),
        _ => ("false", "unsupported", "*unsupported*", "false"),
    };
    format!(
        r#"set -eu
candidate_dir="{ASSET_DIR}"
candidate="$candidate_dir/{asset}"
sidecar="$candidate_dir/{sidecar}"
provenance="$candidate_dir/{provenance}"
manifest="{MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}"
test -f "$candidate" && test ! -L "$candidate" && test -s "$candidate"
test -f "$sidecar" && test ! -L "$sidecar" && test -s "$sidecar"
test -f "$provenance" && test ! -L "$provenance" && test -s "$provenance"
test -f "$manifest" && test ! -L "$manifest" && test -s "$manifest"
chmod +x "$candidate"
entries="$(find "$candidate_dir" -mindepth 1 -maxdepth 1 -print | wc -l | tr -d ' ')"
test "$entries" = 3
manifest_entries="$(find "{MANIFEST_DIR}" -mindepth 1 -maxdepth 1 -print | wc -l | tr -d ' ')"
test "$manifest_entries" = 1
catalog_version() {{
  awk -F '"' -v name="$1" '$1 == "pub const " name ": &str = " && $3 == ";" {{ value = $2; count += 1 }} END {{ if (count != 1 || value !~ /^[0-9]+\.[0-9]+\.[0-9]+$/) exit 1; print value }}' crates/velnor-actions-mise/src/catalog.rs
}}
test "$GITHUB_WORKFLOW_SHA" = "$GITHUB_SHA"
test "$(git rev-parse HEAD)" = "$GITHUB_SHA"
manifest_line="$({sum} "$manifest")"
manifest_sha="${{manifest_line%% *}}"
test "$manifest_sha" = "$VELNOR_RELEASE_MANIFEST_SHA256"
jq -e --arg version "{version}" --arg repository "tailrocks/velnor-new" --arg commit "$GITHUB_SHA" --arg linux_target "{LINUX_TARGET}" --arg arm_target "{MACOS_ARM_TARGET}" --arg x64_target "{MACOS_X64_TARGET}" --arg linux_asset "{linux_asset}" --arg arm_asset "{arm_asset}" --arg x64_asset "{x64_asset}" --arg tag "v{version}" '
  .schema == 1 and .version == $version and .repository == $repository and .commit == $commit and
  (.targets | length == 3) and
  [.targets[].target] == [$linux_target, $arm_target, $x64_target] and
  all(.targets[];
    (.sha256 | test("^[0-9a-f]{{64}}$")) and
    ((.target == $linux_target and .artifact == ("https://github.com/" + $repository + "/releases/download/" + $tag + "/" + $linux_asset)) or
     (.target == $arm_target and .artifact == ("https://github.com/" + $repository + "/releases/download/" + $tag + "/" + $arm_asset)) or
     (.target == $x64_target and .artifact == ("https://github.com/" + $repository + "/releases/download/" + $tag + "/" + $x64_asset)))
  )
' "$manifest" >/dev/null
expected="$(jq -er --arg target "{target}" '[.targets[] | select(.target == $target)] | if length == 1 then .[0].sha256 else error("target_digest_missing") end' "$manifest")"
case "$expected" in ''|*[!0-9a-f]*) exit 1 ;; esac
test "${{#expected}}" -eq 64
sidecar_hash="$(awk -v name="{asset}" 'NR != 1 || NF != 2 || $2 != name || length($1) != 64 || $1 !~ /^[0-9a-f]+$/ {{ exit 1 }} {{ print $1 }} END {{ if (NR != 1) exit 1 }}' "$sidecar")"
test "$sidecar_hash" = "$expected"
actual_line="$({sum} "$candidate")"
actual="${{actual_line%% *}}"
test "$actual" = "$expected"
jq -e --arg version "{version}" --arg repository "$GITHUB_REPOSITORY" --arg commit "$GITHUB_SHA" --arg target "{target}" --arg asset "{asset}" --arg sha256 "$expected" --arg rust "$(catalog_version RUST_VERSION)" --arg mbx "$(catalog_version MR_BOXINGTON_VERSION)" '.schema == 1 and .version == $version and .repository == $repository and .commit == $commit and .target == $target and .asset == $asset and .sha256 == $sha256 and .toolchain.rust == $rust and .toolchain["mr-boxington"] == $mbx' "$provenance" >/dev/null
test "$(./"$candidate" --version)" = "velnor-actions {version}"
desc="$(file -b "$candidate")"
test "$RUNNER_OS:$(uname -m)" = "{host}"
case "$desc" in
  {file_pattern}) {native_check} ;;
  *) echo "candidate target mismatch: $desc" >&2; exit 1 ;;
esac
"#,
        linux_asset = assets.linux_bin,
        arm_asset = assets.macos_arm_bin,
        x64_asset = assets.macos_x64_bin,
        provenance = provenance,
    )
}

pub(super) fn qualify_candidate_script(target: &str, asset: &str, os: &str) -> String {
    let sum = match os {
        "linux" => "sha256sum --",
        "macos-arm64" | "macos-x64" => "shasum -a 256",
        _ => "false",
    };
    let mut script = qualification_preamble(target, asset, sum);
    script.push_str(&qualification_parity_steps(sum));
    script.push_str(
        "mise --no-config --no-env --no-hooks exec \"rust@$rust_version\" -- scripts/capture-opentofu-goldens.sh check-release \"$candidate\" \"$manifest\" \"$VELNOR_RELEASE_MANIFEST_SHA256\"\ncheck_candidate\n",
    );
    script
}

fn qualification_preamble(target: &str, asset: &str, sum: &str) -> String {
    format!(
        r#"set -eu
candidate="$GITHUB_WORKSPACE/{ASSET_DIR}/{asset}"
manifest="$GITHUB_WORKSPACE/{MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}"
manifest_line="$({sum} "$manifest")"
manifest_sha="${{manifest_line%% *}}"
test "$manifest_sha" = "$VELNOR_RELEASE_MANIFEST_SHA256"
expected="$(jq -er --arg target "{target}" '[.targets[] | select(.target == $target)] | if length == 1 then .[0].sha256 else error("target_digest_missing") end' "$manifest")"
"#
    )
}

fn qualification_parity_steps(sum: &str) -> String {
    format!(
        r#"check_candidate() {{
  actual_line="$({sum} "$candidate")"
  actual="${{actual_line%% *}}"
  test "$actual" = "$expected"
}}
check_candidate
catalog_version() {{
  awk -F '"' -v name="$1" '$1 == "pub const " name ": &str = " && $3 == ";" {{ value = $2; count += 1 }} END {{ if (count != 1 || value !~ /^[0-9]+\.[0-9]+\.[0-9]+$/) exit 1; print value }}' crates/velnor-actions-mise/src/catalog.rs
}}
rust_version="$(catalog_version RUST_VERSION)"
actionlint_version="$(catalog_version ACTIONLINT_VERSION)"
shellcheck_version="$(catalog_version SHELLCHECK_VERSION)"
zizmor_version="$(catalog_version ZIZMOR_VERSION)"
run_candidate() {{
  status=0
  mise --no-config --no-env --no-hooks exec "rust@$rust_version" "actionlint@$actionlint_version" "shellcheck@$shellcheck_version" "zizmor@$zizmor_version" -- "$candidate" "$@" || status="$?"
  check_candidate || return 1
  return "$status"
}}
temp="$(mktemp -d "$RUNNER_TEMP/velnor-release-qualification.XXXXXXXX")"
trap 'rm -rf "$temp"' EXIT
prepare_repo() {{
  case_name="$1"
  repo="$temp/$case_name"
  mkdir -p "$repo"
  cp -R "$GITHUB_WORKSPACE/fixtures/parity/$case_name/input/." "$repo/"
  git -C "$repo" init -q
  git -C "$repo" -c user.name='Velnor release qualification' -c user.email='release-qualification@users.noreply.github.com' add --all
  git -C "$repo" -c user.name='Velnor release qualification' -c user.email='release-qualification@users.noreply.github.com' commit -qm 'qualification fixture'
}}
for case_name in minimal-cargo multi-crate ignored-stack; do
  prepare_repo "$case_name"
  repo="$temp/$case_name"
  preview="$temp/$case_name-preview"
  (cd "$repo" && run_candidate plan > "$temp/$case_name-plan.stdout" 2> "$temp/$case_name-plan.stderr")
  test ! -s "$temp/$case_name-plan.stderr"
  repo_path="$(cd "$repo" && pwd -P)"
  head="$(git -C "$repo" rev-parse HEAD)"
  grep -Fq "Repository: $repo_path" "$temp/$case_name-plan.stdout"
  sed -e "s|Repository: $repo_path|Repository: <repo>|g" -e "s|$head|<head>|g" "$temp/$case_name-plan.stdout" > "$temp/$case_name-plan.normalized.txt"
  cmp "$temp/$case_name-plan.normalized.txt" "$GITHUB_WORKSPACE/fixtures/parity/$case_name/expected/plan.txt"
  (cd "$repo" && run_candidate generate --output-dir "$preview" > "$temp/$case_name-generate.stdout" 2> "$temp/$case_name-generate.stderr")
  cmp "$preview/.github/workflows/ci.yml" "$GITHUB_WORKSPACE/fixtures/parity/$case_name/expected/ci.yml"
  cmp "$preview/.github/actionlint.yaml" "$GITHUB_WORKSPACE/fixtures/parity/$case_name/expected/actionlint.yaml"
  test -z "$(git -C "$repo" status --porcelain)"
  check_candidate
done
for case_name in malformed malformed-ignored; do
  prepare_repo "$case_name"
  repo="$temp/$case_name"
  status=0
  (cd "$repo" && run_candidate plan > "$temp/$case_name-plan.stdout" 2> "$temp/$case_name-plan.stderr") || status="$?"
  test "$status" = "$(cat "$GITHUB_WORKSPACE/fixtures/parity/$case_name/expected/plan.exit")"
  sed 's/\(malformed_manifest:Cargo.toml: \).*/\1<cargo-diagnostic>/' "$temp/$case_name-plan.stderr" > "$temp/$case_name-plan.normalized.stderr"
  cmp "$temp/$case_name-plan.normalized.stderr" "$GITHUB_WORKSPACE/fixtures/parity/$case_name/expected/plan.stderr.txt"
  test -z "$(git -C "$repo" status --porcelain)"
  check_candidate
done
"#
    )
}
