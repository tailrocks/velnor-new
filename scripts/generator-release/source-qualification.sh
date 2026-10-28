#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
set -euo pipefail
export MISE_NO_CONFIG=1

if [ "$#" -ne 1 ]; then
  printf 'usage: %s PHASE\n' "$0" >&2
  exit 64
fi

preserve_plan_diagnostics() {
  local workspace="$1" destination="$2" tmp_root="$RUNNER_TEMP/velnor-source-qualification-tmp"
  local physical_tmp workspace_parent workspace_base diagnostics file basename target
  if [ -L "$tmp_root" ] || [ ! -d "$tmp_root" ]; then
    printf 'controlled golden TMPDIR is absent or unsafe\n' >&2
    return 1
  fi
  physical_tmp="$(cd "$tmp_root" && pwd -P)" || return 1
  workspace_base="${workspace##*/}"
  if [[ ! "$workspace_base" =~ ^velnor-goldens-bin-test\.[A-Za-z0-9]+$ ]] \
    || [ -L "$workspace" ] || [ ! -d "$workspace" ]; then
    printf 'retained harness workspace is absent or unsafe\n' >&2
    return 1
  fi
  workspace_parent="$(cd "$workspace/.." && pwd -P)" || return 1
  if [ "$workspace_parent" != "$physical_tmp" ] \
    || [ "$(cd "$workspace" && pwd -P)" != "$workspace" ]; then
    printf 'retained harness workspace is outside physical TMPDIR\n' >&2
    return 1
  fi
  diagnostics="$workspace/plan-diagnostics"
  if [ -L "$diagnostics" ]; then
    printf 'retained plan diagnostics directory is a symlink\n' >&2
    return 1
  fi
  if [ ! -e "$diagnostics" ]; then return 0; fi
  if [ ! -d "$diagnostics" ] || [ -L "$destination" ] || [ ! -d "$destination" ]; then
    printf 'retained plan diagnostics paths are not safe directories\n' >&2
    return 1
  fi
  for file in "$diagnostics"/*.log "$diagnostics"/*.exit "$diagnostics"/*.txt "$diagnostics"/*.diff; do
    if [ ! -e "$file" ] && [ ! -L "$file" ]; then continue; fi
    basename="${file##*/}"
    if [ -L "$file" ] || [ ! -f "$file" ] \
      || [[ ! "$basename" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*\.(log|exit|txt|diff)$ ]]; then
      printf 'retained plan diagnostics file is unsafe: %s\n' "$basename" >&2
      return 1
    fi
    target="$destination/plan-diagnostics-$basename"
    if [ -e "$target" ] || [ -L "$target" ] || ! cp -p "$file" "$target"; then
      printf 'could not preserve retained plan diagnostics file: %s\n' "$basename" >&2
      return 1
    fi
  done
}

case "$1" in
  prepare-external-evidence-directories)
