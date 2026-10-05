#!/usr/bin/env -S -i PATH=/usr/bin:/bin /usr/bin/bash --noprofile --norc
# shellcheck shell=bash
# External launch: env -i PATH=/usr/bin:/bin bash --noprofile --norc /root/.velnor-generator-preview-20261005/preseed-preview-cargo-fetch-v4.sh
# Fetch exact lockfile packages and qualify offline Cargo metadata in the private task rootfs.
set -Eeuo pipefail
umask 077

readonly TASK_DIR=/root/.velnor-generator-preview-20261005
readonly ROOTFS="$TASK_DIR/rootfs"
readonly SOURCE_DIR="$TASK_DIR/cargo-fetch-bc7-source-attempt-2"
readonly OBJECTS_DIR=/root/Projects/tailrocks/velnor-project/velnor-new/.git/objects
readonly SOURCE_MANIFEST="$TASK_DIR/cargo-fetch-bc7-source-attempt-2-manifest.json"
readonly WORK_DIR="$TASK_DIR/cargo-fetch-bc7-work-attempt-2"
readonly SAFE_GIT_DIR="$TASK_DIR/cargo-fetch-bc7-git-reader-attempt-2"
readonly HOME_HELPER="$TASK_DIR/rustup-home-prepare-v2.py"
readonly HOME_HELPER_SHA256=5876a1cfb5a1c6a43182995af93a31cb8729fa4421e3c5466a1afe8cfb9e3f0f
readonly HOME_HELPER_MOUNT="$WORK_DIR/rustup-home-prepare.py"
readonly LOG_DIR="$TASK_DIR/logs"
readonly LOG_FILE="$LOG_DIR/cargo-fetch-bc7-attempt-2.log"
readonly MANIFEST_FILE="$TASK_DIR/cargo-source-seed-bc7-attempt-2-manifest.json"
readonly BASE_MANIFEST="$TASK_DIR/rootfs-base-manifest.json"
readonly INPUT_DELTA_MANIFEST="$TASK_DIR/rootfs-input-delta-manifest.json"
readonly RUSTUP_DELTA_MANIFEST="$TASK_DIR/rootfs-rustup-delta-v1-manifest.json"
readonly RUSTUP_EXECUTION_RECORD="$TASK_DIR/rootfs-rustup-delta-v1-execution.json"
readonly RUSTUP_STAGE_LOG="$TASK_DIR/rootfs-rustup-delta-v1.log"
readonly RUSTUP_STAGE_SCRIPT="$TASK_DIR/rootfs-rustup-delta-v1.py"
readonly RUSTUP_FIXTURE_SCRIPT="$TASK_DIR/rootfs-rustup-fsize-fixture-v1.py"
readonly RUSTUP_FIXTURE_RESULT="$TASK_DIR/rootfs-rustup-fsize-fixture-v1-l3ajf73b/result.json"
readonly BASE_PLAN="$TASK_DIR/rootfs-stage-plan.json"
readonly INPUT_DELTA_PLAN="$TASK_DIR/rootfs-stage-plan-with-input.json"
readonly TOOL_PROVENANCE="$TASK_DIR/official-tool-asset-provenance.json"
readonly RUSTUP_ASSET_PROVENANCE="$TASK_DIR/assets/rustup-asset-provenance-v1.json"
readonly RUSTUP_ASSET="$TASK_DIR/assets/rustup-init-1.28.2-x86_64-unknown-linux-gnu"
readonly RUSTUP_ASSET_CHECKSUM="$TASK_DIR/assets/rustup-init-1.28.2-x86_64-unknown-linux-gnu.sha256"
readonly INPUT_DELTA_VERIFICATION="$TASK_DIR/rootfs-input-delta-verify-result-v2.json"
readonly EXPECTED_BASE_MANIFEST_SHA256=1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf
readonly EXPECTED_BASE_ROOTFS_TREE_SHA256=1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763
readonly EXPECTED_INPUT_DELTA_MANIFEST_SHA256=dd5425fceb1524fc304e0da46cf6c64a8b63ee643f8ad6e6689cb15c241ef974
readonly EXPECTED_INPUT_VERIFICATION_SHA256=08912633edeadfca709d587fc5b373824da2ee3cbedf0d2b02d6c85c873318f8
readonly EXPECTED_INPUT_ROOTFS_TREE_SHA256=117843b6eb5d4d06f2309fa205cc51e679445c1dc6f230461e4bd82aa7bf0684
readonly EXPECTED_RUSTUP_DELTA_MANIFEST_SHA256=34cac9f1449601a9c2ddbf526bd4b08724cad30677785c5b445007d9736fcf7d
readonly EXPECTED_RUSTUP_EXECUTION_SHA256=124d065d1ac4caf81ec7f4d721b9f3b767136bfeba7ddd3d74579ee07bedf219
readonly EXPECTED_RUSTUP_STAGE_LOG_SHA256=abb59bef5f305bb216389fd103f54dbadc56f911478af28effa83da7d5fbb6cb
readonly EXPECTED_RUSTUP_STAGE_SCRIPT_SHA256=29b080a5fb5a0fd72164ac11ab094feaf303bac4efd6fee29e087cebd47dad2f
readonly EXPECTED_RUSTUP_FIXTURE_SCRIPT_SHA256=199ae9375d0fda7fcb8c7ac9eca1ef4ea67212492676f7b273af29b62cab1628
readonly EXPECTED_RUSTUP_FIXTURE_RESULT_SHA256=c3d8140a8f17753ae620baaf844d22a4149de5797de6b8960553768a33867454
readonly EXPECTED_RUSTUP_ASSET_SHA256=20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c
readonly EXPECTED_RUSTUP_ASSET_CHECKSUM_SHA256=fda213f7e5b0860abe98f2500ccef7cefd7a4c358833388a0a3d9fba43865fd8
readonly EXPECTED_RUSTUP_PROVENANCE_SHA256=92be10601e7c4c8d6129b3c6463860f3eafe3cba928d5b901bae0de9226355bc
readonly EXPECTED_ROOTFS_TREE_SHA256=fa4194fa10eecd66c72580c4c6acde1d02a1a9a800275b15178ae5b4bf6acf06
readonly EXPECTED_BASE_PLAN_SHA256=ebca0ef5f4c716e533cece9637b3d68763cc8265ce4016bf84a85a279416e77f
readonly EXPECTED_INPUT_DELTA_PLAN_SHA256=4c4a18dd85d71aaa9c9fc9352da9553fc2eb7f5c2d10a8365058c19ea04597bc
readonly EXPECTED_TOOL_PROVENANCE_SHA256=67a8ba245858b11276a6496a513584fefec26e26218b0cdfad028a3fac07d137
readonly NAMESPACE_SCRIPT="$TASK_DIR/preseed-preview-cargo-fetch-namespace-v4.sh"
readonly EXPECTED_SOURCE_COMMIT=bc7f784a51233bd29676353392c9fc07c0ad01a7
readonly EXPECTED_SOURCE_TREE=b3939a4d6010f243485fb528f73af74d49b2fb94
readonly EXPECTED_BRANCH_PARENT=ff41745394712604bddf94b6a4e9f1069100789e
readonly EXPECTED_MAIN_PARENT=703d1b84d9f9e0246a52e3989bf3b78a13bbbf05
readonly EXPECTED_LOCK_SHA256=b43ddef9c7a6b7e4c33cb6a8a4dd3659f26a7645a0f0c09782e04b2ee5b898a4
readonly EXPECTED_RESOLV_SHA256=b6874ac2e7b3c8ca61c0fa8a8e67646b79ad6024c93b7864576b44be1b6d0fa9
readonly EXPECTED_MISE_SHA256=2fd6b7eb9889f13e07dc3fdb6271c05815b81e03da86ff9f875de0b5c473afb8
readonly EXPECTED_GIT_SHA256=356db14e102d68a1a37d8a1ac577dfd678d45d46e92f468bef8b7154e7bfdc60
readonly EXPECTED_NAMESPACE_SHA256=1a13cbbf7c0372119dbb878e389f3739b6078d53626963cd30c0c6c5685998b3
readonly REGISTRY_PACKAGE_COUNT=74

fail() {
  printf 'cargo source fetch: %s\n' "$*" >&2
  exit 1
}

close_inherited_fds() {
  local descriptor
  local -a inherited_descriptors remaining_descriptors
  mapfile -t inherited_descriptors < <(/usr/bin/find "/proc/$$/fd" -mindepth 1 -maxdepth 1 -printf '%f\n')
  for descriptor in "${inherited_descriptors[@]}"; do
    case "$descriptor" in
      0|1|2) continue ;;
      ''|*[!0-9]*) fail 'cannot identify an inherited file descriptor' ;;
    esac
    if [[ "/proc/$$/fd/$descriptor" -ef $0 ]]; then
      continue
    fi
    exec {descriptor}>&- || fail 'cannot close an inherited file descriptor'
  done
  mapfile -t remaining_descriptors < <(/usr/bin/find "/proc/$$/fd" -mindepth 1 -maxdepth 1 -printf '%f\n')
  for descriptor in "${remaining_descriptors[@]}"; do
    case "$descriptor" in
      0|1|2) ;;
      ''|*[!0-9]*) fail 'cannot verify inherited file descriptors' ;;
      *) [[ ! -e "/proc/$$/fd/$descriptor" || "/proc/$$/fd/$descriptor" -ef $0 ]] || fail 'an inherited file descriptor remains open' ;;
    esac
  done
}

