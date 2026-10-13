//! Native, digest-bound qualification of the uploaded release candidates.

use crate::yaml::Yaml;

use super::{
    ASSET_DIR, AssetNames, LINUX_ARTIFACT, LINUX_TARGET, MACOS_ARM_ARTIFACT, MACOS_ARM_TARGET,
    MACOS_X64_ARTIFACT, MACOS_X64_TARGET, RELEASE_MANIFEST_FILENAME, release_steps, scripts,
};

const MANIFEST_ARTIFACT: &str = "generator-release-manifest";
const MANIFEST_DIR: &str = "release-manifest";

/// Three native target jobs that consume the exact uploaded binaries.
pub(super) fn jobs(
    linux: Yaml,
    macos_arm: Yaml,
    macos_x64: Yaml,
    version: &str,
    assets: &AssetNames,
) -> Vec<(String, Yaml)> {
    vec![
        target_job(
            TargetSpec {
                id: "qualify-linux-x64",
                name: "Qualify Linux x86_64 candidate",
                build: "build-linux-x64",
                target: LINUX_TARGET,
                artifact: LINUX_ARTIFACT,
                asset: &assets.linux_bin,
                sidecar: &assets.linux_sum,
                os: "linux",
            },
            linux,
            version,
            assets,
        ),
        target_job(
            TargetSpec {
                id: "qualify-macos-arm64",
                name: "Qualify macOS arm64 candidate",
                build: "build-macos-arm64",
                target: MACOS_ARM_TARGET,
                artifact: MACOS_ARM_ARTIFACT,
                asset: &assets.macos_arm_bin,
                sidecar: &assets.macos_arm_sum,
                os: "macos-arm64",
            },
            macos_arm,
            version,
            assets,
        ),
        target_job(
            TargetSpec {
                id: "qualify-macos-x64",
                name: "Qualify macOS x86_64 candidate",
                build: "build-macos-x64",
                target: MACOS_X64_TARGET,
                artifact: MACOS_X64_ARTIFACT,
                asset: &assets.macos_x64_bin,
                sidecar: &assets.macos_x64_sum,
                os: "macos-x64",
            },
            macos_x64,
            version,
            assets,
        ),
    ]
}

#[derive(Clone, Copy)]
struct TargetSpec<'a> {
    id: &'a str,
    name: &'a str,
    build: &'a str,
    target: &'a str,
    artifact: &'a str,
    asset: &'a str,
    sidecar: &'a str,
    os: &'a str,
}

fn target_job(
    spec: TargetSpec<'_>,
    runs_on: Yaml,
    version: &str,
    assets: &AssetNames,
) -> (String, Yaml) {
    let needs = ["release-gate", spec.build, "prepare-manifest"];
    let needs_success = needs
        .iter()
        .map(|job| format!("needs.{job}.result == 'success'"))
        .collect::<Vec<_>>()
        .join(" && ");
    let fields = super::with_if(
        super::with_needs(
            super::with_permissions(
                super::base(spec.name, runs_on, 60),
                release_steps::build_permissions(),
            ),
            &needs,
        ),
        &needs_success,
    );
    let mut steps = vec![release_steps::checkout_step(), release_steps::mise_step()];
    steps.push(release_steps::bash_run_step(
        "Install catalog Rust for candidate execution",
        &scripts::install_qualification_toolchain(),
    ));
    steps.push(release_steps::download_step(
        "Download exact candidate artifact",
        spec.artifact,
        ASSET_DIR,
    ));
    steps.push(release_steps::download_step(
        "Download attested release manifest",
        MANIFEST_ARTIFACT,
        MANIFEST_DIR,
    ));
    steps.push(release_steps::bash_run_step(
        "Bind downloaded candidate to manifest digest",
        &verify_candidate_script(
            spec.target,
            spec.asset,
            spec.sidecar,
            version,
            spec.os,
            assets,
        ),
    ));
    steps.push(release_steps::bash_run_step(
        "Qualify exact downloaded candidate",
        &qualify_candidate_script(spec.target, spec.asset, spec.os),
    ));
    super::finish(spec.id, fields, steps)
}

fn verify_candidate_script(
    target: &str,
    asset: &str,
    sidecar: &str,
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
manifest="{MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}"
test -f "$candidate" && test ! -L "$candidate" && test -s "$candidate"
test -f "$sidecar" && test ! -L "$sidecar" && test -s "$sidecar"
test -f "$manifest" && test ! -L "$manifest" && test -s "$manifest"
entries="$(find "$candidate_dir" -mindepth 1 -maxdepth 1 -print | wc -l | tr -d ' ')"
test "$entries" = 2
manifest_entries="$(find "{MANIFEST_DIR}" -mindepth 1 -maxdepth 1 -print | wc -l | tr -d ' ')"
test "$manifest_entries" = 1
jq -e --arg version "{version}" --arg repository "tailrocks/velnor-new" --arg commit "$GITHUB_SHA" --arg linux_target "{LINUX_TARGET}" --arg arm_target "{MACOS_ARM_TARGET}" --arg x64_target "{MACOS_X64_TARGET}" --arg linux_asset "{linux_asset}" --arg arm_asset "{arm_asset}" --arg x64_asset "{x64_asset}" --arg tag "generator-$GITHUB_SHA" '
  .schema == 1 and .version == $version and .repository == $repository and .commit == $commit and
  (.targets | length == 3) and
  ([.targets[].target] | sort) == ([$linux_target, $arm_target, $x64_target] | sort) and
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
desc="$(file -b "$candidate")"
test "$RUNNER_OS:$(uname -m)" = "{host}"
case "$desc" in
  {file_pattern}) {native_check} ;;
  *) echo "candidate target mismatch: $desc" >&2; exit 1 ;;
esac
chmod +x "$candidate"
"#,
        linux_asset = assets.linux_bin,
        arm_asset = assets.macos_arm_bin,
        x64_asset = assets.macos_x64_bin,
    )
}

fn qualify_candidate_script(target: &str, asset: &str, os: &str) -> String {
    let sum = match os {
        "linux" => "sha256sum --",
        "macos-arm64" | "macos-x64" => "shasum -a 256",
        _ => "false",
    };
    format!(
        r#"set -eu
candidate="$GITHUB_WORKSPACE/{ASSET_DIR}/{asset}"
manifest="$GITHUB_WORKSPACE/{MANIFEST_DIR}/{RELEASE_MANIFEST_FILENAME}"
expected="$(jq -er --arg target "{target}" '[.targets[] | select(.target == $target)] | if length == 1 then .[0].sha256 else error("target_digest_missing") end' "$manifest")"
check_candidate() {{
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
"#,
    )
}