set -euo pipefail
case "$RUNNER_TEMP" in
  /*) ;;
  *) printf 'FATAL: RUNNER_TEMP must be absolute\n' >&2; exit 1 ;;
esac
workspace="$(cd "$GITHUB_WORKSPACE" && pwd -P)"
evidence="$RUNNER_TEMP/source-qualification-artifacts"
mkdir -p "$evidence/logs" "$evidence/previews"
mkdir -p "$RUNNER_TEMP/velnor-source-qualification-tmp"
mise_data="$RUNNER_TEMP/velnor-source-qualification-mise-data"
mise_cache="$RUNNER_TEMP/velnor-source-qualification-mise-cache"
mise_state="$RUNNER_TEMP/velnor-source-qualification-mise-state"
for path in "$mise_data" "$mise_cache" "$mise_state"; do
  if [ -e "$path" ] || [ -L "$path" ]; then
    printf 'FATAL: isolated Mise path already exists or is unsafe: %s\n' "$path" >&2
    exit 1
  fi
  mkdir -m 700 "$path"
done
evidence="$(cd "$evidence" && pwd -P)"
case "$evidence" in
  "$workspace"|"$workspace"/*)
    printf 'FATAL: qualification evidence must stay outside the checkout\n' >&2
    exit 1
    ;;
esac
{
  printf 'CARGO_TARGET_DIR=%s/velnor-source-qualification-target\n' "$RUNNER_TEMP"
  printf 'CARGO_HOME=%s/velnor-source-qualification-cargo-home\n' "$RUNNER_TEMP"
  printf 'MISE_DATA_DIR=%s\n' "$mise_data"
  printf 'MISE_CACHE_DIR=%s\n' "$mise_cache"
  printf 'MISE_STATE_DIR=%s\n' "$mise_state"
  printf 'MISE_AUTO_INSTALL=0\n'
} >>"$GITHUB_ENV"
{
  printf 'CARGO_TARGET_DIR=%s/velnor-source-qualification-target\n' "$RUNNER_TEMP"
  printf 'CARGO_HOME=%s/velnor-source-qualification-cargo-home\n' "$RUNNER_TEMP"
  printf 'MISE_DATA_DIR=%s\n' "$mise_data"
  printf 'MISE_CACHE_DIR=%s\n' "$mise_cache"
  printf 'MISE_STATE_DIR=%s\n' "$mise_state"
} >"$evidence/build-environment.txt"
    ;;
  verify-exact-clean-source-checkout)
set -euo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
actual_head="$(GIT_OPTIONAL_LOCKS=0 git rev-parse HEAD)"
actual_tree="$(GIT_OPTIONAL_LOCKS=0 git rev-parse 'HEAD^{tree}')"
test "$actual_head" = "$GITHUB_SHA"
test "$GITHUB_REPOSITORY" = tailrocks/velnor-new
pr_head_commit='not-applicable'
pr_head_tree='not-applicable'
pr_base_commit='not-applicable'
pr_base_tree='not-applicable'
merge_parent_1='not-applicable'
merge_parent_2='not-applicable'
if [ "$GITHUB_EVENT_NAME" = pull_request ]; then
  if ! [[ "$PR_HEAD_SHA" =~ ^[0-9a-f]{40}$ ]] \
    || ! [[ "$PR_BASE_SHA" =~ ^[0-9a-f]{40}$ ]]; then
    echo 'FATAL: pull_request event lacks exact head/base commit SHAs' >&2
    exit 1
  fi
  for rev in 'HEAD^1' 'HEAD^2' "${PR_HEAD_SHA}^{commit}" "${PR_BASE_SHA}^{commit}"; do
    if ! GIT_OPTIONAL_LOCKS=0 git cat-file -e "$rev" 2>/dev/null; then
      GIT_OPTIONAL_LOCKS=0 git fetch --no-tags --depth=2 origin "$GITHUB_SHA"
      break
    fi
  done
  pr_head_commit="$(GIT_OPTIONAL_LOCKS=0 git rev-parse "${PR_HEAD_SHA}^{commit}")"
  pr_head_tree="$(GIT_OPTIONAL_LOCKS=0 git rev-parse "${PR_HEAD_SHA}^{tree}")"
  pr_base_commit="$(GIT_OPTIONAL_LOCKS=0 git rev-parse "${PR_BASE_SHA}^{commit}")"
  pr_base_tree="$(GIT_OPTIONAL_LOCKS=0 git rev-parse "${PR_BASE_SHA}^{tree}")"
  merge_parent_1="$(GIT_OPTIONAL_LOCKS=0 git rev-parse 'HEAD^1')"
  merge_parent_2="$(GIT_OPTIONAL_LOCKS=0 git rev-parse 'HEAD^2')"
  test "$pr_head_commit" = "$PR_HEAD_SHA"
  test "$pr_base_commit" = "$PR_BASE_SHA"
  test "$merge_parent_1" = "$PR_BASE_SHA"
  test "$merge_parent_2" = "$PR_HEAD_SHA"
fi
GIT_OPTIONAL_LOCKS=0 git status --porcelain=v1 --untracked-files=all >"$evidence/checkout-status.txt"
test ! -s "$evidence/checkout-status.txt"
{
  printf 'event=%s\n' "$GITHUB_EVENT_NAME"
  printf 'repository=%s\n' "$GITHUB_REPOSITORY"
  printf 'ref=%s\n' "$GITHUB_REF"
  printf 'github_sha=%s\n' "$GITHUB_SHA"
  printf 'workflow_ref=%s\n' "$GITHUB_WORKFLOW_REF"
  printf 'workflow_sha=%s\n' "$GITHUB_WORKFLOW_SHA"
  printf 'checkout_head=%s\n' "$actual_head"
  printf 'checkout_tree=%s\n' "$actual_tree"
  printf 'pr_head_sha=%s\n' "$pr_head_commit"
  printf 'pr_head_tree=%s\n' "$pr_head_tree"
  printf 'pr_base_sha=%s\n' "$pr_base_commit"
  printf 'pr_base_tree=%s\n' "$pr_base_tree"
  printf 'merge_parent_1=%s\n' "$merge_parent_1"
  printf 'merge_parent_2=%s\n' "$merge_parent_2"
  printf 'worktree=clean\n'
  printf 'runner=%s/%s\n' "$RUNNER_OS" "$RUNNER_ARCH"
} | tee "$evidence/source-identity.txt"
: >"$evidence/source-inputs.sha256"
    for path in .mise-version mise.toml mise.lock rust-toolchain.toml .velnor/config.toml .velnor/mise.toml .velnor/mise.lock .velnor/generator.lock \
      .velnor/qualification/homebrew/jackin/transition.json .velnor/qualification/homebrew/jackin/cutover-author-policy.json \
      .github/workflows/source-qualification.yml scripts/generator-release/source-qualification.sh \
      scripts/capture-opentofu-goldens.sh scripts/test-capture-opentofu-goldens-bin.sh \
      scripts/generator-release/test-plan-diagnostics.sh scripts/generator-release/test-qualification-output-bindings.sh \
      scripts/generator-release/test-rust-verifier-retries.cjs \
      scripts/generator-release/qualification-validator-tools.cjs \
      scripts/generator-release/test-qualification-validator-tools.cjs \
      scripts/generator-release/source-qualification-validator-recipes.json; do
  if [ -f "$path" ] && [ ! -L "$path" ]; then
    sha256sum "$path" >>"$evidence/source-inputs.sha256"
  else
    printf 'absent-or-unsafe %s\n' "$path" >>"$evidence/source-inputs.sha256"
  fi
done
    ;;
  read-and-verify-repository-tool-pins)
set -euo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
mise_version="$(cat .mise-version)"
rust="$(mise --no-config --no-env --no-hooks config get -f mise.toml tools.rust.version)"
mbx="$(mise --no-config --no-env --no-hooks config get -f mise.toml tools.mr-boxington.version)"
nextest="$(mise --no-config --no-env --no-hooks config get -f mise.toml 'tools."aqua:nextest-rs/nextest/cargo-nextest"')"
jactionlint="$(mise --no-config --no-env --no-hooks config get -f mise.toml tools.jactionlint)"
shellcheck="$(mise --no-config --no-env --no-hooks config get -f mise.toml tools.shellcheck)"
channel="$(mise --no-config --no-env --no-hooks config get -f rust-toolchain.toml toolchain.channel)"
expected_mise=2026.10.21
expected_rust=1.99.1
expected_mbx=1.28.0
expected_nextest=0.9.150
expected_jactionlint=2.3.0
expected_shellcheck=0.11.0
if [ "$mise_version" != "$expected_mise" ] || [ "$rust" != "$expected_rust" ] \
  || [ "$mbx" != "$expected_mbx" ] || [ "$nextest" != "$expected_nextest" ] \
  || [ "$jactionlint" != "$expected_jactionlint" ] \
  || [ "$shellcheck" != "$expected_shellcheck" ] \
  || [ "$channel" != "$rust" ]; then
  printf 'repository tool pin mismatch: mise=%s rust=%s mbx=%s nextest=%s jactionlint=%s shellcheck=%s channel=%s\n' \
    "$mise_version" "$rust" "$mbx" "$nextest" "$jactionlint" "$shellcheck" "$channel" >&2
  exit 1
fi
bun_version="$(mise --no-config --no-env --no-hooks config get -f mise.toml tools.bun)"
expected_bun=1.4.4
if [ "$bun_version" != "$expected_bun" ]; then
  printf 'repository Bun pin mismatch: expected %s, found %s\n' "$expected_bun" "$bun_version" >&2
  exit 1
fi
mise --no-config --no-env --no-hooks install "bun@$bun_version"
bun_path="$(mise --no-config --no-env --no-hooks which bun --tool "bun@$bun_version")"
"$bun_path" --version | tee "$evidence/bun-version.txt"
grep -Fx "$bun_version" "$evidence/bun-version.txt"
"$bun_path" --bun run -- - \
  "$mise_version" "$rust" "$mbx" "$nextest" "$jactionlint" "$shellcheck" \
  "$channel" "$bun_version" "$evidence/tool-pins.json" "$GITHUB_ENV" <<'JS'
  const fs = require("node:fs");
  const crypto = require("node:crypto");
  const [mise, rust, mbx, nextest, jactionlint, shellcheck, channel, bun, receiptPath, envPath] = process.argv.slice(2);
  const files = [".mise-version", "mise.toml", "mise.lock", "rust-toolchain.toml"];
  const source_sha256 = Object.fromEntries(files.map((file) => [
    file,
    crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex"),
  ]));
  const receipt = {
    versions: {
      mise, rust, "mr-boxington": mbx, nextest, jactionlint, shellcheck, bun,
    },
    rust_toolchain_channel: channel, bun, source_sha256,
  };
  fs.writeFileSync(receiptPath, `${JSON.stringify(receipt, null, 2)}\n`);
  fs.appendFileSync(envPath,
    `VELNOR_MISE_VERSION=${mise}\n` +
    `VELNOR_RUST_VERSION=${rust}\n` +
    `VELNOR_MBX_VERSION=${mbx}\n` +
    `VELNOR_NEXTEST_VERSION=${nextest}\n` +
    `VELNOR_JACTIONLINT_VERSION=${jactionlint}\n` +
    `VELNOR_SHELLCHECK_VERSION=${shellcheck}\n` +
    `VELNOR_BUN_VERSION=${bun}\n`);
JS
mise --version | tee "$evidence/mise-version.txt"
grep -Eq "(^|[[:space:]])2026\.10\.21([[:space:]]|$)" "$evidence/mise-version.txt"
    ;;
  install-pinned-rust-and-mbx)
set -euo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
mise --no-config --no-env --no-hooks install \
  "rust@$VELNOR_RUST_VERSION" "mr-boxington@$VELNOR_MBX_VERSION" \
  >"$evidence/logs/tool-install.log" 2>&1
cat "$evidence/logs/tool-install.log"
    ;;
  record-installed-tool-identities)
set -euo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
mise --no-config --no-env --no-hooks exec "rust@$VELNOR_RUST_VERSION" -- rustc --version \
  >"$evidence/rustc-version.txt"
mise --no-config --no-env --no-hooks exec "mr-boxington@$VELNOR_MBX_VERSION" -- mbx --version \
  >"$evidence/mbx-version.txt"
bun_path="$(mise --no-config --no-env --no-hooks which bun --tool "bun@$VELNOR_BUN_VERSION")"
"$bun_path" --version >"$evidence/bun-version.txt"
cat "$evidence/rustc-version.txt" "$evidence/mbx-version.txt" "$evidence/bun-version.txt"
grep -F "rustc $VELNOR_RUST_VERSION " "$evidence/rustc-version.txt"
grep -Eq "(^|[^0-9])$VELNOR_MBX_VERSION([^0-9]|$)" "$evidence/mbx-version.txt"
grep -Fx "$VELNOR_BUN_VERSION" "$evidence/bun-version.txt"
    ;;
  prefetch-locked-sources-for-qualification-workspaces)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
root="$GITHUB_WORKSPACE"
log="$evidence/logs/cargo-prefetch.log"
status_file="$evidence/logs/cargo-prefetch.exit"
before="$evidence/cargo-fetch-locks.before.txt"
after="$evidence/cargo-fetch-locks.after.txt"
identity="$evidence/cargo-fetch-lock-identity.txt"
manifests=(Cargo.toml crates/velnor-runner/Cargo.toml)
locks=(Cargo.lock crates/velnor-runner/Cargo.lock)
final_status=0
: >"$log"
: >"$identity"
record_lock_hashes() {
  local destination="$1" lock digest
  : >"$destination" || return 1
  for lock in "${locks[@]}"; do
    if [ -L "$root/$lock" ] || [ ! -f "$root/$lock" ]; then
      printf 'invalid lockfile: %s\n' "$lock" >&2
      return 1
    fi
    if ! digest="$(sha256sum -- "$root/$lock" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')" \
      || [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
      printf 'could not calculate lockfile SHA-256: %s\n' "$lock" >&2
      return 1
    fi
    printf '%s  %s\n' "$digest" "$lock" >>"$destination" || return 1
  done
}
if ! record_lock_hashes "$before"; then
  final_status=1
  printf 'lockfile baseline capture failed\n' >>"$log"
else
  for manifest in "${manifests[@]}"; do
    if [ -L "$root/$manifest" ] || [ ! -f "$root/$manifest" ]; then
      printf 'invalid manifest: %s\n' "$manifest" >>"$log"
      final_status=1
      continue
    fi
    printf 'command=mise --no-config --no-env --no-hooks exec rust@%s mr-boxington@%s -- cargo fetch --locked --manifest-path %s/%s\n' \
      "$VELNOR_RUST_VERSION" "$VELNOR_MBX_VERSION" "$root" "$manifest" >>"$log"
    mise --no-config --no-env --no-hooks exec \
      "rust@$VELNOR_RUST_VERSION" "mr-boxington@$VELNOR_MBX_VERSION" -- \
      cargo fetch --locked --manifest-path "$root/$manifest" >>"$log" 2>&1
    command_status=$?
    printf 'manifest=%s status=%s\n' "$manifest" "$command_status" >>"$log"
    if [ "$command_status" -ne 0 ]; then final_status=1; fi
  done
fi
if ! record_lock_hashes "$after"; then
  final_status=1
  printf 'lockfile post-fetch capture failed\n' >>"$log"
elif [ -f "$before" ] && ! cmp -s "$before" "$after"; then
  final_status=1
  printf 'lockfile hashes changed during cargo fetch\n' >>"$log"
fi
{
  printf 'source_head=%s\n' "$GITHUB_SHA"
  printf 'fetch_scope=root-and-runner\n'
  printf 'target_filter=none\n'
  printf 'locked=true\n'
  printf 'before_sha256\n'
  cat "$before"
  printf 'after_sha256\n'
  cat "$after"
} >"$identity" || final_status=1
printf '%s\n' "$final_status" >"$status_file" || exit 1
cat "$log"
exit "$final_status"
    ;;
  build-runner-native-archive-guard-from-this-checkout)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
log="$evidence/logs/archive-guard-build.log"
receipt="$evidence/archive-guard-identity.txt"
printf '%s\n' 'bash scripts/build-owned-archive-guard.sh' >"$evidence/logs/archive-guard-build.command"
set +e
bash scripts/build-owned-archive-guard.sh >"$log" 2>&1
command_status=$?
cat "$log"
identity_status=0
artifact_status=0
guard="$GITHUB_WORKSPACE/.velnor/archive-guard/bin/velnor-archive-guard"
fingerprint=unavailable
source_fingerprint=unavailable
digest=unavailable
file_identity=unavailable
if [ "$command_status" -ne 0 ]; then
  identity_status=1
  printf 'guard identity unavailable because the host-native build failed\n' >&2
elif [ -L "$guard" ] || [ ! -f "$guard" ] || [ ! -x "$guard" ]; then
  identity_status=1
  printf 'FATAL: builder did not produce a regular executable guard in the checkout\n' >&2
else
  if ! fingerprint="$("$guard" --fingerprint)" \
    || ! source_fingerprint="$("$guard" source-fingerprint "$GITHUB_WORKSPACE")" \
    || [[ ! "$fingerprint" =~ ^[0-9a-f]{64}$ ]] \
    || [ "$source_fingerprint" != "$fingerprint" ]; then
    identity_status=1
    printf 'FATAL: runner-native archive guard fingerprint does not match this checkout\n' >&2
  fi
  if ! digest="$(sha256sum "$guard" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')" \
    || [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
    identity_status=1
    digest=unavailable
    printf 'FATAL: could not calculate the archive guard SHA-256\n' >&2
  fi
  if ! file_identity="$(stat -c '%u:%a:%h:%s' "$guard")"; then
    identity_status=1
    file_identity=unavailable
    printf 'FATAL: could not inspect the archive guard identity\n' >&2
  fi
  owner="${file_identity%%:*}"
  remainder="${file_identity#*:}"
  mode="${remainder%%:*}"
  remainder="${remainder#*:}"
  links="${remainder%%:*}"
  size="${remainder#*:}"
  if [[ ! "$owner" =~ ^[0-9]+$ || "$owner" != "$(id -u)" \
    || ! "$mode" =~ ^[0-7]+$ || ! "$links" =~ ^[0-9]+$ \
    || "$links" != 1 || ! "$size" =~ ^[0-9]+$ ]] \
    || (( (8#$mode & 022) != 0 || (8#$mode & 0111) == 0 )); then
    identity_status=1
    printf 'FATAL: archive guard file ownership or permissions are unsafe\n' >&2
  fi
  if ! {
    printf 'source_head=%s\n' "$GITHUB_SHA"
    printf 'host=%s/%s\n' "$RUNNER_OS" "$RUNNER_ARCH"
    printf 'builder=scripts/build-owned-archive-guard.sh\n'
    printf 'binary_path=%s\n' "$guard"
    printf 'binary_sha256=%s\n' "$digest"
    printf 'binary_identity=%s\n' "$file_identity"
    printf 'source_fingerprint=%s\n' "$source_fingerprint"
  } >"$receipt"; then
    artifact_status=1
    printf 'FATAL: could not write the archive guard identity receipt\n' >&2
  fi
fi
printf '%s\n' "$command_status" >"$evidence/logs/archive-guard-build.command.exit" || artifact_status=1
printf '%s\n' "$identity_status" >"$evidence/logs/archive-guard-build.identity.exit" || artifact_status=1
printf '%s\n' "$artifact_status" >"$evidence/logs/archive-guard-build.artifact.exit" || artifact_status=1
final_status=0
if [ "$command_status" -ne 0 ] || [ "$identity_status" -ne 0 ] || [ "$artifact_status" -ne 0 ]; then
  final_status=1
fi
if [ "$final_status" -eq 0 ]; then
  {
    printf 'VELNOR_QUALIFICATION_ARCHIVE_GUARD=%s\n' "$guard"
    printf 'VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256=%s\n' "$digest"
  } >>"$GITHUB_ENV" || final_status=1
fi
printf '%s\n' "$final_status" >"$evidence/logs/archive-guard-build.exit" || exit 1
if [ "$artifact_status" -ne 0 ]; then
  printf 'artifact_status=1\n' >"$evidence/logs/archive-guard-build.artifact.exit" || true
fi
exit "$final_status"
    ;;
  install-validator-tools-with-native-mise-checksums)
set -uo pipefail
export MISE_BACKENDS_JACTIONLINT=github:jdx/jactionlint
export MISE_BACKENDS_SHELLCHECK=github:koalaman/shellcheck
evidence="$RUNNER_TEMP/source-qualification-artifacts"
tool_plan="$evidence/validator-tool-install-plan.json"
tool_specs="$evidence/validator-tool-install-specs.txt"
log="$evidence/logs/validator-tool-install.log"
status_file="$evidence/logs/validator-tool-install.exit"
if [ -L mise.toml ] || [ ! -f mise.toml ] || [ -L mise.lock ] || [ ! -f mise.lock ]; then
  printf 'FATAL: committed validator Mise config or lock is missing or unsafe\n' >"$log"
  printf '1\n' >"$status_file"
  exit 1
fi
validator_jactionlint="$(MISE_NO_CONFIG=1 mise --no-config --no-env --no-hooks config get \
  -f mise.toml tools.jactionlint)"
validator_shellcheck="$(MISE_NO_CONFIG=1 mise --no-config --no-env --no-hooks config get \
  -f mise.toml tools.shellcheck)"
if [ "$validator_jactionlint" != 2.3.0 ] || [ "$validator_shellcheck" != 0.11.0 ]; then
  printf 'FATAL: committed validator Mise pins differ from qualification pins\n' >"$log"
  printf '1\n' >"$status_file"
  exit 1
fi
platform=''
case "$RUNNER_OS/$RUNNER_ARCH" in
  Linux/X64) platform=linux-x64 ;;
  macOS/ARM64) platform=macos-arm64 ;;
  *) printf 'FATAL: unsupported validator runner platform: %s/%s\n' "$RUNNER_OS" "$RUNNER_ARCH" >"$log"; printf '1\n' >"$status_file"; exit 1 ;;
esac
lock_before="$(sha256sum mise.lock | awk '{print $1}')"
config_before="$(sha256sum mise.toml | awk '{print $1}')"
{
  printf 'committed_config_path=mise.toml\ncommitted_config_sha256_before=%s\n' "$config_before"
  printf 'committed_lock_path=mise.lock\ncommitted_lock_sha256_before=%s\n' "$lock_before"
} >"$evidence/validator-tool-config-identity.before.txt"
set +e
validator_bun="$(mise --no-config --no-env --no-hooks which bun --tool "bun@$VELNOR_BUN_VERSION")"
spec_output="$(MISE_NO_CONFIG=1 "$validator_bun" --bun run scripts/generator-release/qualification-validator-tools.cjs plan mise.lock \
  "$platform" "$validator_jactionlint" "$validator_shellcheck" "$tool_plan" 2>"$log")"
plan_status=$?
set -e
spec_status=0
if [ "$plan_status" -eq 0 ]; then
  printf '%s\n' "$spec_output" >"$tool_specs"
  specs=()
  while IFS= read -r spec; do specs+=("$spec"); done <"$tool_specs"
  if [ "${#specs[@]}" -ne 2 ] || [ -L "$tool_plan" ] || [ ! -f "$tool_plan" ]; then spec_status=1; fi
else
  spec_status=1
fi
if [ "$spec_status" -eq 0 ]; then
  printf 'install_command=MISE_NO_CONFIG=1 mise --no-config --no-env --no-hooks install <two validated lock-derived specs>\n' >"$evidence/logs/validator-tool-install.command"
  set +e
  MISE_NO_CONFIG=1 mise --no-config --no-env --no-hooks install "${specs[@]}" >>"$log" 2>&1
  command_status=$?
  set -e
else
  command_status=1
  printf 'FATAL: validator tool lock plan was rejected before installation\n' >>"$log"
fi
cat "$log"
identity_status=0
if [ "$(sha256sum mise.toml | awk '{print $1}')" != "$config_before" ] \
  || [ "$(sha256sum mise.lock | awk '{print $1}')" != "$lock_before" ]; then
  identity_status=1
  printf 'FATAL: native validator install changed config or lock bytes\n' >&2
fi
jactionlint_version=unavailable
shellcheck_version=unavailable
jactionlint_which=unavailable
shellcheck_which=unavailable
jactionlint_binary_sha=unavailable
shellcheck_binary_sha=unavailable
if [ "$command_status" -eq 0 ]; then
  if MISE_NO_CONFIG=1 MISE_BACKENDS_JACTIONLINT=github:jdx/jactionlint \
    MISE_BACKENDS_SHELLCHECK=github:koalaman/shellcheck \
    mise --no-config --no-env --no-hooks which --tool \
    jactionlint@2.3.0 jactionlint >"$evidence/logs/jactionlint-which.log" 2>&1; then
    jactionlint_which="$(cat "$evidence/logs/jactionlint-which.log")"
    if [ -f "$jactionlint_which" ] && [ ! -L "$jactionlint_which" ] \
      && [ -x "$jactionlint_which" ] \
      && MISE_NO_CONFIG=1 MISE_BACKENDS_JACTIONLINT=github:jdx/jactionlint \
      MISE_BACKENDS_SHELLCHECK=github:koalaman/shellcheck \
      mise --no-config --no-env --no-hooks exec \
      jactionlint@2.3.0 -- jactionlint --version \
      >"$evidence/logs/jactionlint-version.log" 2>&1 \
      && IFS= read -r jactionlint_version \
        <"$evidence/logs/jactionlint-version.log" \
      && [ "$jactionlint_version" = 2.3.0 ]; then
      if ! jactionlint_binary_sha="$(sha256sum "$jactionlint_which" | awk '{print $1}')" \
        || [[ ! "$jactionlint_binary_sha" =~ ^[0-9a-f]{64}$ ]]; then
        identity_status=1
      fi
    else
      identity_status=1
    fi
  else
    identity_status=1
  fi
  if MISE_NO_CONFIG=1 MISE_BACKENDS_JACTIONLINT=github:jdx/jactionlint \
    MISE_BACKENDS_SHELLCHECK=github:koalaman/shellcheck \
    mise --no-config --no-env --no-hooks which --tool \
    shellcheck@0.11.0 shellcheck >"$evidence/logs/shellcheck-which.log" 2>&1; then
    shellcheck_which="$(cat "$evidence/logs/shellcheck-which.log")"
    if [ -f "$shellcheck_which" ] && [ ! -L "$shellcheck_which" ] \
      && [ -x "$shellcheck_which" ] \
      && MISE_NO_CONFIG=1 MISE_BACKENDS_JACTIONLINT=github:jdx/jactionlint \
      MISE_BACKENDS_SHELLCHECK=github:koalaman/shellcheck \
      mise --no-config --no-env --no-hooks exec \
      shellcheck@0.11.0 -- shellcheck --version \
      >"$evidence/logs/shellcheck-version.log" 2>&1 \
      && grep -Fxq 'version: 0.11.0' "$evidence/logs/shellcheck-version.log"; then
      shellcheck_version=0.11.0
      if ! shellcheck_binary_sha="$(sha256sum "$shellcheck_which" | awk '{print $1}')" \
        || [[ ! "$shellcheck_binary_sha" =~ ^[0-9a-f]{64}$ ]]; then
        identity_status=1
      fi
    else
      identity_status=1
    fi
  else
    identity_status=1
  fi
else
  identity_status=1
fi
if [ "$identity_status" -eq 0 ]; then
  cp -p "$evidence/logs/jactionlint-version.log" "$evidence/jactionlint-version.txt"
  cp -p "$evidence/logs/shellcheck-version.log" "$evidence/shellcheck-version.txt"
fi
{
  printf 'source_head=%s\n' "$GITHUB_SHA"
  printf 'platform=%s/%s\n' "$RUNNER_OS" "$RUNNER_ARCH"
  printf 'mise_data_dir=%s\n' "$MISE_DATA_DIR"
  printf 'committed_config_sha256_before=%s\n' "$config_before"
  printf 'committed_config_sha256_after=%s\n' "$(sha256sum mise.toml | awk '{print $1}')"
  printf 'committed_lock_sha256_before=%s\n' "$lock_before"
  printf 'committed_lock_sha256_after=%s\n' "$(sha256sum mise.lock | awk '{print $1}')"
  printf 'validator_plan=%s\n' "$tool_plan"
  printf 'jactionlint_root_backend=github:jdx/jactionlint\n'
  printf 'jactionlint_installer_backend=github:jdx/jactionlint\n'
  printf 'jactionlint_version=%s\n' "$jactionlint_version"
  printf 'jactionlint_binary_path=%s\n' "$jactionlint_which"
  printf 'jactionlint_binary_sha256=%s\n' "$jactionlint_binary_sha"
  printf 'shellcheck_root_backend=aqua:koalaman/shellcheck\n'
  printf 'shellcheck_installer_backend=github:koalaman/shellcheck\n'
  printf 'shellcheck_version=%s\n' "$shellcheck_version"
  printf 'shellcheck_binary_path=%s\n' "$shellcheck_which"
  printf 'shellcheck_binary_sha256=%s\n' "$shellcheck_binary_sha"
} >"$evidence/validator-tool-identity.txt"
printf '%s\n' "$command_status" >"$evidence/logs/validator-tool-install.command.exit"
printf '%s\n' "$spec_status" >"$evidence/logs/validator-tool-install.plan.exit"
printf '%s\n' "$identity_status" >"$evidence/logs/validator-tool-install.identity.exit"
final_status=0
if [ "$spec_status" -ne 0 ] || [ "$command_status" -ne 0 ] || [ "$identity_status" -ne 0 ]; then
  final_status=1
  printf 'FATAL: native validator checksum installation or identity check failed\n' >&2
fi
printf '%s\n' "$final_status" >"$status_file"
exit "$final_status"
    ;;
  test-validator-tool-lock-admission)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
name='validator-tool-lock-admission'
receipt="$evidence/validator-tool-lock-admission.txt"
log="$evidence/logs/$name.log"
set +e
MISE_NO_CONFIG=1 "$(mise --no-config --no-env --no-hooks which bun --tool "bun@$VELNOR_BUN_VERSION")" --bun run scripts/generator-release/test-qualification-validator-tools.cjs \
  scripts/generator-release/qualification-validator-tools.cjs mise.lock \
  >"$log" 2>&1
command_status=$?
set -u
if ! cat "$log"; then command_status=1; fi
artifact_status=0
if [ "$command_status" -eq 0 ]; then
  printf 'source_head=%s\nlock_sha256=%s\nplatform=%s/%s\n' \
    "$GITHUB_SHA" "$(sha256sum mise.lock | awk '{print $1}')" "$RUNNER_OS" "$RUNNER_ARCH" >"$receipt" \
    || artifact_status=1
else
  rm -f "$receipt"
fi
printf '%s\n' "$command_status" >"$evidence/logs/$name.command.exit" || artifact_status=1
printf '%s\n' "$artifact_status" >"$evidence/logs/$name.artifact.exit" || artifact_status=1
final_status=0
if [ "$command_status" -ne 0 ] || [ "$artifact_status" -ne 0 ]; then final_status=1; fi
printf '%s\n' "$final_status" >"$evidence/logs/$name.exit" || exit 1
exit "$final_status"
    ;;
  build-source-cli-with-pinned-mbx)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
mise --no-config --no-env --no-hooks exec \
  "rust@$VELNOR_RUST_VERSION" "mr-boxington@$VELNOR_MBX_VERSION" -- \
  mbx build --locked --release --package velnor-actions-cli --bin velnor-actions \
  --example qualification-diagnostics \
  >"$evidence/logs/cli-build.log" 2>&1
command_status=$?
identity_status=0
artifact_status=0
binary="$CARGO_TARGET_DIR/release/velnor-actions"
if ! cat "$evidence/logs/cli-build.log"; then artifact_status=1; fi
version=unavailable
digest=unavailable
size=unavailable
expected_version=unavailable
if [ "$command_status" -ne 0 ]; then
  identity_status=1
  printf 'CLI identity unavailable because the build command failed\n' >&2
elif [ ! -f "$binary" ] || [ -L "$binary" ] || [ ! -x "$binary" ]; then
  identity_status=1
  printf 'FATAL: successful build did not produce a regular executable CLI at %s\n' "$binary" >&2
else
  if ! expected_version="$(mise --no-config --no-env --no-hooks config get -f Cargo.toml workspace.package.version)"; then
    identity_status=1
    printf 'FATAL: could not read the workspace package version\n' >&2
  fi
  if ! version="$("$binary" --version)"; then
    identity_status=1
    version=unavailable
    printf 'FATAL: built CLI version command failed\n' >&2
  elif [ "$version" != "velnor-actions $expected_version" ]; then
    identity_status=1
    printf 'FATAL: built CLI version %s does not match expected velnor-actions %s\n' "$version" "$expected_version" >&2
  fi
  if ! digest="$(sha256sum "$binary" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')" \
    || [[ ! "$digest" =~ ^[0-9a-f]{64}$ ]]; then
    identity_status=1
    digest=unavailable
    printf 'FATAL: could not compute the built CLI SHA-256\n' >&2
  fi
  if ! size="$(stat -c '%s' "$binary")" || [[ ! "$size" =~ ^[0-9]+$ ]]; then
    identity_status=1
    size=unavailable
    printf 'FATAL: could not read the built CLI size\n' >&2
  fi
  identity_tmp="$evidence/cli-identity.txt.tmp"
  if ! {
    printf 'source_head=%s\n' "$GITHUB_SHA"
    printf 'expected_version=velnor-actions %s\n' "$expected_version"
    printf 'binary_path=%s\n' "$binary"
    printf 'binary_version=%s\n' "$version"
    printf 'binary_sha256=%s\n' "$digest"
    printf 'binary_size=%s\n' "$size"
    printf 'mise=%s\n' "$VELNOR_MISE_VERSION"
    printf 'rust=%s\n' "$VELNOR_RUST_VERSION"
    printf 'mr_boxington=%s\n' "$VELNOR_MBX_VERSION"
    printf 'nextest_pin=%s\n' "$VELNOR_NEXTEST_VERSION"
  } >"$identity_tmp" || ! mv "$identity_tmp" "$evidence/cli-identity.txt"; then
    artifact_status=1
    printf 'FATAL: could not write CLI identity receipt\n' >&2
  else
    cat "$evidence/cli-identity.txt" || artifact_status=1
  fi
fi
fixture="$CARGO_TARGET_DIR/release/examples/qualification-diagnostics"
if [ "$command_status" -eq 0 ] && [ "$identity_status" -eq 0 ]; then
  if [ -L "$fixture" ] || [ ! -f "$fixture" ] || [ ! -x "$fixture" ] \
    || [ "$("$fixture" --version)" != "$version" ]; then
    identity_status=1
  else
    fixture_digest="$(sha256sum "$fixture" | awk '{print $1}')"
    if [[ ! "$fixture_digest" =~ ^[0-9a-f]{64}$ ]]; then
      identity_status=1
    elif ! {
      printf 'diagnostics_fixture_path=%s\n' "$fixture"
      printf 'diagnostics_fixture_sha256=%s\n' "$fixture_digest"
    } >>"$evidence/cli-identity.txt"; then
      artifact_status=1
    elif ! {
      printf 'VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE=%s\n' "$fixture"
      printf 'VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE_SHA256=%s\n' "$fixture_digest"
    } >>"$GITHUB_ENV"; then
      artifact_status=1
    fi
  fi
fi
printf '%s\n' "$command_status" >"$evidence/logs/cli-build.command.exit" || artifact_status=1
printf '%s\n' "$identity_status" >"$evidence/logs/cli-build.identity.exit" || artifact_status=1
printf '%s\n' "$artifact_status" >"$evidence/logs/cli-build.artifact.exit" || artifact_status=1
final_status=0
if [ "$command_status" -ne 0 ] || [ "$identity_status" -ne 0 ] || [ "$artifact_status" -ne 0 ]; then
  final_status=1
fi
printf '%s\n' "$final_status" >"$evidence/logs/cli-build.exit" || exit 1
if [ "$artifact_status" -ne 0 ]; then
  printf 'artifact_status=1\n' >"$evidence/logs/cli-build.artifact.exit" || true
fi
exit "$final_status"
    ;;
  qualify-linux-x64-native-validator-tools)
if [ "${GITHUB_EVENT_NAME:-}" != workflow_dispatch ]; then printf 'FATAL: native validator authoring requires a workflow_dispatch event\n' >&2; exit 64; fi
set -uo pipefail
for path in "${RUNNER_TEMP:-}" "${CARGO_TARGET_DIR:-}"; do case "$path" in /*) ;; *) printf 'FATAL: qualification paths must be absolute\n' >&2; exit 1 ;; esac; done
evidence="$RUNNER_TEMP/source-qualification-artifacts"
phase_dir="$evidence/native-validator-tool-qualification"
if [ -L "$evidence" ] || [ ! -d "$evidence" ] || [ -L "$evidence/logs" ] || [ ! -d "$evidence/logs" ] \
  || [ -e "$phase_dir" ] || [ -L "$phase_dir" ] || ! mkdir -m 700 "$phase_dir"; then
  printf 'FATAL: native validator evidence path is unavailable\n' >&2; exit 1
fi
status_file="$phase_dir/status.txt" log="$phase_dir/qualification.log" request="$phase_dir/request.json" candidate="$phase_dir/candidate.json"
archive_root="$phase_dir/archives" work_dir="$phase_dir/work" binary="$CARGO_TARGET_DIR/release/velnor-actions"
identity="$evidence/cli-identity.txt" source_head="${GITHUB_SHA:-unavailable}" plan="$evidence/validator-tool-install-plan.json"
admission="$evidence/validator-tool-lock-admission.txt"
record_qualification_status() {
  local exit_status=$? result=failed
  [ "$exit_status" -eq 0 ] && result=success
  trap - EXIT
  if ! printf 'source_head=%s\nevent=%s\nresult=%s\nexit_status=%s\n' "$source_head" "$GITHUB_EVENT_NAME" "$result" "$exit_status" >"$status_file"; then [ "$exit_status" -ne 0 ] || exit_status=1; fi
  exit "$exit_status"
}
trap record_qualification_status EXIT
umask 077
fail_qualification() { printf 'FATAL: %s\n' "$1" >&2; exit 1; }
if [ -L "$identity" ] || [ ! -f "$identity" ] || [ -L "$plan" ] || [ ! -f "$plan" ] || [ -L "$admission" ] || [ ! -f "$admission" ] \
  || ! grep -Fxq 0 "$evidence/logs/validator-tool-install.exit" || ! grep -Fxq 0 "$evidence/logs/validator-tool-lock-admission.exit"; then fail_qualification 'admitted plan or source CLI identity is unavailable'; fi
if [ -L "$binary" ] || [ ! -f "$binary" ] || [ ! -x "$binary" ]; then fail_qualification 'source CLI is unavailable'; fi
cli_version="$("$binary" --version)"
cli_sha256="$(sha256sum "$binary" | cut -d' ' -f1)"
if [[ ! "$source_head" =~ ^[0-9a-f]{40}$ ]] || [[ ! "$cli_sha256" =~ ^[0-9a-f]{64}$ ]] \
  || ! grep -Fqx "source_head=$source_head" "$identity" || ! grep -Fqx "binary_path=$binary" "$identity" \
  || ! grep -Fqx "expected_version=$cli_version" "$identity" || ! grep -Fqx "binary_version=$cli_version" "$identity" \
  || ! grep -Fqx "binary_sha256=$cli_sha256" "$identity"; then fail_qualification 'source CLI differs from its build identity receipt'; fi
lock_sha256="$(sha256sum mise.lock | cut -d' ' -f1)"
if ! grep -Fqx "source_head=$source_head" "$admission" || ! grep -Fqx "lock_sha256=$lock_sha256" "$admission"; then fail_qualification 'validator lock admission receipt differs from this source'; fi
plan_values="$(jq -er --arg lock_sha256 "$lock_sha256" 'if .lock_sha256 == $lock_sha256 and .platform == "linux-x64" and ([.tools[].tool] == ["jactionlint", "shellcheck"]) and all(.tools[]; (.url | startswith("https://")) and (.checksum | test("^sha256:[0-9a-f]{64}$"))) then [.tools[0].url, (.tools[0].checksum | ltrimstr("sha256:")), .tools[1].url, (.tools[1].checksum | ltrimstr("sha256:"))] | @tsv else error("validator plan differs from admitted lock") end' "$plan")" || fail_qualification 'validator plan differs from admitted Linux x64 lock'
IFS=$'\t' read -r jactionlint_url jactionlint_sha256 shellcheck_url shellcheck_sha256 <<<"$plan_values"
jactionlint_archive="$archive_root/jactionlint-linux-x64.tar.gz"
shellcheck_archive="$archive_root/shellcheck-linux-x64.tar.xz"
mkdir -m 700 "$archive_root" || fail_qualification 'could not create archive directory'
download_archive() { curl --fail --location --silent --show-error --proto '=https' --proto-redir '=https' --connect-timeout 15 --max-time 300 --retry 3 --retry-all-errors --retry-delay 1 --output "$2" "$1" >>"$log" 2>&1; }
if ! download_archive "$jactionlint_url" "$jactionlint_archive" \
  || ! download_archive "$shellcheck_url" "$shellcheck_archive"; then
  cat "$log" >&2
  fail_qualification 'pinned validator archive download failed'
fi
if [ -L "$jactionlint_archive" ] || [ ! -f "$jactionlint_archive" ] || [ -L "$shellcheck_archive" ] || [ ! -f "$shellcheck_archive" ]; then fail_qualification 'validator archive downloads are unsafe'; fi
if ! jq --arg jpath "$jactionlint_archive" --arg jurl "$jactionlint_url" --arg jsha "$jactionlint_sha256" \
  --arg spath "$shellcheck_archive" --arg surl "$shellcheck_url" --arg ssha "$shellcheck_sha256" \
  '.tools[0].recipe.platforms[0].artifacts[0].url=$jurl | .tools[0].archives=[{path:$jpath,url:$jurl,sha256:$jsha}] | .tools[1].recipe.platforms[0].artifacts[0].url=$surl | .tools[1].archives=[{path:$spath,url:$surl,sha256:$ssha}]' \
  scripts/generator-release/source-qualification-validator-recipes.json >"$request" 2>>"$log"; then cat "$log" >&2; fail_qualification 'native validator request could not be encoded'; fi
if ! "$binary" qualify-tools --request "$request" --work-dir "$work_dir" \
  --output "$candidate" >>"$log" 2>&1; then
  cat "$log" >&2
  fail_qualification 'source CLI rejected Linux x64 validator authoring'
fi
cat "$log"
    ;;
  capture-all-five-external-goldens)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
tmp="$RUNNER_TEMP/velnor-source-qualification-tmp"
oracle="$evidence/external-oracle"
binary="$CARGO_TARGET_DIR/release/velnor-actions"
if [ ! -f "$binary" ] || [ ! -x "$binary" ]; then
  printf 'CLI binary unavailable; capture not run\n' >"$evidence/logs/external-capture.log"
  printf '1\n' >"$evidence/logs/external-capture.command.exit"
  printf '0\n' >"$evidence/logs/external-capture.artifact.exit"
  printf '1\n' >"$evidence/logs/external-capture.exit"
  printf 'capture workspace unavailable\n' >"$evidence/logs/external-capture-workspace.txt"
  exit 1
fi
mkdir -p "$oracle"
set +e
TMPDIR="$tmp" VELNOR_GOLDEN_DIR="$oracle" VELNOR_GOLDEN_KEEP_WORK=1 \
  scripts/capture-opentofu-goldens.sh \
  --archive-guard "$VELNOR_QUALIFICATION_ARCHIVE_GUARD" \
  --archive-guard-sha256 "$VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256" capture "$binary" \
  >"$evidence/logs/external-capture.log" 2>&1
command_status=$?
artifact_status=0
cat "$evidence/logs/external-capture.log"
workspace="$(sed -n 's/^retaining \(successful golden evidence\|failed golden\) workspace: //p' "$evidence/logs/external-capture.log" | tail -n 1)"
printf '%s\n' "$workspace" >"$evidence/logs/external-capture-workspace.txt"
case "$workspace" in
  "$tmp"/velnor-goldens-work.*)
    if [ ! -L "$workspace" ] && [ ! -L "$workspace/stage" ] && [ -d "$workspace/stage" ]; then
      rm -rf "$evidence/captured-stage"
      if ! cp -RP "$workspace/stage" "$evidence/captured-stage"; then
        printf 'could not preserve retained capture stage\n' >>"$evidence/logs/external-capture-workspace.txt"
        artifact_status=1
      fi
    else
      printf 'retained workspace has no stage directory\n' >>"$evidence/logs/external-capture-workspace.txt"
      artifact_status=1
    fi
    if [ ! -L "$workspace/raw" ] && [ -d "$workspace/raw" ]; then
      rm -rf "$evidence/captured-raw"
      if ! cp -RP "$workspace/raw" "$evidence/captured-raw"; then
        printf 'could not preserve retained raw capture logs\n' >>"$evidence/logs/external-capture-workspace.txt"
        artifact_status=1
      fi
    else
      printf 'retained workspace has no safe raw log directory\n' >>"$evidence/logs/external-capture-workspace.txt"
      artifact_status=1
    fi
    ;;
  *)
    printf 'retained workspace path was absent or outside controlled TMPDIR\n' >>"$evidence/logs/external-capture-workspace.txt"
    artifact_status=1
    ;;
esac
final_status=$command_status
if [ "$final_status" -eq 0 ] && [ "$artifact_status" -ne 0 ]; then final_status=1; fi
printf '%s\n' "$command_status" >"$evidence/logs/external-capture.command.exit"
printf '%s\n' "$artifact_status" >"$evidence/logs/external-capture.artifact.exit"
printf '%s\n' "$final_status" >"$evidence/logs/external-capture.exit"
exit "$final_status"
    ;;
  repeat-check-against-external-oracle)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
tmp="$RUNNER_TEMP/velnor-source-qualification-tmp"
oracle="$evidence/external-oracle"
binary="$CARGO_TARGET_DIR/release/velnor-actions"
if [ ! -x "$binary" ] || [ ! -d "$oracle/cases" ]; then
  printf 'CLI or externally captured oracle is unavailable\n' >"$evidence/logs/external-check.log"
  printf '1\n' >"$evidence/logs/external-check.command.exit"
  printf '0\n' >"$evidence/logs/external-check.artifact.exit"
  printf '1\n' >"$evidence/logs/external-check.exit"
  printf 'check workspace unavailable\n' >"$evidence/logs/external-check-workspace.txt"
  exit 1
fi
set +e
TMPDIR="$tmp" VELNOR_GOLDEN_DIR="$oracle" VELNOR_GOLDEN_KEEP_WORK=1 \
  scripts/capture-opentofu-goldens.sh \
  --archive-guard "$VELNOR_QUALIFICATION_ARCHIVE_GUARD" \
  --archive-guard-sha256 "$VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256" check "$binary" \
  >"$evidence/logs/external-check.log" 2>&1
command_status=$?
artifact_status=0
cat "$evidence/logs/external-check.log"
workspace="$(sed -n 's/^retaining \(successful golden evidence\|failed golden\) workspace: //p' "$evidence/logs/external-check.log" | tail -n 1)"
printf '%s\n' "$workspace" >"$evidence/logs/external-check-workspace.txt"
case "$workspace" in
  "$tmp"/velnor-goldens-work.*)
    if [ ! -L "$workspace" ] && [ ! -L "$workspace/stage" ] && [ -d "$workspace/stage" ]; then
      rm -rf "$evidence/external-check-stage"
      if ! cp -RP "$workspace/stage" "$evidence/external-check-stage"; then
        printf 'could not preserve external check stage\n' >>"$evidence/logs/external-check-workspace.txt"
        artifact_status=1
      fi
    else
      printf 'retained check workspace has no safe stage directory\n' >>"$evidence/logs/external-check-workspace.txt"
      artifact_status=1
    fi
    if [ ! -L "$workspace/raw" ] && [ -d "$workspace/raw" ]; then
      rm -rf "$evidence/external-check-raw"
      if ! cp -RP "$workspace/raw" "$evidence/external-check-raw"; then
        printf 'could not preserve external check raw logs\n' >>"$evidence/logs/external-check-workspace.txt"
        artifact_status=1
      fi
    else
      printf 'retained check workspace has no safe raw log directory\n' >>"$evidence/logs/external-check-workspace.txt"
      artifact_status=1
    fi
    if [ ! -L "$workspace/check-diff" ] && [ -d "$workspace/check-diff" ]; then
      rm -rf "$evidence/external-check-diff"
      if ! cp -RP "$workspace/check-diff" "$evidence/external-check-diff"; then
        printf 'could not preserve external check differences\n' >>"$evidence/logs/external-check-workspace.txt"
        artifact_status=1
      fi
    fi
    ;;
  *)
    printf 'retained workspace path was absent or outside controlled TMPDIR\n' >>"$evidence/logs/external-check-workspace.txt"
    artifact_status=1
    ;;
esac
final_status=$command_status
if [ "$final_status" -eq 0 ] && [ "$artifact_status" -ne 0 ]; then final_status=1; fi
printf '%s\n' "$command_status" >"$evidence/logs/external-check.command.exit"
printf '%s\n' "$artifact_status" >"$evidence/logs/external-check.artifact.exit"
printf '%s\n' "$final_status" >"$evidence/logs/external-check.exit"
exit "$final_status"
    ;;
  run-full-golden-shell-harness-against-external-oracle)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
binary="$CARGO_TARGET_DIR/release/velnor-actions"
oracle="$evidence/external-oracle"
if [ ! -x "$binary" ] || [ ! -d "$oracle/cases" ]; then
  printf 'CLI or externally captured oracle is unavailable\n' >"$evidence/logs/external-shell-harness.log"
  printf '1\n' >"$evidence/logs/external-shell-harness.command.exit"
  printf '0\n' >"$evidence/logs/external-shell-harness.artifact.exit"
  printf '1\n' >"$evidence/logs/external-shell-harness.exit"
  printf 'harness workspace unavailable\n' >"$evidence/logs/external-shell-harness-workspace.txt"
  exit 1
fi
set +e
bun_path="$(mise --no-config --no-env --no-hooks which bun --tool "bun@$VELNOR_BUN_VERSION")" || exit 2
PATH="${bun_path%/*}:$PATH" VELNOR_BUN_BIN="$bun_path" VELNOR_BUN_VERSION="$VELNOR_BUN_VERSION" \
  TMPDIR="$RUNNER_TEMP/velnor-source-qualification-tmp" VELNOR_GOLDEN_DIR="$oracle" VELNOR_GOLDEN_KEEP_WORK=0 \
  bash scripts/test-capture-opentofu-goldens-bin.sh \
  --archive-guard "$VELNOR_QUALIFICATION_ARCHIVE_GUARD" \
  --archive-guard-sha256 "$VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256" \
  --diagnostics-fixture "$VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE" \
  --diagnostics-fixture-sha256 "$VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE_SHA256" "$binary" \
  >"$evidence/logs/external-shell-harness.log" 2>&1
command_status=$?
artifact_status=0
cat "$evidence/logs/external-shell-harness.log"
workspace="$(sed -n 's/^retaining failed golden harness workspace: //p' "$evidence/logs/external-shell-harness.log" | tail -n 1)"
printf '%s\n' "$workspace" >"$evidence/logs/external-shell-harness-workspace.txt"
case "$workspace" in
  "$RUNNER_TEMP"/velnor-source-qualification-tmp/velnor-goldens-bin-test.*)
    mkdir -p "$evidence/external-shell-harness-logs"
    for file in "$workspace"/*.log "$workspace"/*.diff; do
      if [ -f "$file" ] && [ ! -L "$file" ]; then
        if ! cp -p "$file" "$evidence/external-shell-harness-logs/${file##*/}"; then
          printf 'could not preserve %s\n' "$file" >>"$evidence/logs/external-shell-harness-workspace.txt"
          artifact_status=1
        fi
      fi
    done
    if ! preserve_plan_diagnostics "$workspace" "$evidence/external-shell-harness-logs"; then
      printf 'could not safely preserve plan diagnostics files\n' >>"$evidence/logs/external-shell-harness-workspace.txt"
      artifact_status=1
    fi
    ;;
  *)
    if [ "$command_status" -ne 0 ]; then
      printf 'failed harness workspace was absent or outside controlled TMPDIR\n' >>"$evidence/logs/external-shell-harness-workspace.txt"
      artifact_status=1
    fi
    ;;