verify_host_tools() {
  local path expected actual
  while read -r path expected; do
    [[ -f $path && -x $path ]] || fail "required host tool is unavailable: $path"
    actual=$(/usr/bin/sha256sum "$path")
    actual=${actual%% *}
    [[ $actual == "$expected" ]] || fail "host tool digest mismatch: $path"
  done <<'TOOLS'
/usr/bin/awk eff9f2b711eb5bee1b58d4d33b891e8da8f6534655d3854a45bd10217dbb28e6
/usr/bin/bash 223fe8564b60636bc738b6178d3ba9a50ef7d791266b0efae6363bb716e4c47f
/usr/bin/cut f7f5511ede2187fbb7646581470c84647d087fb87cb74836c0237b0943d3dbc2
/usr/bin/env eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e
/usr/bin/find 91b75e72c495e68dd6aab198c2fd6c458fd51a2dc361045ee31f8e657417de26
/usr/bin/findmnt 598a23e36910f5cf7a99dfe83b6b29b54dae26ec73faa82fb097c0d5ddc36334
/usr/bin/hostname a9f8d0b870f2184ac9f51fc21d7951a8543349cffdc8cd6904c23d2485682220
/usr/bin/id 50629fa0ab5c003b538d4ccf7de34e85d2df4e799a79eb2dd4c2f7b1e0d7ca59
/usr/bin/mount 2f36342bd2fef42343c4fa4a2a396a7f56c7c8f4ddcbd31c43205c62e152e6d4
/usr/bin/pgrep 23bb9ae1e4b5ab7e11558429d1ba2bba77b757da52cfc780382c668342b500b3
/usr/bin/readlink 1dcaf494f4dec175292b1684b3e978d18a4c3c703c52f81606d9e8eea30eb6db
/usr/bin/python3 889c603f0d17cb54060951bcf4c4f9b8c9ebd9e52b392c70209bbb9755d797d9
/usr/bin/sha256sum 89f8c1d1ba3c76138f3771e1a91e2796ade6180b1c1e4258c04698ff32787c97
/usr/bin/stat 128754b37ab743a539d889a91441c440f9b52df77f4e008b345ff221ce1daeaa
/usr/bin/timeout 6ca1891dfc0b05d7680770c2884c0391b92467c7bb6500a5c84677e6481739f1
/usr/bin/unshare e7d3961bd874d5a4fe531efe6feb61b358b8000bd8c5fe91c5f1bdd9af81a002
/usr/sbin/chroot 019aa72e5412a6b0efdd6d94135acbc15ffbd37f7a5fb5ac47a9ef1c515c6c3e
TOOLS
}

[[ ${PATH:-} == /usr/bin:/bin ]] || fail 'outer Bash did not start with the sanitized environment'
verify_host_tools
close_inherited_fds