esac
final_status=$command_status
if [ "$final_status" -eq 0 ] && [ "$artifact_status" -ne 0 ]; then final_status=1; fi
printf '%s\n' "$command_status" >"$evidence/logs/external-shell-harness.command.exit"
printf '%s\n' "$artifact_status" >"$evidence/logs/external-shell-harness.artifact.exit"
printf '%s\n' "$final_status" >"$evidence/logs/external-shell-harness.exit"
exit "$final_status"
    ;;
  run-full-golden-shell-harness-with-retained-workspaces)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
binary="$CARGO_TARGET_DIR/release/velnor-actions"
oracle="$evidence/external-oracle"
name='external-shell-harness-keep'
if [ ! -x "$binary" ] || [ ! -d "$oracle/cases" ]; then
  printf 'CLI or externally captured oracle is unavailable\n' >"$evidence/logs/$name.log"
  printf '1\n' >"$evidence/logs/$name.command.exit"
  printf '0\n' >"$evidence/logs/$name.artifact.exit"
  printf '1\n' >"$evidence/logs/$name.exit"
  printf 'harness workspace unavailable\n' >"$evidence/logs/$name-workspace.txt"
  exit 1
fi
set +e
bun_path="$(mise --no-config --no-env --no-hooks which bun --tool "bun@$VELNOR_BUN_VERSION")" || exit 2
PATH="${bun_path%/*}:$PATH" VELNOR_BUN_BIN="$bun_path" VELNOR_BUN_VERSION="$VELNOR_BUN_VERSION" \
  TMPDIR="$RUNNER_TEMP/velnor-source-qualification-tmp" VELNOR_GOLDEN_DIR="$oracle" \
  VELNOR_GOLDEN_KEEP_WORK=1 bash scripts/test-capture-opentofu-goldens-bin.sh \
  --archive-guard "$VELNOR_QUALIFICATION_ARCHIVE_GUARD" \
  --archive-guard-sha256 "$VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256" \
  --diagnostics-fixture "$VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE" \
  --diagnostics-fixture-sha256 "$VELNOR_QUALIFICATION_DIAGNOSTICS_FIXTURE_SHA256" "$binary" \
  >"$evidence/logs/$name.log" 2>&1
command_status=$?
artifact_status=0
cat "$evidence/logs/$name.log"
workspace="$(sed -n 's/^retaining \(successful golden harness evidence\|failed golden harness\) workspace: //p' "$evidence/logs/$name.log" | tail -n 1)"
printf '%s\n' "$workspace" >"$evidence/logs/$name-workspace.txt"
case "$workspace" in
  "$RUNNER_TEMP"/velnor-source-qualification-tmp/velnor-goldens-bin-test.*)
    if [ ! -L "$workspace" ] && [ -d "$workspace" ]; then
      mkdir -p "$evidence/external-shell-harness-keep-logs"
      for file in "$workspace"/*.log "$workspace"/*.diff; do
        if [ -f "$file" ] && [ ! -L "$file" ]; then
          if ! cp -p "$file" "$evidence/external-shell-harness-keep-logs/${file##*/}"; then
            printf 'could not preserve %s\n' "$file" >>"$evidence/logs/$name-workspace.txt"
            artifact_status=1
          fi
        fi
      done
      if ! preserve_plan_diagnostics "$workspace" "$evidence/external-shell-harness-keep-logs"; then
        printf 'could not safely preserve plan diagnostics files\n' >>"$evidence/logs/$name-workspace.txt"
        artifact_status=1
      fi
    else
      printf 'retained harness workspace is not a safe directory\n' >>"$evidence/logs/$name-workspace.txt"
      artifact_status=1
    fi
    ;;
  *)
    printf 'retained harness workspace was absent or outside controlled TMPDIR\n' >>"$evidence/logs/$name-workspace.txt"
    artifact_status=1
    ;;