[[ $# == 0 ]] || fail 'takes no arguments'
[[ $(/usr/bin/id -u) == 0 ]] || fail 'requires effective uid 0'
[[ $(/usr/bin/hostname -s) == bastion ]] || fail 'host is not bastion'
[[ -d $TASK_DIR && ! -L $TASK_DIR ]] || fail 'task directory is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$TASK_DIR") == 0:0:700 ]] || fail 'task directory identity changed'
[[ ! -e $SOURCE_DIR && ! -L $SOURCE_DIR ]] || fail 'source snapshot already exists'
[[ ! -e $SOURCE_MANIFEST && ! -L $SOURCE_MANIFEST ]] || fail 'source manifest already exists'
[[ -d $OBJECTS_DIR && ! -L $OBJECTS_DIR ]] || fail 'trusted Git object directory is unavailable'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$OBJECTS_DIR") == 0:0:755 ]] || fail 'trusted Git object directory identity changed'
[[ ! -e $OBJECTS_DIR/info/alternates && ! -L $OBJECTS_DIR/info/alternates ]] || fail 'Git object alternates are not permitted'
[[ -f $HOME_HELPER && ! -L $HOME_HELPER ]] || fail 'managed-home helper is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a:%s' "$HOME_HELPER") == 0:0:444:15680 ]] || fail 'managed-home helper identity changed'
[[ $(/usr/bin/sha256sum "$HOME_HELPER" | /usr/bin/cut -d' ' -f1) == "$HOME_HELPER_SHA256" ]] || fail 'managed-home helper digest mismatch'
[[ -d $ROOTFS && ! -L $ROOTFS ]] || fail 'reviewed rootfs is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$ROOTFS") == 0:0:755 ]] || fail 'rootfs identity changed'
[[ -f $BASE_MANIFEST && ! -L $BASE_MANIFEST ]] || fail 'base rootfs manifest is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$BASE_MANIFEST") == 0:0:600 ]] || fail 'base rootfs manifest identity changed'
[[ $(/usr/bin/sha256sum "$BASE_MANIFEST" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_BASE_MANIFEST_SHA256" ]] || fail 'base rootfs manifest digest mismatch'
[[ -f $INPUT_DELTA_MANIFEST && ! -L $INPUT_DELTA_MANIFEST ]] || fail 'input mountpoint delta manifest is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$INPUT_DELTA_MANIFEST") == 0:0:600 ]] || fail 'input mountpoint delta manifest identity changed'
[[ $(/usr/bin/sha256sum "$INPUT_DELTA_MANIFEST" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_INPUT_DELTA_MANIFEST_SHA256" ]] || fail 'input mountpoint delta manifest digest mismatch'
[[ -f $INPUT_DELTA_VERIFICATION && ! -L $INPUT_DELTA_VERIFICATION ]] || fail 'input delta verification record is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$INPUT_DELTA_VERIFICATION") == 0:0:600 ]] || fail 'input delta verification record identity changed'
[[ $(/usr/bin/sha256sum "$INPUT_DELTA_VERIFICATION" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_INPUT_VERIFICATION_SHA256" ]] || fail 'input delta verification record digest mismatch'
for evidence_file in "$BASE_PLAN" "$INPUT_DELTA_PLAN" "$TOOL_PROVENANCE"; do
  [[ -f $evidence_file && ! -L $evidence_file ]] || fail "rootfs stage evidence is missing or symlinked: $evidence_file"
  [[ $(/usr/bin/stat -c '%u:%g:%a' "$evidence_file") == 0:0:600 ]] || fail "rootfs stage evidence identity changed: $evidence_file"
done
[[ $(/usr/bin/sha256sum "$BASE_PLAN" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_BASE_PLAN_SHA256" ]] || fail 'base stage plan digest mismatch'
[[ $(/usr/bin/sha256sum "$INPUT_DELTA_PLAN" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_INPUT_DELTA_PLAN_SHA256" ]] || fail 'input delta plan digest mismatch'
[[ $(/usr/bin/sha256sum "$TOOL_PROVENANCE" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_TOOL_PROVENANCE_SHA256" ]] || fail 'tool provenance digest mismatch'
[[ -f $RUSTUP_DELTA_MANIFEST && ! -L $RUSTUP_DELTA_MANIFEST ]] || fail 'Rustup delta manifest is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$RUSTUP_DELTA_MANIFEST") == 0:0:600 ]] || fail 'Rustup delta manifest identity changed'
[[ $(/usr/bin/sha256sum "$RUSTUP_DELTA_MANIFEST" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_DELTA_MANIFEST_SHA256" ]] || fail 'Rustup delta manifest digest mismatch'
[[ -f $RUSTUP_EXECUTION_RECORD && ! -L $RUSTUP_EXECUTION_RECORD ]] || fail 'Rustup execution record is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$RUSTUP_EXECUTION_RECORD") == 0:0:644 ]] || fail 'Rustup execution record identity changed'
[[ $(/usr/bin/sha256sum "$RUSTUP_EXECUTION_RECORD" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_EXECUTION_SHA256" ]] || fail 'Rustup execution record digest mismatch'
[[ -f $RUSTUP_STAGE_LOG && ! -L $RUSTUP_STAGE_LOG ]] || fail 'Rustup staging log is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$RUSTUP_STAGE_LOG") == 0:0:600 ]] || fail 'Rustup staging log identity changed'
[[ $(/usr/bin/sha256sum "$RUSTUP_STAGE_LOG" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_STAGE_LOG_SHA256" ]] || fail 'Rustup staging log digest mismatch'
for evidence_file in "$RUSTUP_STAGE_SCRIPT" "$RUSTUP_FIXTURE_SCRIPT"; do
  [[ -f $evidence_file && ! -L $evidence_file ]] || fail "Rustup staging source is missing or symlinked: $evidence_file"
  [[ $(/usr/bin/stat -c '%u:%g:%a' "$evidence_file") == 0:0:644 ]] || fail "Rustup staging source identity changed: $evidence_file"
done
[[ -f $RUSTUP_FIXTURE_RESULT && ! -L $RUSTUP_FIXTURE_RESULT ]] || fail 'Rustup copy fixture result is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$RUSTUP_FIXTURE_RESULT") == 0:0:600 ]] || fail 'Rustup copy fixture result identity changed'
[[ $(/usr/bin/sha256sum "$RUSTUP_STAGE_SCRIPT" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_STAGE_SCRIPT_SHA256" ]] || fail 'Rustup staging source digest mismatch'
[[ $(/usr/bin/sha256sum "$RUSTUP_FIXTURE_SCRIPT" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_FIXTURE_SCRIPT_SHA256" ]] || fail 'Rustup copy fixture source digest mismatch'
[[ $(/usr/bin/sha256sum "$RUSTUP_FIXTURE_RESULT" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_FIXTURE_RESULT_SHA256" ]] || fail 'Rustup copy fixture result digest mismatch'
[[ -f $RUSTUP_ASSET_PROVENANCE && ! -L $RUSTUP_ASSET_PROVENANCE ]] || fail 'Rustup asset provenance is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$RUSTUP_ASSET_PROVENANCE") == 0:0:600 ]] || fail 'Rustup asset provenance identity changed'
[[ $(/usr/bin/sha256sum "$RUSTUP_ASSET_PROVENANCE" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_PROVENANCE_SHA256" ]] || fail 'Rustup asset provenance digest mismatch'
[[ -f $RUSTUP_ASSET && ! -L $RUSTUP_ASSET ]] || fail 'official Rustup asset is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a:%s' "$RUSTUP_ASSET") == 0:0:600:20889000 ]] || fail 'official Rustup asset identity changed'
[[ $(/usr/bin/sha256sum "$RUSTUP_ASSET" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_ASSET_SHA256" ]] || fail 'official Rustup asset digest mismatch'
[[ -f $RUSTUP_ASSET_CHECKSUM && ! -L $RUSTUP_ASSET_CHECKSUM ]] || fail 'official Rustup checksum sidecar is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$RUSTUP_ASSET_CHECKSUM") == 0:0:600 ]] || fail 'official Rustup checksum sidecar identity changed'
[[ $(/usr/bin/sha256sum "$RUSTUP_ASSET_CHECKSUM" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_ASSET_CHECKSUM_SHA256" ]] || fail 'official Rustup checksum sidecar digest mismatch'
[[ -x $ROOTFS/tools/mise/bin/mise && ! -L $ROOTFS/tools/mise/bin/mise ]] || fail 'pinned Mise binary is unavailable'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/mise/bin/mise" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_MISE_SHA256" ]] || fail 'pinned Mise binary digest mismatch'
[[ -x $ROOTFS/tools/bin/git && ! -L $ROOTFS/tools/bin/git ]] || fail 'pinned Git binary is unavailable'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/bin/git" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_GIT_SHA256" ]] || fail 'pinned Git binary digest mismatch'
[[ -f $ROOTFS/tools/rustup-proxy/rustup && ! -L $ROOTFS/tools/rustup-proxy/rustup ]] || fail 'official Rustup proxy binary is unavailable'
[[ $(/usr/bin/stat -c '%u:%g:%a:%s' "$ROOTFS/tools/rustup-proxy/rustup") == 0:0:555:20889000 ]] || fail 'official Rustup proxy binary identity changed'
[[ $(/usr/bin/sha256sum "$ROOTFS/tools/rustup-proxy/rustup" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RUSTUP_ASSET_SHA256" ]] || fail 'official Rustup proxy binary digest mismatch'
for rustup_alias in cargo cargo-clippy cargo-fmt clippy-driver rustc rustdoc rustfmt; do
  [[ -L $ROOTFS/tools/rustup-proxy/$rustup_alias && $(/usr/bin/readlink "$ROOTFS/tools/rustup-proxy/$rustup_alias") == rustup ]] || fail "Rustup proxy alias differs: $rustup_alias"
done
rust_toolchain_link="$ROOTFS/tools/mise-data/installs/rust/1.98.1"
[[ -L $rust_toolchain_link && $(/usr/bin/readlink "$rust_toolchain_link") == /tools/rust-1.98.1 ]] || fail 'pinned Mise Rust selector link differs from the reviewed toolchain'
[[ -x $ROOTFS/tools/rust-1.98.1/bin/cargo && -x $ROOTFS/tools/rust-1.98.1/bin/rustc ]] || fail 'pinned Rust toolchain binaries are unavailable'
[[ -d $ROOTFS/input && -d $ROOTFS/work && -d $ROOTFS/proc ]] || fail 'rootfs mount points are missing'
[[ -f $ROOTFS/etc/resolv.conf && ! -L $ROOTFS/etc/resolv.conf ]] || fail 'rootfs resolver input is unsafe'
[[ -f /etc/resolv.conf && ! -L /etc/resolv.conf ]] || fail 'host resolver is not a regular file'
[[ $(/usr/bin/stat -c '%u:%g:%a:%s' /etc/resolv.conf) == 0:0:644:167 ]] || fail 'host resolver identity changed'
[[ $(/usr/bin/sha256sum /etc/resolv.conf | /usr/bin/cut -d' ' -f1) == "$EXPECTED_RESOLV_SHA256" ]] || fail 'host resolver digest mismatch'
[[ -f $NAMESPACE_SCRIPT && ! -L $NAMESPACE_SCRIPT ]] || fail 'namespace script is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$NAMESPACE_SCRIPT") == 0:0:700 ]] || fail 'namespace script identity changed'
[[ $EXPECTED_NAMESPACE_SHA256 =~ ^[0-9a-f]{64}$ ]] || fail 'namespace script hash is not sealed'
[[ $(/usr/bin/sha256sum "$NAMESPACE_SCRIPT" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_NAMESPACE_SHA256" ]] || fail 'namespace script digest mismatch'
[[ -d $LOG_DIR && ! -L $LOG_DIR ]] || fail 'private log directory is missing or symlinked'
[[ $(/usr/bin/stat -c '%u:%g:%a' "$LOG_DIR") == 0:0:700 ]] || fail 'private log directory identity changed'
[[ ! -e $SAFE_GIT_DIR && ! -L $SAFE_GIT_DIR ]] || fail 'private Git-reader directory already exists'
[[ ! -e $WORK_DIR && ! -L $WORK_DIR ]] || fail 'cargo work directory already exists'
[[ ! -e $LOG_FILE && ! -L $LOG_FILE ]] || fail 'cargo log already exists'
[[ ! -e $MANIFEST_FILE && ! -L $MANIFEST_FILE ]] || fail 'cargo seed manifest already exists'

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - \
  "$BASE_MANIFEST" "$INPUT_DELTA_MANIFEST" "$RUSTUP_DELTA_MANIFEST" \
  "$INPUT_DELTA_VERIFICATION" "$RUSTUP_EXECUTION_RECORD" "$RUSTUP_STAGE_LOG" \
  "$RUSTUP_ASSET_PROVENANCE" "$RUSTUP_ASSET" "$RUSTUP_ASSET_CHECKSUM" "$ROOTFS" \
  "$EXPECTED_BASE_MANIFEST_SHA256" "$EXPECTED_INPUT_DELTA_MANIFEST_SHA256" \
  "$EXPECTED_RUSTUP_DELTA_MANIFEST_SHA256" "$EXPECTED_INPUT_VERIFICATION_SHA256" \
  "$EXPECTED_RUSTUP_EXECUTION_SHA256" "$EXPECTED_RUSTUP_STAGE_LOG_SHA256" \
  "$EXPECTED_RUSTUP_PROVENANCE_SHA256" "$EXPECTED_RUSTUP_ASSET_SHA256" \
  "$EXPECTED_RUSTUP_ASSET_CHECKSUM_SHA256" "$EXPECTED_BASE_ROOTFS_TREE_SHA256" \
  "$EXPECTED_INPUT_ROOTFS_TREE_SHA256" "$EXPECTED_ROOTFS_TREE_SHA256" \
  "$EXPECTED_BASE_PLAN_SHA256" "$EXPECTED_INPUT_DELTA_PLAN_SHA256" \
  "$EXPECTED_TOOL_PROVENANCE_SHA256" "$EXPECTED_RUSTUP_STAGE_SCRIPT_SHA256" <<'PY'
import hashlib
import json
import os
import posixpath
import stat
import sys

(
    base_manifest_path,
    input_delta_path,
    rustup_delta_path,
    input_verification_path,
    rustup_execution_path,
    rustup_log_path,
    rustup_provenance_path,
    rustup_asset_path,
    rustup_checksum_path,
    rootfs,
    base_manifest_sha,
    input_delta_sha,
    rustup_delta_sha,
    input_verification_sha,
    rustup_execution_sha,
    rustup_log_sha,
    rustup_provenance_sha,
    rustup_asset_sha,
    rustup_checksum_sha,
    base_tree_sha,
    input_tree_sha,
    result_tree_sha,
    base_plan_sha,
    input_plan_sha,
    provenance_sha,
    rustup_script_sha,
) = sys.argv[1:]

def fail(message):
    raise SystemExit("cargo source fetch: " + message)

st = os.lstat(base_manifest_path)
if not stat.S_ISREG(st.st_mode) or st.st_uid != 0 or st.st_gid != 0 or st.st_nlink != 1 or stat.S_IMODE(st.st_mode) != 0o600:
    fail("base rootfs manifest is not a safe private regular file")
data = open(base_manifest_path, "rb").read()
if hashlib.sha256(data).hexdigest() != base_manifest_sha:
    fail("base rootfs manifest digest mismatch")
manifest = json.loads(data)
if manifest != {
    "base_rootfs_tree_sha256": base_tree_sha,
    "materializer_sha256": "4aba299ea3cb925bae7413bd6b8b45845d1ce83a1f617d964b9068dbf87da0a3",
    "plan_sha256": base_plan_sha,
    "schema": 1,
    "status": "BASE_MATERIALIZED; cargo seed and final stage manifest are still absent",
    "tool_provenance_sha256": provenance_sha,
}:
    fail("base rootfs manifest fields differ from the reviewed stage")

delta_stat = os.lstat(input_delta_path)
if not stat.S_ISREG(delta_stat.st_mode) or delta_stat.st_uid != 0 or delta_stat.st_gid != 0 or delta_stat.st_nlink != 1 or stat.S_IMODE(delta_stat.st_mode) != 0o600:
    fail("input mountpoint delta manifest is not a safe private regular file")
delta_bytes = open(input_delta_path, "rb").read()
if hashlib.sha256(delta_bytes).hexdigest() != input_delta_sha:
    fail("input mountpoint delta manifest digest mismatch")
delta_manifest = json.loads(delta_bytes)
if delta_manifest != {
    "added_path": "/input",
    "added_record": {"gid": 0, "mode": 493, "path": "/input", "type": "directory", "uid": 0},
    "base_manifest_sha256": base_manifest_sha,
    "base_rootfs_tree_sha256": base_tree_sha,
    "plan_sha256": input_plan_sha,
    "result_rootfs_tree_sha256": input_tree_sha,
    "reversal": "Remove only empty rootfs/input after verifying this delta manifest and directory identity.",
    "schema": 1,
    "status": "BASE_ROOTFS_PLUS_EMPTY_INPUT_MOUNTPOINT; cargo seed absent",
    "tool_provenance_sha256": provenance_sha,
}:
    fail("input mountpoint delta manifest fields differ from the reviewed stage")

def verify_private_record(path, digest, mode, label):
    item = os.lstat(path)
    if not stat.S_ISREG(item.st_mode) or item.st_uid != 0 or item.st_gid != 0 or item.st_nlink != 1 or stat.S_IMODE(item.st_mode) != mode:
        fail(label + " is not a safe root-owned regular file")
    data = open(path, "rb").read()
    if hashlib.sha256(data).hexdigest() != digest:
        fail(label + " digest mismatch")
    return data

input_verification = json.loads(verify_private_record(input_verification_path, input_verification_sha, 0o600, "input delta verification record"))
if input_verification.get("base_manifest_sha256") != base_manifest_sha or input_verification.get("base_tree_sha256") != base_tree_sha or input_verification.get("delta_manifest_sha256") != input_delta_sha or input_verification.get("result_tree_sha256") != input_tree_sha:
    fail("input delta verification record does not bind its exact parent and result")
if input_verification.get("checks") != {
    "independent_delta_hasher_matches_result": True,
    "input_is_empty_root_owned_mode0755_directory": True,
    "reviewed_base_hasher_matches_result": True,
}:
    fail("input delta verification record lacks the reviewed checks")
rustup_delta_bytes = verify_private_record(rustup_delta_path, rustup_delta_sha, 0o600, "Rustup delta manifest")
rustup_delta = json.loads(rustup_delta_bytes)
expected_added_paths = [
    {"gid": 0, "mode": 493, "path": "/tools/rustup-proxy", "type": "directory", "uid": 0},
    {"gid": 0, "mode": 511, "path": "/tools/rustup-proxy/cargo", "target": "rustup", "type": "symlink", "uid": 0},
    {"gid": 0, "mode": 511, "path": "/tools/rustup-proxy/cargo-clippy", "target": "rustup", "type": "symlink", "uid": 0},
    {"gid": 0, "mode": 511, "path": "/tools/rustup-proxy/cargo-fmt", "target": "rustup", "type": "symlink", "uid": 0},
    {"gid": 0, "mode": 511, "path": "/tools/rustup-proxy/clippy-driver", "target": "rustup", "type": "symlink", "uid": 0},
    {"gid": 0, "mode": 511, "path": "/tools/rustup-proxy/rustc", "target": "rustup", "type": "symlink", "uid": 0},
    {"gid": 0, "mode": 511, "path": "/tools/rustup-proxy/rustdoc", "target": "rustup", "type": "symlink", "uid": 0},
    {"gid": 0, "mode": 511, "path": "/tools/rustup-proxy/rustfmt", "target": "rustup", "type": "symlink", "uid": 0},
    {"gid": 0, "mode": 365, "path": "/tools/rustup-proxy/rustup", "sha256": rustup_asset_sha, "size": 20889000, "type": "file", "uid": 0},
]
if rustup_delta.get("schema") != 1 or rustup_delta.get("status") != "RUSTUP_1_28_2_OFFICIAL_ASSET_STAGED; NOT_EXECUTED; cargo seed and full rootfs seal pending":
    fail("Rustup delta is not the reviewed unexecuted staging result")
if rustup_delta.get("added_paths") != expected_added_paths:
    fail("Rustup delta path inventory differs from the reviewed proxy contract")
if rustup_delta.get("asset_sha256") != rustup_asset_sha or rustup_delta.get("asset_size_bytes") != 20889000 or rustup_delta.get("asset_target") != "x86_64-unknown-linux-gnu" or rustup_delta.get("asset_version") != "1.28.2":
    fail("Rustup delta asset identity differs from the official pinned asset")
if rustup_delta.get("asset_provenance_sha256") != rustup_provenance_sha or rustup_delta.get("parent_input_delta_manifest_sha256") != input_delta_sha:
    fail("Rustup delta provenance or parent manifest binding differs")
if rustup_delta.get("parent_input_verification_result_sha256") != input_verification_sha or rustup_delta.get("parent_rootfs_tree_sha256") != input_tree_sha or rustup_delta.get("result_rootfs_tree_sha256") != result_tree_sha:
    fail("Rustup delta parent or result tree binding differs")
if rustup_delta.get("asset_role") != "rustup-init official binary staged as rustup proxy; not executed by root" or rustup_delta.get("reversal") != "Remove only the exact seven symlinks, rustup binary, and empty /tools/rustup-proxy directory after verifying this manifest and the staged hashes.":
    fail("Rustup delta role or reversal contract differs")
if rustup_delta.get("staging_runtime") != {
    "env_sha256": "eb1c64bbc9908afcdf8cb48524f17112d8dd76a249f80dee2f01da14747b3b6e",
    "python_realpath": "/usr/bin/python3.13",
    "python_sha256": "889c603f0d17cb54060951bcf4f4f9b8c9ebd9e52b392c70209bbb9755d797d9",
    "python_version": "3.13.5",
}:
    fail("Rustup delta staging runtime differs from the reviewed tool provenance")
if rustup_delta.get("execution") != "No Rustup, installer, Cargo, Mise, compiler, Git, network, container, or account command ran during staging.":
    fail("Rustup delta execution disclosure differs")

execution_bytes = verify_private_record(rustup_execution_path, rustup_execution_sha, 0o644, "Rustup execution record")
execution = json.loads(execution_bytes)
stage = execution.get("stage", {})
copy_fixture = execution.get("copy_fixture", {})
if stage.get("exit_code") != 0 or stage.get("status") != "PASS; asset copied and tree sealed; Rustup was not executed" or stage.get("manifest_sha256") != rustup_delta_sha or stage.get("result_rootfs_tree_sha256") != result_tree_sha or stage.get("stage_script_sha256") != rustup_script_sha:
    fail("Rustup execution record does not prove the reviewed staging result")
if stage.get("log_sha256") != rustup_log_sha or copy_fixture.get("exit_code") != 0 or copy_fixture.get("asset_sha256") != rustup_asset_sha or copy_fixture.get("asset_size_bytes") != 20889000:
    fail("Rustup execution record lacks the exact log or copy fixture proof")
verify_private_record(rustup_log_path, rustup_log_sha, 0o600, "Rustup staging log")

provenance_bytes = verify_private_record(rustup_provenance_path, rustup_provenance_sha, 0o600, "Rustup asset provenance")
provenance = json.loads(provenance_bytes)
if provenance.get("project") != "rust-lang/rustup" or provenance.get("version") != "1.28.2" or provenance.get("target") != "x86_64-unknown-linux-gnu" or provenance.get("official_sha256") != rustup_asset_sha or provenance.get("asset_size_bytes") != 20889000:
    fail("Rustup official asset provenance does not match the staged tool")
if provenance.get("asset_url") != "https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init" or provenance.get("checksum_url") != "https://static.rust-lang.org/rustup/archive/1.28.2/x86_64-unknown-linux-gnu/rustup-init.sha256" or provenance.get("checksum_sidecar_sha256") != rustup_checksum_sha:
    fail("Rustup official asset URLs or checksum sidecar provenance differ")
if provenance.get("transport") != {
    "allowed_protocols": "https only",
    "authentication": "none",
    "client": "/usr/bin/curl",
    "environment": "env -i PATH=/usr/bin:/bin LC_ALL=C",
    "http_status": 200,
    "redirects": "disabled",
    "tls_minimum": "1.2",
}:
    fail("Rustup official asset transport evidence differs")
asset_item = os.lstat(rustup_asset_path)
if not stat.S_ISREG(asset_item.st_mode) or (asset_item.st_uid, asset_item.st_gid, stat.S_IMODE(asset_item.st_mode), asset_item.st_size, asset_item.st_nlink) != (0, 0, 0o600, 20889000, 1):
    fail("official Rustup asset has an unexpected identity")
asset_digest = hashlib.sha256()
with open(rustup_asset_path, "rb") as stream:
    for block in iter(lambda: stream.read(1 << 20), b""):
        asset_digest.update(block)
if asset_digest.hexdigest() != rustup_asset_sha:
    fail("official Rustup asset digest differs from its provenance")
checksum_bytes = verify_private_record(rustup_checksum_path, rustup_checksum_sha, 0o600, "Rustup checksum sidecar")
if checksum_bytes != (rustup_asset_sha + " *./rustup-init\n").encode("ascii"):
    fail("official Rustup checksum sidecar content differs")

records = []

def walk(fd, prefix):
    for name in sorted(os.listdir(fd), key=os.fsencode):
        if name in (".", "..") or "/" in name:
            fail("invalid rootfs directory entry")
        path = (prefix + "/" + name) if prefix else "/" + name
        item = os.stat(name, dir_fd=fd, follow_symlinks=False)
        mode = stat.S_IMODE(item.st_mode)
        if item.st_uid != 0 or item.st_gid != 0:
            fail("rootfs owner mismatch")
        if stat.S_ISCHR(item.st_mode) and path == "/dev/null" and os.major(item.st_rdev) == 1 and os.minor(item.st_rdev) == 3:
            if mode != 0o666:
                fail("unexpected null device mode")
            records.append({"path": path, "type": "character-device", "mode": mode, "uid": 0, "gid": 0, "major": 1, "minor": 3})
            continue
        if stat.S_ISLNK(item.st_mode):
            target = os.readlink(name, dir_fd=fd)
            normalized = posixpath.normpath(target if target.startswith("/") else posixpath.join(posixpath.dirname(path), target))
            if not normalized.startswith("/") or normalized == "/.." or normalized.startswith("/../"):
                fail("rootfs symlink escape")
            records.append({"path": path, "type": "symlink", "mode": mode, "uid": 0, "gid": 0, "target": target})
            continue
        if mode & 0o022:
            fail("rootfs writable-by-group-or-world path")
        if stat.S_ISDIR(item.st_mode):
            child = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            try:
                opened = os.fstat(child)
                if not stat.S_ISDIR(opened.st_mode) or (opened.st_dev, opened.st_ino) != (item.st_dev, item.st_ino):
                    fail("rootfs directory changed during hashing")
                records.append({"path": path, "type": "directory", "mode": mode, "uid": 0, "gid": 0})
                walk(child, path)
            finally:
                os.close(child)
        elif stat.S_ISREG(item.st_mode):
            if item.st_nlink != 1:
                fail("rootfs hard link rejected")
            child = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=fd)
            try:
                opened = os.fstat(child)
                if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (item.st_dev, item.st_ino):
                    fail("rootfs file changed during hashing")
                digest = hashlib.sha256()
                while True:
                    block = os.read(child, 1 << 20)
                    if not block:
                        break
                    digest.update(block)
                records.append({"path": path, "type": "file", "mode": mode, "uid": 0, "gid": 0, "size": item.st_size, "sha256": digest.hexdigest()})
            finally:
                os.close(child)
        else:
            fail("unsupported rootfs entry")

rootfd = os.open(rootfs, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
rootst = os.fstat(rootfd)
if not stat.S_ISDIR(rootst.st_mode) or rootst.st_uid != 0 or rootst.st_gid != 0 or stat.S_IMODE(rootst.st_mode) != 0o755:
    os.close(rootfd)
    fail("rootfs root mode mismatch")
records.append({"path": "/", "type": "directory", "mode": 0o755, "uid": 0, "gid": 0})
try:
    walk(rootfd, "")
finally:
    os.close(rootfd)
encoded = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
if hashlib.sha256(encoded).hexdigest() != result_tree_sha:
    fail("result rootfs tree digest mismatch")
input_path = os.path.join(rootfs, "input")
input_stat = os.lstat(input_path)
if not stat.S_ISDIR(input_stat.st_mode) or input_stat.st_uid != 0 or input_stat.st_gid != 0 or stat.S_IMODE(input_stat.st_mode) != 0o755 or os.listdir(input_path):
    fail("input mountpoint is not the reviewed empty root-owned directory")
print("reviewed base, input, and Rustup delta chain plus complete result rootfs tree verified")
PY

if /usr/bin/pgrep -u 65534 >/dev/null 2>&1; then fail 'uid 65534 is already active on the host'; fi
if /usr/bin/findmnt -rn -o TARGET | /usr/bin/awk -v p="$TASK_DIR" '$0==p || index($0,p"/")==1 { found=1 } END { exit !found }'; then
  fail 'task directory already has a mounted resource'
fi

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$WORK_DIR" "$LOG_FILE" "$HOME_HELPER_MOUNT" <<'PY'
import os
import sys

work, log, helper_mount = sys.argv[1:]
os.mkdir(work, 0o755)
os.chown(work, 65534, 65534)
os.chmod(work, 0o700)
for name in ("home", "tmp", "rustup-home", "mise-data", "mise-cache", "xdg-data", "xdg-cache", "xdg-state", "xdg-config", "cargo-seed", "qualification"):
    path = os.path.join(work, name)
    os.mkdir(path, 0o700)
    os.chown(path, 65534, 65534)
    os.chmod(path, 0o700)
fd = os.open(helper_mount, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o444)
try:
    os.fchown(fd, 0, 0)
    os.fchmod(fd, 0o444)
finally:
    os.close(fd)
fd = os.open(log, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
try:
    os.fchown(fd, 0, 0)
    os.fchmod(fd, 0o600)
finally:
    os.close(fd)
PY

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - \
  "$ROOTFS/tools/bin/git" "$OBJECTS_DIR" "$SAFE_GIT_DIR" "$SOURCE_DIR" "$SOURCE_MANIFEST" \
  "$EXPECTED_SOURCE_COMMIT" "$EXPECTED_SOURCE_TREE" "$EXPECTED_BRANCH_PARENT" "$EXPECTED_MAIN_PARENT" \
  "$EXPECTED_LOCK_SHA256" <<'PY'
import hashlib
import json
import os
import posixpath
import re
import resource
import signal
import stat
import subprocess
import sys

git, objects, git_dir, source, manifest_path, expected_commit, expected_tree, branch_parent, main_parent, lock_sha = sys.argv[1:]

def fail(message):
    raise SystemExit("cargo source snapshot: " + message)

def require_root_private(path, mode):
    item = os.lstat(path)
    if not stat.S_ISDIR(item.st_mode) or item.st_uid != 0 or item.st_gid != 0 or stat.S_IMODE(item.st_mode) != mode:
        fail("unsafe task Git directory")

for path in ("/root", "/root/Projects", "/root/Projects/tailrocks", "/root/Projects/tailrocks/velnor-project", "/root/Projects/tailrocks/velnor-project/velnor-new", "/root/Projects/tailrocks/velnor-project/velnor-new/.git", objects, os.path.join(objects, "info")):
    require_root_private(path, 0o700 if path == "/root" else 0o755)

if os.path.lexists(os.path.join(objects, "info", "alternates")):
    fail("Git object alternates are not permitted")
object_dirs = [objects]
object_count = 0
object_store_bytes = 0
while object_dirs:
    directory = object_dirs.pop()
    with os.scandir(directory) as scan:
        children = list(scan)
    for child in children:
        item = os.lstat(child.path)
        if item.st_uid != 0 or item.st_gid != 0 or stat.S_ISLNK(item.st_mode):
            fail("Git object store contains a non-root or symlink entry")
        if stat.S_ISDIR(item.st_mode):
            if stat.S_IMODE(item.st_mode) != 0o755:
                fail("Git object store directory mode changed")
            object_dirs.append(child.path)
        elif stat.S_ISREG(item.st_mode):
            if item.st_nlink != 1 or stat.S_IMODE(item.st_mode) & 0o022:
                fail("Git object store file mode or link count is unsafe")
            object_store_bytes += item.st_size
            if object_store_bytes > (4 << 30):
                fail("Git object store exceeds the bounded size")
        else:
            fail("Git object store contains an unsupported file type")
        object_count += 1
        if object_count > 200000:
            fail("Git object store entry count exceeds the bounded limit")

if os.path.lexists(git_dir) or os.path.lexists(source) or os.path.lexists(manifest_path):
    fail("snapshot output path already exists")
os.mkdir(git_dir, 0o700)
os.mkdir(os.path.join(git_dir, "objects"), 0o700)
os.mkdir(os.path.join(git_dir, "objects", "info"), 0o700)
os.mkdir(os.path.join(git_dir, "refs"), 0o700)
os.mkdir(os.path.join(git_dir, "refs", "heads"), 0o700)
os.mkdir(os.path.join(git_dir, "refs", "tags"), 0o700)
with open(os.path.join(git_dir, "config"), "xb") as stream:
    stream.write(b"[core]\n\trepositoryformatversion = 0\n\tbare = true\n")
head_path = os.path.join(git_dir, "HEAD")
with open(head_path, "xb") as stream:
    stream.write(expected_commit.encode("ascii") + b"\n")
os.chown(head_path, 0, 0)
os.chmod(head_path, 0o600)
with open(os.path.join(git_dir, "objects", "info", "alternates"), "xb") as stream:
    stream.write(os.fsencode(objects) + b"\n")
for path in (
    git_dir,
    os.path.join(git_dir, "objects"),
    os.path.join(git_dir, "objects", "info"),
    os.path.join(git_dir, "refs"),
    os.path.join(git_dir, "refs", "heads"),
    os.path.join(git_dir, "refs", "tags"),
):
    os.chown(path, 0, 0)
    os.chmod(path, 0o700)
safe_env = {
    "PATH": "/usr/bin:/bin",
    "HOME": "/nonexistent",
    "GIT_DIR": git_dir,
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_TERMINAL_PROMPT": "0",
    "GIT_OPTIONAL_LOCKS": "0",
    "GIT_NO_REPLACE_OBJECTS": "1",
    "GIT_ALLOW_PROTOCOL": "none",
    "LC_ALL": "C",
}
for limit, value in ((resource.RLIMIT_CPU, 90), (resource.RLIMIT_FSIZE, 1 << 29), (resource.RLIMIT_NPROC, 16), (resource.RLIMIT_NOFILE, 64), (resource.RLIMIT_AS, 2 << 30), (resource.RLIMIT_CORE, 0)):
    resource.setrlimit(limit, (value, value))
proc = subprocess.Popen(
    [git, "--no-replace-objects", "cat-file", "--batch"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    env=safe_env,
    close_fds=True,
    start_new_session=True,
)
def stop_git():
    if proc.poll() is None:
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

import atexit
atexit.register(stop_git)
def alarm_handler(_signum, _frame):
    raise TimeoutError("bounded source-object parsing timed out")
signal.signal(signal.SIGALRM, alarm_handler)
signal.alarm(120)
object_count = 0
object_bytes = 0

def read_exact(size):
    chunks = []
    remaining = size
    while remaining:
        chunk = proc.stdout.read(remaining)
        if not chunk:
            fail("Git object stream ended early")
        chunks.append(chunk)
        remaining -= len(chunk)
    return b"".join(chunks)

def read_object(oid):
    global object_count, object_bytes
    if not re.fullmatch(r"[0-9a-f]{40}", oid):
        fail("invalid Git object identifier")
    proc.stdin.write(oid.encode("ascii") + b"\n")
    proc.stdin.flush()
    header = proc.stdout.readline()
    fields = header.rstrip(b"\n").split(b" ")
    if len(fields) != 3 or fields[0].decode("ascii", "strict") != oid:
        fail("Git object is missing or returned an invalid header")
    kind = fields[1].decode("ascii", "strict")
    try:
        size = int(fields[2])
    except ValueError:
        fail("invalid Git object size")
    if size < 0 or size > (1 << 28):
        fail("Git object exceeds the bounded size limit")
    body = read_exact(size)
    if read_exact(1) != b"\n":
        fail("Git object framing is invalid")
    calculated = hashlib.sha1(kind.encode("ascii") + b" " + str(size).encode("ascii") + b"\0" + body).hexdigest()
    if calculated != oid:
        fail("Git object bytes do not match their object identifier")
    object_count += 1
    object_bytes += size
    if object_count > 20000 or object_bytes > (1 << 29):
        fail("source object stream exceeds bounded limits")
    return kind, body

def parse_commit(body):
    header, separator, _message = body.partition(b"\n\n")
    if not separator:
        fail("commit object has no header separator")
    values = {}
    for line in header.splitlines():
        if line.startswith(b" "):
            continue
        key, space, value = line.partition(b" ")
        if not space:
            fail("malformed commit header")
        values.setdefault(key, []).append(value)
    trees = values.get(b"tree", [])
    parents = values.get(b"parent", [])
    if len(trees) != 1 or parents != [branch_parent.encode(), main_parent.encode()]:
        fail("commit tree or parent chain differs from the reviewed PR merge")
    tree = trees[0].decode("ascii", "strict")
    if tree != expected_tree:
        fail("commit tree differs from the reviewed source tree")
    return tree, [branch_parent, main_parent]

def valid_component(raw):
    if not raw or raw in (b".", b"..") or b"/" in raw or b"\\" in raw or len(raw) > 255:
        fail("unsafe Git path component")
    try:
        text = raw.decode("utf-8", "strict")
    except UnicodeDecodeError:
        fail("non-UTF-8 Git path is not supported by the Rust workspace")
    if any(ord(char) < 32 or ord(char) == 127 for char in text) or text == ".git":
        fail("Git path uses a prohibited name")
    return text

def validate_link(path, raw):
    try:
        target = raw.decode("utf-8", "strict")
    except UnicodeDecodeError:
        fail("non-UTF-8 symbolic link")
    if not target or target.startswith("/") or "\\" in target or "\0" in target:
        fail("absolute or malformed symbolic link")
    if any(part == ".." for part in target.split("/")):
        fail("parent traversal in symbolic link is not permitted")
    if posixpath.normpath(posixpath.join(posixpath.dirname(path), target)).startswith("../"):
        fail("symbolic link escapes the source snapshot")
    return target

def parse_tree(body):
    entries = []
    offset = 0
    while offset < len(body):
        space = body.find(b" ", offset)
        nul = body.find(b"\0", space + 1)
        if space < 0 or nul < 0 or nul + 21 > len(body):
            fail("malformed Git tree object")
        mode = body[offset:space]
        raw_name = body[space + 1:nul]
        name = valid_component(raw_name)
        oid = body[nul + 1:nul + 21].hex()
        offset = nul + 21
        entries.append((mode, name, oid))
    return entries

def create_dir(path):
    os.mkdir(path, 0o755)
    os.chown(path, 0, 0)
    os.chmod(path, 0o755)

os.mkdir(source, 0o755)
os.chown(source, 0, 0)
os.chmod(source, 0o755)
commit_kind, commit_body = read_object(expected_commit)
if commit_kind != "commit":
    fail("expected source object is not a commit")
tree, parents = parse_commit(commit_body)
submodules = []
file_count = 0
file_bytes = 0
tree_count = 0

def materialize_tree(tree_oid, rel_dir, depth=0):
    global file_count, file_bytes, tree_count
    if depth > 64:
        fail("source tree nesting exceeds the bounded depth")
    tree_count += 1
    if tree_count > 10000:
        fail("source tree entry count exceeds the bounded limit")
    kind, body = read_object(tree_oid)
    if kind != "tree":
        fail("tree entry does not identify a tree")
    for mode, name, oid in parse_tree(body):
        relative = posixpath.join(rel_dir, name) if rel_dir else name
        if len(relative.encode("utf-8")) > 4096:
            fail("source path exceeds the supported length")
        if name == ".cargo" and relative != ".cargo":
            fail(".cargo is permitted only at the source root")
        if relative == ".cargo" and mode not in (b"40000", b"040000"):
            fail(".cargo must be a directory")
        if relative.startswith(".cargo/") and (relative != ".cargo/mutants.toml" or mode != b"100644"):
            fail("only the regular .cargo/mutants.toml file is permitted")
        path = os.path.join(source, *relative.split("/"))
        if mode in (b"40000", b"040000"):
            create_dir(path)
            materialize_tree(oid, relative, depth + 1)
        elif mode == b"160000":
            if relative != "qualification/fixtures/submodule":
                fail("unexpected submodule path")
            create_dir(path)
            submodules.append({"path": relative, "commit": oid, "materialized": "empty directory; submodule code excluded"})
        elif mode in (b"100644", b"100755", b"120000"):
            blob_kind, data = read_object(oid)
            if blob_kind != "blob":
                fail("file entry does not identify a blob")
            file_count += 1
            file_bytes += len(data)
            if file_count > 10000 or file_bytes > (1 << 28):
                fail("source snapshot exceeds bounded size limits")
            if mode == b"120000":
                target = validate_link(relative, data)
                os.symlink(target, path)
            else:
                fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o755 if mode == b"100755" else 0o644)
                try:
                    os.fchown(fd, 0, 0)
                    os.fchmod(fd, 0o755 if mode == b"100755" else 0o644)
                    view = memoryview(data)
                    while view:
                        written = os.write(fd, view)
                        if written <= 0:
                            fail("short source file write")
                        view = view[written:]
                    os.fsync(fd)
                finally:
                    os.close(fd)
        else:
            fail("unsupported Git tree entry mode")

materialize_tree(tree, "")
lock_path = os.path.join(source, "Cargo.lock")
lock = open(lock_path, "rb").read()
if hashlib.sha256(lock).hexdigest() != lock_sha:
    fail("source Cargo.lock does not match the reviewed lock digest")
cargo_dir = os.lstat(os.path.join(source, ".cargo"))
mutants_path = os.path.join(source, ".cargo", "mutants.toml")
mutants_stat = os.lstat(mutants_path)
if not stat.S_ISDIR(cargo_dir.st_mode) or cargo_dir.st_uid != 0 or cargo_dir.st_gid != 0 or stat.S_IMODE(cargo_dir.st_mode) != 0o755:
    fail(".cargo directory is not the expected safe directory")
if not stat.S_ISREG(mutants_stat.st_mode) or mutants_stat.st_uid != 0 or mutants_stat.st_gid != 0 or mutants_stat.st_nlink != 1 or stat.S_IMODE(mutants_stat.st_mode) != 0o644:
    fail(".cargo/mutants.toml is not the expected regular file")
mutants_digest = hashlib.sha256(open(mutants_path, "rb").read()).hexdigest()

records = []
def walk(directory, relative="."):
    st = os.lstat(directory)
    if not stat.S_ISDIR(st.st_mode) or st.st_uid != 0 or st.st_gid != 0 or stat.S_IMODE(st.st_mode) != 0o755:
        fail("snapshot directory ownership or mode is unsafe")
    records.append({"path": relative, "type": "directory", "mode": 0o755, "uid": 0, "gid": 0})
    with os.scandir(directory) as scan:
        entries = sorted(list(scan), key=lambda entry: os.fsencode(entry.name))
    for entry in entries:
        rel = entry.name if relative == "." else relative + "/" + entry.name
        item = os.lstat(entry.path)
        mode_bits = stat.S_IMODE(item.st_mode)
        if item.st_uid != 0 or item.st_gid != 0 or (not stat.S_ISLNK(item.st_mode) and mode_bits & 0o022):
            fail("snapshot entry ownership or mode is unsafe")
        if stat.S_ISDIR(item.st_mode):
            walk(entry.path, rel)
        elif stat.S_ISREG(item.st_mode):
            if item.st_nlink != 1:
                fail("snapshot hard links are prohibited")
            digest = hashlib.sha256()
            fd = os.open(entry.path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
            try:
                opened = os.fstat(fd)
                if not stat.S_ISREG(opened.st_mode) or (opened.st_dev, opened.st_ino) != (item.st_dev, item.st_ino):
                    fail("source file changed during snapshot hashing")
                while True:
                    block = os.read(fd, 1 << 20)
                    if not block:
                        break
                    digest.update(block)
            finally:
                os.close(fd)
            records.append({"path": rel, "type": "file", "mode": mode_bits, "uid": 0, "gid": 0, "size": item.st_size, "sha256": digest.hexdigest()})
        elif stat.S_ISLNK(item.st_mode):
            records.append({"path": rel, "type": "symlink", "mode": mode_bits, "uid": 0, "gid": 0, "target": os.readlink(entry.path)})
        else:
            fail("unsupported materialized source entry")
walk(source)
records.sort(key=lambda record: os.fsencode(record["path"]))
snapshot_bytes = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
snapshot_sha = hashlib.sha256(snapshot_bytes).hexdigest()
manifest = {
    "schema": 1,
    "status": "EXACT_OBJECT_SNAPSHOT_FOR_LOCKED_CARGO_FETCH_ONLY",
    "repository": "https://github.com/tailrocks/velnor-new",
    "source_commit": expected_commit,
    "source_tree": expected_tree,
    "source_parents": parents,
    "cargo_snapshot_tree_sha256": snapshot_sha,
    "cargo_snapshot_file_count": file_count,
    "cargo_snapshot_file_bytes": file_bytes,
    "lock_path": "Cargo.lock",
    "lock_sha256": lock_sha,
    "cargo_configuration_policy": "Only tracked .cargo/mutants.toml is included; .cargo/config and .cargo/config.toml are prohibited.",
    "cargo_mutants_sha256": mutants_digest,
    "submodules": submodules,
    "generator_input": False,
    "origin_validation": "source bytes came from the fixed verified Git object database; this snapshot has no .git metadata and is not a generator input",
}
encoded = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode()
fd = os.open(manifest_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
try:
    os.fchown(fd, 0, 0)
    os.fchmod(fd, 0o600)
    if os.write(fd, encoded) != len(encoded):
        fail("short source manifest write")
    os.fsync(fd)
finally:
    os.close(fd)
signal.alarm(0)
proc.stdin.close()
stderr = proc.stderr.read()
status = proc.wait(timeout=30)
if status != 0:
    fail("pinned Git cat-file failed: " + stderr.decode("utf-8", "replace")[:512])
if file_count < 1 or not submodules:
    fail("source tree inventory is incomplete")
print(f"exact source objects verified: {expected_commit} {expected_tree}; Cargo snapshot {snapshot_sha}; files {file_count}; bytes {file_bytes}")
PY

source_manifest_sha=$(/usr/bin/sha256sum "$SOURCE_MANIFEST" | /usr/bin/cut -d' ' -f1)

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$SOURCE_DIR/Cargo.lock" "$EXPECTED_LOCK_SHA256" "$REGISTRY_PACKAGE_COUNT" <<'PY'
import hashlib
import re
import sys
import tomllib

path, expected_sha, expected_count = sys.argv[1:]
data = open(path, "rb").read()
if hashlib.sha256(data).hexdigest() != expected_sha:
    raise SystemExit("cargo source fetch: lockfile digest mismatch")
lock = tomllib.loads(data.decode("utf-8"))
packages = lock.get("package")
if not isinstance(packages, list):
    raise SystemExit("cargo source fetch: lock package table is missing")
registry = []
git_sources = []
other_sources = []
for package in packages:
    source = package.get("source")
    if source is None:
        continue
    if source == "registry+https://github.com/rust-lang/crates.io-index":
        name = package.get("name")
        version = package.get("version")
        checksum = package.get("checksum")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name):
            raise SystemExit("cargo source fetch: invalid registry package name")
        if not isinstance(version, str) or not re.fullmatch(r"[A-Za-z0-9.+_-]+", version):
            raise SystemExit("cargo source fetch: invalid registry package version")
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise SystemExit("cargo source fetch: invalid registry package checksum")
        registry.append((name, version, checksum))
    elif source.startswith("git+"):
        git_sources.append(source)
    else:
        other_sources.append(source)
if len(registry) != int(expected_count) or git_sources or other_sources:
    raise SystemExit("cargo source fetch: lock source inventory differs from the reviewed crates.io-only set")
if len({(name, version) for name, version, _ in registry}) != len(registry):
    raise SystemExit("cargo source fetch: duplicate registry package identity")
print(f"lock inventory verified: {len(registry)} crates.io packages; 0 Git packages")
PY

exec </dev/null >>"$LOG_FILE" 2>&1
printf 'source commit: %s\nsource tree: %s\nmain parent: %s\nlock sha256: %s\nsource manifest sha256: %s\n' \
  "$EXPECTED_SOURCE_COMMIT" "$EXPECTED_SOURCE_TREE" "$EXPECTED_MAIN_PARENT" "$EXPECTED_LOCK_SHA256" "$source_manifest_sha"
printf 'cargo fetch command: mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo fetch --locked --manifest-path /input/Cargo.toml\n'
printf 'offline metadata command: CARGO_NET_OFFLINE=true mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo metadata --locked --offline --format-version 1 --manifest-path /input/Cargo.toml\n'

fetch_status=0
/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/timeout --foreground --signal=TERM --kill-after=10s 900s \
  /usr/bin/python3 -I -S - "$NAMESPACE_SCRIPT" "$source_manifest_sha" <<'PY' || fetch_status=$?
import ctypes
import os
import resource
import sys

script = sys.argv[1]
manifest_sha = sys.argv[2]
limits = (
    (resource.RLIMIT_CPU, 600),
    (resource.RLIMIT_FSIZE, 1 << 29),
    (resource.RLIMIT_NPROC, 64),
    (resource.RLIMIT_NOFILE, 64),
    (resource.RLIMIT_AS, 4 << 30),
    (resource.RLIMIT_CORE, 0),
)
for limit, value in limits:
    resource.setrlimit(limit, (value, value))
net_ns = os.readlink("/proc/self/ns/net")
expected = {
    "PATH": "/usr/sbin:/usr/bin:/sbin:/bin",
    "EXPECTED_MNT_NS": os.readlink("/proc/self/ns/mnt"),
    "EXPECTED_PID_NS": os.readlink("/proc/self/ns/pid"),
    "EXPECTED_IPC_NS": os.readlink("/proc/self/ns/ipc"),
    "EXPECTED_UTS_NS": os.readlink("/proc/self/ns/uts"),
    "EXPECTED_NET_NS": net_ns,
    "SOURCE_MANIFEST_SHA256": manifest_sha,
}
null_fd = os.open("/dev/null", os.O_RDONLY | os.O_CLOEXEC)
os.dup2(null_fd, 0)
if null_fd > 2:
    os.close(null_fd)
libc = ctypes.CDLL(None, use_errno=True)
close_range = libc.close_range
close_range.argtypes = [ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
close_range.restype = ctypes.c_int
if close_range(3, 0xFFFFFFFF, 0) != 0:
    error = ctypes.get_errno()
    raise OSError(error, os.strerror(error), "close_range")
argv = [
    "/usr/bin/unshare", "--mount", "--pid", "--ipc", "--uts", "--fork",
    "--kill-child", "--propagation", "private", "--",
    "/usr/bin/bash", "--noprofile", "--norc", script,
]
os.execve(argv[0], argv, expected)
PY

cleanup_status=0
if /usr/bin/pgrep -u 65534 >/dev/null 2>&1; then
  printf 'cargo source fetch: uid 65534 process remains after namespace exit\n' >&2
  cleanup_status=1
fi
if /usr/bin/findmnt -rn -o TARGET | /usr/bin/awk -v p="$TASK_DIR" '$0==p || index($0,p"/")==1 { found=1 } END { exit !found }'; then
  printf 'cargo source fetch: task-owned mount remains after namespace exit\n' >&2
  cleanup_status=1
fi
[[ $cleanup_status == 0 ]] || fail 'namespace cleanup check failed; preserve private logs and work for review'
[[ $fetch_status == 0 ]] || fail "Cargo fetch failed with status $fetch_status; preserve private logs and work for review"

/usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S - "$SOURCE_DIR/Cargo.lock" "$WORK_DIR/cargo-seed" "$WORK_DIR/qualification/cargo-metadata.json" "$MANIFEST_FILE" "$EXPECTED_SOURCE_COMMIT" "$EXPECTED_SOURCE_TREE" "$EXPECTED_MAIN_PARENT" "$EXPECTED_LOCK_SHA256" "$REGISTRY_PACKAGE_COUNT" "$SOURCE_MANIFEST" "$source_manifest_sha" <<'PY'
import hashlib
import json
import os
import posixpath
import re
import stat
import sys
import tomllib

lock_path, cargo_home, metadata_path, output_manifest_path, commit, tree, main_parent, lock_sha, count_text, source_manifest_path, source_manifest_sha = sys.argv[1:]
expected_count = int(count_text)

def fail(message):
    raise SystemExit("cargo source fetch: " + message)

def read_lock():
    st = os.lstat(lock_path)
    if not stat.S_ISREG(st.st_mode) or st.st_uid != 0 or st.st_gid != 0 or st.st_nlink != 1 or stat.S_IMODE(st.st_mode) & 0o022:
        fail("lockfile is not a safe root-owned regular file")
    data = open(lock_path, "rb").read()
    if hashlib.sha256(data).hexdigest() != lock_sha:
        fail("lockfile changed after fetch")
    packages = tomllib.loads(data.decode("utf-8")).get("package")
    if not isinstance(packages, list):
        fail("lock package table is missing")
    registry = {}
    for package in packages:
        source = package.get("source")
        if source is None:
            continue
        if source != "registry+https://github.com/rust-lang/crates.io-index":
            fail("lock contains a non-crates.io source")
        name = package.get("name")
        version = package.get("version")
        checksum = package.get("checksum")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name):
            fail("invalid registry package name")
        if not isinstance(version, str) or not re.fullmatch(r"[A-Za-z0-9.+_-]+", version):
            fail("invalid registry package version")
        if not isinstance(checksum, str) or not re.fullmatch(r"[0-9a-f]{64}", checksum):
            fail("invalid registry package checksum")
        filename = f"{name}-{version}.crate"
        if filename in registry:
            fail("duplicate registry archive filename")
        registry[filename] = {"name": name, "version": version, "sha256": checksum}
    if len(registry) != expected_count:
        fail("locked crates.io package count changed")
    return registry

def walk_tree(root):
    root_st = os.lstat(root)
    if not stat.S_ISDIR(root_st.st_mode) or root_st.st_uid != 65534 or root_st.st_gid != 65534 or stat.S_IMODE(root_st.st_mode) != 0o700:
        fail("Cargo home root identity changed")
    records = [{"path": ".", "type": "dir", "mode": 0o700, "uid": 65534, "gid": 65534}]
    total = 0
    for current, dirs, files in os.walk(root, topdown=True, followlinks=False):
        dirs.sort(key=os.fsencode)
        files.sort(key=os.fsencode)
        for name in list(dirs):
            path = os.path.join(current, name)
            st = os.lstat(path)
            if not stat.S_ISDIR(st.st_mode) or st.st_uid != 65534 or st.st_gid != 65534 or stat.S_IMODE(st.st_mode) & 0o022:
                fail("Cargo home contains an unsafe directory")
            rel = os.path.relpath(path, root)
            records.append({"path": rel, "type": "dir", "mode": stat.S_IMODE(st.st_mode), "uid": st.st_uid, "gid": st.st_gid})
        for name in files:
            path = os.path.join(current, name)
            st = os.lstat(path)
            rel = os.path.relpath(path, root)
            if stat.S_ISLNK(st.st_mode):
                expected = {
                    "bin/cargo": "/tools/rustup-proxy/cargo",
                    "bin/cargo-clippy": "/tools/rustup-proxy/cargo-clippy",
                    "bin/cargo-fmt": "/tools/rustup-proxy/cargo-fmt",
                    "bin/clippy-driver": "/tools/rustup-proxy/clippy-driver",
                    "bin/rustc": "/tools/rustup-proxy/rustc",
                    "bin/rustdoc": "/tools/rustup-proxy/rustdoc",
                    "bin/rustfmt": "/tools/rustup-proxy/rustfmt",
                    "bin/rustup": "/tools/rustup-proxy/rustup",
                }
                target = os.readlink(path)
                if rel not in expected or target != expected[rel] or (st.st_uid, st.st_gid) != (65534, 65534) or st.st_nlink != 1:
                    fail("Cargo home contains an unapproved symbolic link")
                records.append({"path": rel, "type": "symlink", "mode": stat.S_IMODE(st.st_mode), "uid": st.st_uid, "gid": st.st_gid, "target": target})
                continue
            if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1 or st.st_uid != 65534 or st.st_gid != 65534 or stat.S_IMODE(st.st_mode) & 0o022:
                fail("Cargo home contains a non-regular or unsafe file")
            if st.st_size > (1 << 28):
                fail("Cargo home file exceeds the size limit")
            total += st.st_size
            if total > (1 << 29):
                fail("Cargo home exceeds the total size limit")
            digest = hashlib.sha256()
            with open(path, "rb") as stream:
                for block in iter(lambda: stream.read(1 << 20), b""):
                    digest.update(block)
            records.append({"path": rel, "type": "file", "mode": stat.S_IMODE(st.st_mode), "uid": st.st_uid, "gid": st.st_gid, "size": st.st_size, "sha256": digest.hexdigest()})
    records.sort(key=lambda item: os.fsencode(item["path"]))
    encoded = (json.dumps(records, sort_keys=True, separators=(",", ":")) + "\n").encode()
    return hashlib.sha256(encoded).hexdigest(), total

registry = read_lock()
source_manifest_stat = os.lstat(source_manifest_path)
if not stat.S_ISREG(source_manifest_stat.st_mode) or source_manifest_stat.st_uid != 0 or source_manifest_stat.st_gid != 0 or source_manifest_stat.st_nlink != 1 or stat.S_IMODE(source_manifest_stat.st_mode) != 0o600:
    fail("source manifest is not a safe private regular file")
source_manifest_bytes = open(source_manifest_path, "rb").read()
if hashlib.sha256(source_manifest_bytes).hexdigest() != source_manifest_sha:
    fail("source manifest changed after Cargo fetch")
source_manifest = json.loads(source_manifest_bytes)
if source_manifest.get("source_commit") != commit or source_manifest.get("source_tree") != tree or source_manifest.get("lock_sha256") != lock_sha or source_manifest.get("generator_input") is not False:
    fail("source manifest does not describe the reviewed Cargo-only source")
target_root = os.path.join(os.path.dirname(cargo_home), "target")
if os.path.lexists(target_root):
    fail("Cargo target output exists; this phase permits no compilation")
proxy_names = {"cargo", "cargo-clippy", "cargo-fmt", "clippy-driver", "rustc", "rustdoc", "rustfmt", "rustup"}
cargo_bin = os.path.join(cargo_home, "bin")
cargo_bin_stat = os.lstat(cargo_bin)
if not stat.S_ISDIR(cargo_bin_stat.st_mode) or (cargo_bin_stat.st_uid, cargo_bin_stat.st_gid, stat.S_IMODE(cargo_bin_stat.st_mode)) != (65534, 65534, 0o700):
    fail("Cargo proxy directory is missing or unsafe")
if set(os.listdir(cargo_bin)) != proxy_names:
    fail("Cargo proxy inventory differs from the verified Rustup runtime")
for name in sorted(proxy_names):
    path = os.path.join(cargo_bin, name)
    item = os.lstat(path)
    if not stat.S_ISLNK(item.st_mode) or (item.st_uid, item.st_gid) != (65534, 65534) or os.readlink(path) != "/tools/rustup-proxy/" + name:
        fail("Cargo proxy target differs from the verified Rustup runtime")
for blocked in ("credentials", "credentials.toml", "config", "config.toml"):
    if os.path.lexists(os.path.join(cargo_home, blocked)):
        fail("Cargo home contains an authentication or source configuration file")
if os.path.lexists(os.path.join(cargo_home, "git")):
    fail("Cargo fetch created Git state despite the registry-only lockfile")
cache_root = os.path.join(cargo_home, "registry", "cache")
if not os.path.isdir(cache_root) or os.path.islink(cache_root):
    fail("Cargo registry archive cache is missing")
archives = {}
for current, dirs, files in os.walk(cache_root, topdown=True, followlinks=False):
    dirs.sort(key=os.fsencode)
    files.sort(key=os.fsencode)
    for directory in dirs:
        if os.path.islink(os.path.join(current, directory)):
            fail("registry cache contains a symlink")
    for filename in files:
        if not filename.endswith(".crate"):
            continue
        path = os.path.join(current, filename)
        st = os.lstat(path)
        if not stat.S_ISREG(st.st_mode) or st.st_nlink != 1:
            fail("registry cache archive is not a regular file")
        if filename in archives:
            fail("duplicate registry cache archive")
        archives[filename] = path
if set(archives) != set(registry):
    fail("downloaded archive set differs from the lockfile")
for filename, expected in registry.items():
    path = archives[filename]
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    if digest.hexdigest() != expected["sha256"]:
        fail("downloaded archive checksum differs from Cargo.lock")

registry_source_root = os.path.join(cargo_home, "registry", "src")
source_root_stat = os.lstat(registry_source_root)
if not stat.S_ISDIR(source_root_stat.st_mode) or source_root_stat.st_uid != 65534 or source_root_stat.st_gid != 65534 or stat.S_IMODE(source_root_stat.st_mode) & 0o022:
    fail("Cargo extracted-source root is missing or unsafe")
registry_names = []
with os.scandir(registry_source_root) as scan:
    registry_names = sorted(list(scan), key=lambda entry: os.fsencode(entry.name))
if len(registry_names) != 1 or not registry_names[0].is_dir(follow_symlinks=False):
    fail("extracted sources do not use exactly one registry namespace")
registry_source_dir = registry_names[0].path
registry_source_stat = os.lstat(registry_source_dir)
if registry_source_stat.st_uid != 65534 or registry_source_stat.st_gid != 65534 or stat.S_IMODE(registry_source_stat.st_mode) & 0o022:
    fail("registry source namespace directory is unsafe")
source_packages = {}
with os.scandir(registry_source_dir) as scan:
    for entry in scan:
        item = os.lstat(entry.path)
        if not stat.S_ISDIR(item.st_mode) or item.st_uid != 65534 or item.st_gid != 65534 or stat.S_IMODE(item.st_mode) & 0o022:
            fail("registry source package root is not a safe directory")
        if entry.name in source_packages:
            fail("duplicate extracted registry source directory")
        source_packages[entry.name] = entry.path
expected_source_packages = {filename.removesuffix(".crate") for filename in registry}
if set(source_packages) != expected_source_packages:
    fail("extracted source directory set differs from the locked registry archive set")

metadata_stat = os.lstat(metadata_path)
if not stat.S_ISREG(metadata_stat.st_mode) or metadata_stat.st_uid != 65534 or metadata_stat.st_gid != 65534 or metadata_stat.st_nlink != 1 or stat.S_IMODE(metadata_stat.st_mode) != 0o600 or metadata_stat.st_size <= 0 or metadata_stat.st_size > (1 << 27):
    fail("offline Cargo metadata output is missing or unsafe")
metadata_bytes = open(metadata_path, "rb").read()
metadata_sha = hashlib.sha256(metadata_bytes).hexdigest()
metadata = json.loads(metadata_bytes)
if metadata.get("version") != 1 or metadata.get("workspace_root") != "/input" or metadata.get("target_directory") != "/work/target":
    fail("offline Cargo metadata identifies the wrong workspace or target path")
package_rows = metadata.get("packages")
workspace_members = metadata.get("workspace_members")
resolve = metadata.get("resolve")
if not isinstance(package_rows, list) or not isinstance(workspace_members, list) or len(workspace_members) != 8 or not isinstance(resolve, dict) or not isinstance(resolve.get("nodes"), list):
    fail("offline Cargo metadata lacks the complete workspace resolution")
locked_sources = {(entry["name"], entry["version"]) for entry in registry.values()}
package_ids = set()
path_package_ids = set()
metadata_sources = set()
for package in package_rows:
    if not isinstance(package, dict):
        fail("offline Cargo metadata has an invalid package record")
    package_id = package.get("id")
    name = package.get("name")
    version = package.get("version")
    package_manifest_path = package.get("manifest_path")
    package_source = package.get("source")
    if not all(isinstance(value, str) for value in (package_id, name, version, package_manifest_path)):
        fail("offline Cargo metadata package identity is malformed")
    if package_id in package_ids:
        fail("offline Cargo metadata contains a duplicate package id")
    package_ids.add(package_id)
    if package_source is None:
        if not package_manifest_path.startswith("/input/") or ".." in package_manifest_path.split("/") or posixpath.normpath(package_manifest_path) != package_manifest_path:
            fail("workspace package manifest escapes the source snapshot")
        relative = posixpath.relpath(package_manifest_path, "/input")
        disk_path = os.path.join(os.path.dirname(lock_path), *relative.split("/"))
        item = os.lstat(disk_path)
        if not stat.S_ISREG(item.st_mode) or item.st_uid != 0 or item.st_gid != 0 or item.st_nlink != 1:
            fail("workspace package manifest is not a safe source file")
        path_package_ids.add(package_id)
    elif package_source == "registry+https://github.com/rust-lang/crates.io-index":
        key = (name, version)
        if key not in locked_sources:
            fail("offline metadata includes a registry package absent from Cargo.lock")
        if key in metadata_sources:
            fail("offline metadata duplicates a registry package identity")
        metadata_sources.add(key)
        relative = posixpath.relpath(package_manifest_path, "/work/cargo-seed/registry/src")
        parts = relative.split("/")
        if posixpath.normpath(package_manifest_path) != package_manifest_path or len(parts) != 3 or parts[0] != registry_names[0].name or parts[1] != f"{name}-{version}" or parts[2] != "Cargo.toml":
            fail("registry package manifest path escapes its verified extracted source")
        disk_path = os.path.join(source_packages[f"{name}-{version}"], "Cargo.toml")
        item = os.lstat(disk_path)
        if not stat.S_ISREG(item.st_mode) or item.st_uid != 65534 or item.st_gid != 65534 or item.st_nlink != 1:
            fail("registry package manifest is not a safe extracted source file")
    else:
        fail("offline Cargo metadata includes a non-crates.io package source")
if len(workspace_members) != 8 or len(set(workspace_members)) != 8 or not set(workspace_members).issubset(path_package_ids):
    fail("offline Cargo metadata workspace members differ from path package records")
node_ids = {node.get("id") for node in resolve["nodes"] if isinstance(node, dict)}
if len(node_ids) != len(resolve["nodes"]) or not node_ids.issubset(package_ids) or not set(workspace_members).issubset(node_ids):
    fail("offline Cargo metadata resolution refers to missing package records")
if os.path.lexists(output_manifest_path):
    fail("Cargo seed manifest already exists")
seed_tree_sha, total_bytes = walk_tree(cargo_home)
manifest = {
    "schema": 1,
    "status": "LOCKED_CARGO_SOURCES_AND_OFFLINE_METADATA_QUALIFIED; COMPILATION_NOT_RUN; GENERATOR_ELIGIBILITY_NOT_ESTABLISHED",
    "repository": "https://github.com/tailrocks/velnor-new",
    "source_commit": commit,
    "source_tree": tree,
    "source_main_parent": main_parent,
    "source_manifest_sha256": source_manifest_sha,
    "source_snapshot_tree_sha256": source_manifest["cargo_snapshot_tree_sha256"],
    "generator_input": False,
    "lock_path": "Cargo.lock",
    "lock_sha256": lock_sha,
    "registry": "crates.io",
    "rust_toolchain": "rust@1.98.1",
    "rustup_version": "1.28.2",
    "rustup_sha256": "20a06e644b0d9bd2fbdbfd52d42540bdde820ea7df86e92e533c073da0cdd43c",
    "managed_home_helper_sha256": "5876a1cfb5a1c6a43182995af93a31cb8729fa4421e3c5466a1afe8cfb9e3f0f",
    "managed_home_preparation": "PASS; pinned helper ran once with Python isolated and no-site flags under uid/gid 65534",
    "toolchain_selection_probe": "PASS; Mise Cargo/rustc shims resolved through the sealed Rustup 1.28.2 proxy and matched the pinned Rust 1.98.1 versions",
    "cargo_home_proxy_inventory": sorted("bin/" + name for name in proxy_names),
    "cargo_fetch_command": "mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo fetch --locked --manifest-path /input/Cargo.toml",
    "compilation": "NOT RUN",
    "registry_package_count": expected_count,
    "git_package_count": 0,
    "archive_count": len(archives),
    "network_namespace": "same as host; Cargo access is constrained by the exact locked registry-only command and absence of credentials, but kernel egress destinations are not allowlisted",
    "cargo_home_tree_sha256": seed_tree_sha,
    "cargo_home_bytes": total_bytes,
    "archives": [registry[name] for name in sorted(registry)],
    "extracted_source_registry_namespace": registry_names[0].name,
    "extracted_source_package_count": len(source_packages),
    "offline_metadata_gate": "PASS; cargo metadata --locked --offline --format-version 1; no compilation or build script execution",
    "offline_metadata_sha256": metadata_sha,
    "offline_metadata_package_count": len(package_rows),
    "offline_metadata_registry_package_count": len(metadata_sources),
    "offline_metadata_workspace_member_count": len(workspace_members),
    "offline_metadata_command": "CARGO_NET_OFFLINE=true mise --no-config --no-env --no-hooks exec rust@1.98.1 -- cargo metadata --locked --offline --format-version 1 --manifest-path /input/Cargo.toml",
}
data = (json.dumps(manifest, sort_keys=True, separators=(",", ":")) + "\n").encode()
fd = os.open(output_manifest_path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
try:
    if os.write(fd, data) != len(data):
        fail("short manifest write")
    os.fsync(fd)
finally:
    os.close(fd)
print(f"Cargo seed verified: {expected_count} crates.io archives; 0 Git packages; lock {lock_sha}; tree {seed_tree_sha}")
PY
printf 'Private Cargo source seed and offline metadata are verified. No Cargo build or Velnor generator execution ran; managed-home helper execution is recorded in the manifest.\n'