esac
final_status=$command_status
if [ "$final_status" -eq 0 ] && [ "$artifact_status" -ne 0 ]; then final_status=1; fi
printf '%s\n' "$command_status" >"$evidence/logs/$name.command.exit"
printf '%s\n' "$artifact_status" >"$evidence/logs/$name.artifact.exit"
printf '%s\n' "$final_status" >"$evidence/logs/$name.exit"
exit "$final_status"
    ;;
  preserve-output-binding-json-proof-files)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
tmp_root="$RUNNER_TEMP/velnor-source-qualification-tmp"
name='external-shell-harness-json-proof'
proof_status=0

preserve_mode_proofs() {
  local mode="$1" workspace_file="$2" command_file="$3" destination="$4"
  local command_status workspace require_complete mode_status copied missing
  local root_physical workspace_physical relative source target target_parent
  local expected_count
  local files=(
    'output-binding manifest.json'
    'output-binding duplicate manifest.json'
    'output-binding normalized manifest.json'
  )
  expected_count=${#files[@]}
  command_status=unavailable
  workspace=''
  mode_status=0
  copied=0
  missing=0

  if [ -L "$command_file" ] || [ ! -f "$command_file" ]; then
    printf 'invalid or missing harness command status: %s\n' "$command_file" >&2
    mode_status=1
  else
    command_status="$(cat "$command_file")"
    if [[ ! "$command_status" =~ ^[0-9]+$ ]]; then
      printf 'malformed harness command status: %s\n' "$command_file" >&2
      mode_status=1
    fi
  fi
  if [ -L "$workspace_file" ] || [ ! -f "$workspace_file" ]; then
    printf 'invalid or missing harness workspace receipt: %s\n' "$workspace_file" >&2
    mode_status=1
  elif [ "$(wc -l <"$workspace_file" | tr -d '[:space:]')" != 1 ]; then
    printf 'harness workspace receipt must contain one line: %s\n' "$workspace_file" >&2
    mode_status=1
  else
    IFS= read -r workspace <"$workspace_file" || mode_status=1
  fi

  if [ "$mode_status" -eq 0 ] && [ "$mode" = ordinary ] \
    && [ "$command_status" = 0 ] && [ -z "$workspace" ]; then
    printf 'mode=ordinary\ncommand_status=0\nworkspace=not-retained-on-success\ncopied=0\nmissing=%s\nproof_status=0\n' \
      "$expected_count" >"$evidence/$destination.receipt.txt" || mode_status=1
    if [ "$mode_status" -ne 0 ]; then proof_status=1; fi
    return 0
  fi

  if [ -z "$workspace" ]; then
    printf 'retained %s harness workspace is missing\n' "$mode" >&2
    mode_status=1
  elif [ -L "$RUNNER_TEMP" ] || [ -L "$tmp_root" ] || [ ! -d "$tmp_root" ]; then
    printf 'harness temporary parent is missing or a symlink\n' >&2
    mode_status=1
  else
    root_physical="$(cd "$tmp_root" && pwd -P)" || root_physical=''
    case "$workspace" in
      "$tmp_root"/velnor-goldens-bin-test.*) ;;
      *) printf 'retained %s workspace is outside the controlled temporary root\n' "$mode" >&2; mode_status=1 ;;
    esac
    if [ "$(dirname -- "$workspace")" != "$tmp_root" ] \
      || [ "$root_physical" != "$tmp_root" ] \
      || [ -L "$workspace" ] || [ ! -d "$workspace" ]; then
      printf 'retained %s workspace or parent is missing, noncanonical, or a symlink\n' "$mode" >&2
      mode_status=1
    else
      workspace_physical="$(cd "$workspace" && pwd -P)" || workspace_physical=''
      if [ "$workspace_physical" != "$workspace" ]; then
        printf 'retained %s workspace traverses a symlink\n' "$mode" >&2
        mode_status=1
      fi
    fi
  fi

  if [ "$mode_status" -eq 0 ]; then
    if [ -L "$evidence" ] || [ ! -d "$evidence" ] \
      || [ -L "$evidence/$destination" ] \
      || { [ -e "$evidence/$destination" ] && [ ! -d "$evidence/$destination" ]; }; then
      printf 'proof destination is unsafe: %s\n' "$evidence/$destination" >&2
      mode_status=1
    elif ! mkdir -p "$evidence/$destination"; then
      printf 'could not create proof destination: %s\n' "$evidence/$destination" >&2
      mode_status=1
    elif [ "$(cd "$evidence/$destination" && pwd -P)" != "$evidence/$destination" ]; then
      printf 'proof destination traverses a symlink: %s\n' "$evidence/$destination" >&2
      mode_status=1
    fi
  fi

  if [ "$mode_status" -eq 0 ]; then
    for relative in "${files[@]}"; do
      source="$workspace/$relative"
      if [ -L "$source" ] || { [ -e "$source" ] && [ ! -f "$source" ]; }; then
        printf 'proof file is not a regular non-symlink file: %s\n' "$source" >&2
        mode_status=1
        continue
      fi
      if [ ! -f "$source" ]; then
        missing=$((missing + 1))
        continue
      fi
      target="$evidence/$destination/$relative"
      target_parent="$(dirname -- "$target")"
      if ! mkdir -p "$target_parent" \
        || [ -L "$target_parent" ] || [ -e "$target" ] || [ -L "$target" ] \
        || ! cp -p -- "$source" "$target"; then
        printf 'could not safely preserve proof file: %s\n' "$source" >&2
        mode_status=1
        continue
      fi
      copied=$((copied + 1))
    done
  fi

  require_complete=0
  if [ "$mode" = keep ] && [ "$command_status" = 0 ]; then
    require_complete=1
  fi
  if [ "$require_complete" -eq 1 ] && [ "$copied" -ne "$expected_count" ]; then
    printf 'successful KEEP_WORK run retained %s of %s required JSON proof files\n' \
      "$copied" "$expected_count" >&2
    mode_status=1
  fi
  if ! {
    printf 'mode=%s\n' "$mode"
    printf 'command_status=%s\n' "$command_status"
    printf 'workspace=%s\n' "$workspace"
    printf 'copied=%s\n' "$copied"
    printf 'missing=%s\n' "$missing"
    printf 'expected=%s\n' "$expected_count"
    printf 'proof_status=%s\n' "$mode_status"
  } >"$evidence/$destination.receipt.txt"; then
    mode_status=1
  fi
  if [ "$mode_status" -ne 0 ]; then proof_status=1; fi
  return 0
}

preserve_mode_proofs ordinary \
  "$evidence/logs/external-shell-harness-workspace.txt" \
  "$evidence/logs/external-shell-harness.command.exit" \
  external-shell-harness-json-proof
preserve_mode_proofs keep \
  "$evidence/logs/external-shell-harness-keep-workspace.txt" \
  "$evidence/logs/external-shell-harness-keep.command.exit" \
  external-shell-harness-keep-json-proof
printf '%s\n' 0 >"$evidence/logs/$name.command.exit" || proof_status=1
printf '%s\n' "$proof_status" >"$evidence/logs/$name.artifact.exit" || proof_status=1
printf '%s\n' "$proof_status" >"$evidence/logs/$name.exit" || exit 1
exit "$proof_status"
    ;;
  generate-twice-and-retain-root-to-preview-identities)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
preview_root="$evidence/previews"
binary="$CARGO_TARGET_DIR/release/velnor-actions"
if [ ! -x "$binary" ]; then
  printf 'CLI binary unavailable; previews not generated\n' >"$evidence/logs/root-preview.log"
  printf 'preview statuses unavailable\n' >"$evidence/logs/root-preview.exit"
  exit 1
fi
set +e
"$binary" generate --output-dir "$preview_root/generated-1" \
  >"$evidence/logs/root-preview-1.log" 2>&1
first_status=$?
printf '%s\n' "$first_status" >"$evidence/logs/root-preview-1.exit"
"$binary" generate --output-dir "$preview_root/generated-2" \
  >"$evidence/logs/root-preview-2.log" 2>&1
second_status=$?
printf '%s\n' "$second_status" >"$evidence/logs/root-preview-2.exit"
cat "$evidence/logs/root-preview-1.log" "$evidence/logs/root-preview-2.log"
"$(mise --no-config --no-env --no-hooks which bun --tool "bun@$VELNOR_BUN_VERSION")" --bun run -- - \
  "$GITHUB_WORKSPACE/.github" "$preview_root/generated-1/.github" \
  "$preview_root/generated-2/.github" "$evidence/previews" \
  >"$evidence/logs/root-preview-identity.log" 2>&1 <<'JS'
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const [root, first, second, output] = process.argv.slice(2);
fs.mkdirSync(output, { recursive: true });
const sortJson = (value) => {
  if (Array.isArray(value)) return value.map(sortJson);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, sortJson(value[key])]));
  }
  return value;
};
const inventory = (base) => {
  let rootStat;
  try {
    rootStat = fs.lstatSync(base);
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
  if (!rootStat.isDirectory() || rootStat.isSymbolicLink()) return null;
  const rows = [];
  const walk = (directory, prefix = "") => {
    const names = fs.readdirSync(directory).sort((left, right) =>
      Buffer.compare(Buffer.from(left), Buffer.from(right)));
    for (const name of names) {
      const relative = prefix ? `${prefix}/${name}` : name;
      const entryPath = path.join(directory, name);
      const info = fs.lstatSync(entryPath);
      const mode = info.mode & 0o7777;
      if (info.isSymbolicLink()) {
        rows.push({ path: relative, type: "symlink", mode, target: fs.readlinkSync(entryPath) });
      } else if (info.isDirectory()) {
        rows.push({ path: relative, type: "directory", mode });
        walk(entryPath, relative);
      } else if (info.isFile()) {
        const sha256 = crypto.createHash("sha256").update(fs.readFileSync(entryPath)).digest("hex");
        rows.push({ path: relative, type: "file", mode, sha256 });
      } else {
        rows.push({ path: relative, type: "special", mode });
      }
    }
  };
  walk(base);
  return rows;
};
const inventories = {
  repository_github: inventory(root),
  generated_preview_1_github: inventory(first),
  generated_preview_2_github: inventory(second),
};
fs.writeFileSync(path.join(output, "github-identities.json"),
  `${JSON.stringify(sortJson(inventories), null, 2)}\n`);
if (Object.values(inventories).some((rows) => rows === null)) {
  process.stderr.write("missing or invalid .github tree\n");
  process.exitCode = 1;
}
JS
identity_status=$?
set +e
jq -S '.repository_github' "$evidence/previews/github-identities.json" \
  >"$evidence/previews/repository-github.json" || identity_status=1
jq -S '.generated_preview_1_github' "$evidence/previews/github-identities.json" \
  >"$evidence/previews/generated-preview-1-github.json" || identity_status=1
jq -S '.generated_preview_2_github' "$evidence/previews/github-identities.json" \
  >"$evidence/previews/generated-preview-2-github.json" || identity_status=1
diff -u --label repository_github --label generated_preview_1_github \
  "$evidence/previews/repository-github.json" "$evidence/previews/generated-preview-1-github.json" \
  >"$evidence/previews/repository-vs-preview.diff"
first_raw_diff_status=$?
diff -u --label generated_preview_1_github --label generated_preview_2_github \
  "$evidence/previews/generated-preview-1-github.json" "$evidence/previews/generated-preview-2-github.json" \
  >"$evidence/previews/preview-idempotence.diff"
second_raw_diff_status=$?
diff -ruN "$GITHUB_WORKSPACE/.github" "$preview_root/generated-1/.github" \
  >"$evidence/previews/repository-vs-preview.raw.diff" 2>&1
first_content_diff_status=$?
diff -ruN "$preview_root/generated-1/.github" "$preview_root/generated-2/.github" \
  >"$evidence/previews/preview-idempotence.raw.diff" 2>&1
second_content_diff_status=$?
printf 'repository_preview_diff_status=%s\npreview_idempotence_diff_status=%s\n' \
  "$first_content_diff_status" "$second_content_diff_status" >"$evidence/previews/raw-diff-status.txt"
printf 'repository_preview_diff_status=%s\npreview_idempotence_diff_status=%s\n' \
  "$first_raw_diff_status" "$second_raw_diff_status" >"$evidence/previews/diff-status.txt"
cat "$evidence/logs/root-preview-identity.log"
if [ "$first_status" -ne 0 ] || [ "$second_status" -ne 0 ] \
  || [ "$identity_status" -ne 0 ] || [ "$first_raw_diff_status" -ne 0 ] \
  || [ "$second_raw_diff_status" -ne 0 ] \
  || [ "$first_content_diff_status" -gt 1 ] || [ "$second_content_diff_status" -gt 1 ]; then
  printf 'generate_statuses=%s,%s identity_status=%s diff_statuses=%s,%s\n' \
    "$first_status" "$second_status" "$identity_status" \
    "$first_raw_diff_status" "$second_raw_diff_status" >"$evidence/logs/root-preview.exit"
  exit 1
fi
printf 'generate_statuses=0,0 identity_status=0 diff_statuses=%s,%s\n' \
  "$first_raw_diff_status" "$second_raw_diff_status" >"$evidence/logs/root-preview.exit"
    ;;
  test-generated-rust-verifier-transport-and-digest-gate)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
name='rust-verifier-retry'
action="$evidence/previews/generated-1/.github/actions/velnor-verify-restored-rust/action.yml"
receipt="$evidence/rust-verifier-retry.json"
log="$evidence/logs/$name.log"
if [ -L "$evidence" ] || [ ! -d "$evidence" ]; then
  printf 'source qualification evidence directory is missing or unsafe\n' >&2
  exit 1
fi
set +e
"$(mise --no-config --no-env --no-hooks which bun --tool "bun@$VELNOR_BUN_VERSION")" --bun run scripts/generator-release/test-rust-verifier-retries.cjs \
  "$action" "$receipt" >"$log" 2>&1
command_status=$?
set -u
if ! cat "$log"; then command_status=1; fi
artifact_status=0
if [ -L "$receipt" ] || [ ! -f "$receipt" ]; then
  printf 'Rust verifier TLS retry receipt is missing or unsafe\n' >&2
  artifact_status=1
fi
printf '%s\n' "$command_status" >"$evidence/logs/$name.command.exit" || artifact_status=1
printf '%s\n' "$artifact_status" >"$evidence/logs/$name.artifact.exit" || artifact_status=1
final_status=0
if [ "$command_status" -ne 0 ] || [ "$artifact_status" -ne 0 ]; then final_status=1; fi
printf '%s\n' "$final_status" >"$evidence/logs/$name.exit" || exit 1
exit "$final_status"
    ;;
  check-source-output-against-checked-in-repository-goldens)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
binary="$CARGO_TARGET_DIR/release/velnor-actions"
if [ ! -x "$binary" ]; then
  printf 'CLI binary unavailable; repository oracle check not run\n' >"$evidence/logs/repository-oracle-check.log"
  printf '1\n' >"$evidence/logs/repository-oracle-check.command.exit"
  printf '0\n' >"$evidence/logs/repository-oracle-check.artifact.exit"
  printf '1\n' >"$evidence/logs/repository-oracle-check.exit"
  printf 'repository oracle workspace unavailable\n' >"$evidence/logs/repository-oracle-workspace.txt"
  exit 1
fi
set +e
env -u VELNOR_GOLDEN_DIR TMPDIR="$RUNNER_TEMP/velnor-source-qualification-tmp" \
  VELNOR_GOLDEN_KEEP_WORK=1 scripts/capture-opentofu-goldens.sh \
  --archive-guard "$VELNOR_QUALIFICATION_ARCHIVE_GUARD" \
  --archive-guard-sha256 "$VELNOR_QUALIFICATION_ARCHIVE_GUARD_SHA256" check "$binary" \
  >"$evidence/logs/repository-oracle-check.log" 2>&1
command_status=$?
artifact_status=0
cat "$evidence/logs/repository-oracle-check.log"
workspace="$(sed -n 's/^retaining \(successful golden evidence\|failed golden\) workspace: //p' "$evidence/logs/repository-oracle-check.log" | tail -n 1)"
printf '%s\n' "$workspace" >"$evidence/logs/repository-oracle-workspace.txt"
case "$workspace" in
  "$RUNNER_TEMP"/velnor-source-qualification-tmp/velnor-goldens-work.*)
    if [ ! -L "$workspace" ] && [ ! -L "$workspace/stage" ] && [ -d "$workspace/stage" ]; then
      rm -rf "$evidence/repository-oracle-stage"
      if ! cp -RP "$workspace/stage" "$evidence/repository-oracle-stage"; then
        printf 'could not preserve repository oracle stage\n' >>"$evidence/logs/repository-oracle-workspace.txt"
        artifact_status=1
      fi
    else
      printf 'retained repository oracle workspace has no safe stage directory\n' >>"$evidence/logs/repository-oracle-workspace.txt"
      artifact_status=1
    fi
    if [ ! -L "$workspace/raw" ] && [ -d "$workspace/raw" ]; then
      rm -rf "$evidence/repository-oracle-raw"
      if ! cp -RP "$workspace/raw" "$evidence/repository-oracle-raw"; then
        printf 'could not preserve repository oracle raw logs\n' >>"$evidence/logs/repository-oracle-workspace.txt"
        artifact_status=1
      fi
    else
      printf 'retained repository oracle workspace has no safe raw log directory\n' >>"$evidence/logs/repository-oracle-workspace.txt"
      artifact_status=1
    fi
    if [ ! -L "$workspace/check-diff" ] && [ -d "$workspace/check-diff" ]; then
      rm -rf "$evidence/repository-oracle-diff"
      if ! cp -RP "$workspace/check-diff" "$evidence/repository-oracle-diff"; then
        printf 'could not preserve repository oracle differences\n' >>"$evidence/logs/repository-oracle-workspace.txt"
        artifact_status=1
      fi
    elif [ "$command_status" -ne 0 ]; then
      printf 'failed repository oracle check retained no safe diff directory\n' >>"$evidence/logs/repository-oracle-workspace.txt"
    fi
    ;;
  *)
    printf 'retained workspace path was absent or outside controlled TMPDIR\n' >>"$evidence/logs/repository-oracle-workspace.txt"
    artifact_status=1
    ;;
esac
final_status=$command_status
if [ "$final_status" -eq 0 ] && [ "$artifact_status" -ne 0 ]; then final_status=1; fi
printf '%s\n' "$command_status" >"$evidence/logs/repository-oracle-check.command.exit"
printf '%s\n' "$artifact_status" >"$evidence/logs/repository-oracle-check.artifact.exit"
printf '%s\n' "$final_status" >"$evidence/logs/repository-oracle-check.exit"
exit "$final_status"
    ;;
  bundle-selected-qualification-evidence)
set -uo pipefail
evidence="$RUNNER_TEMP/source-qualification-artifacts"
archive="$evidence/source-qualification-preserved.tar.gz"
final_status=0
for name in cargo-prefetch archive-guard-build validator-tool-install validator-tool-lock-admission cli-build external-capture external-check rust-verifier-retry \
  external-shell-harness external-shell-harness-keep external-shell-harness-json-proof \
  repository-oracle-check; do
  status_file="$evidence/logs/$name.exit"
  if [ ! -f "$status_file" ] || [ -L "$status_file" ] \
    || ! grep -Eq '^0$' "$status_file"; then
    final_status=1
  fi
done
    if [ ! -f "$evidence/logs/root-preview.exit" ] || [ -L "$evidence/logs/root-preview.exit" ] \
  || ! grep -Fxq 'generate_statuses=0,0 identity_status=0 diff_statuses=0,0' \
    "$evidence/logs/root-preview.exit"; then
  final_status=1
fi
paths=(logs previews)
qualification_dir=native-validator-tool-qualification
if [ -e "$evidence/$qualification_dir" ] || [ -L "$evidence/$qualification_dir" ]; then
  if [ -L "$evidence/$qualification_dir" ] || [ ! -d "$evidence/$qualification_dir" ] \
    || [ -L "$evidence/$qualification_dir/status.txt" ] \
    || [ ! -f "$evidence/$qualification_dir/status.txt" ] \
    || ! grep -Fxq 'result=success' "$evidence/$qualification_dir/status.txt"; then
    final_status=1
  fi
  paths+=("$qualification_dir")
fi
if ! printf 'qualification_checks_status=%s\n' "$final_status" \
  >"$evidence/qualification-final-status.txt"; then final_status=1; fi
for path in external-oracle captured-stage captured-raw external-check-stage external-check-raw external-check-diff \
  external-shell-harness-logs external-shell-harness-keep-logs repository-oracle-diff repository-oracle-stage repository-oracle-raw \
  external-shell-harness-json-proof external-shell-harness-keep-json-proof \
  external-shell-harness-json-proof.receipt.txt external-shell-harness-keep-json-proof.receipt.txt \
  rust-verifier-retry.json \
  archive-guard-identity.txt qualification-final-status.txt \
  checkout-status.txt source-identity.txt source-inputs.sha256 build-environment.txt \
  tool-pins.json validator-tool-config-identity.before.txt validator-tool-identity.txt \
  validator-tool-install-plan.json validator-tool-install-specs.txt validator-tool-lock-admission.txt \
  cargo-fetch-lock-identity.txt cargo-fetch-locks.before.txt cargo-fetch-locks.after.txt \
  mise-version.txt rustc-version.txt \
  mbx-version.txt jactionlint-version.txt shellcheck-version.txt bun-version.txt cli-identity.txt; do
  if [ -e "$evidence/$path" ] || [ -L "$evidence/$path" ]; then
    paths+=("$path")
  fi
done
set +e
tar -czf "$archive" -C "$evidence" -- "${paths[@]}" \
  2>"$evidence/source-qualification-preserved.tar.stderr.log"
status=$?
if ! printf '%s\n' "$status" >"$evidence/logs/evidence-bundle.exit"; then final_status=1; fi
if ! cat "$evidence/source-qualification-preserved.tar.stderr.log"; then final_status=1; fi
if [ "$status" -ne 0 ]; then final_status=1; fi
exit "$final_status"
    ;;
  *)
    printf 'unknown source qualification phase: %s\n' "$1" >&2
    exit 64
    ;;
esac
