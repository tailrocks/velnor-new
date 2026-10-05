#!/usr/bin/bash
# Review-only child runner. Enter through the hash-bound Python bootstrap.
set -Eeuo pipefail
umask 077

readonly TASK=/root/.velnor-generator-preview-20261005
readonly ROOTFS=$TASK/rootfs
readonly WORK=$TASK/work
readonly OUT=$TASK/out
readonly MAIN_SRC=$TASK/main-source-full
readonly PR_SRC=$TASK/pr65-source
readonly MAIN_HELPER=$TASK/main-helper-37279676047/velnor-actions
readonly MAIN_MANIFEST=$TASK/main-helper-37279676047/preseed-manifest.json
readonly MAIN_ARCHIVE=$TASK/main-source-ecf-tree.tar
readonly PR_HELPER=$TASK/pr65-helper-37275543333/velnor-actions
readonly PR_MANIFEST=$TASK/pr65-helper-37275543333/preseed-manifest.json
readonly PR_ARCHIVE=$TASK/pr65-source-tree.tar
readonly BASE_MANIFEST=$TASK/rootfs-base-manifest.json
readonly STAGE_MANIFEST=$TASK/rootfs-stage-manifest.json
readonly EXPECTED_BASE_MANIFEST_SHA256=1a69b71279d9c5a7e7c1c5937d9c55898f1e5b0f1fc7405010d6ffb273f47acf
readonly EXPECTED_STAGE_MANIFEST_SHA256=
readonly EXPECTED_ROOTFS_TREE_SHA256=1cef88b359a08c32cccb6078a55309d4db67b883dc260ccc3bbeb80ddce55763
readonly EXPECTED_INSIDE_SHA256=
readonly EXPECTED_NAMESPACE_SHA256=
readonly GIT_CONFIG_AUDIT=$TASK/preseed-preview-git-config-audit.py

die() { printf 'preseed preview: %s\n' "$*" >&2; exit 1; }

git_safe() {
  /usr/bin/env -i HOME=/nonexistent PATH=/usr/bin:/bin \
    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_CONFIG_NOSYSTEM=1 \
    GIT_CONFIG_COUNT=3 \
    GIT_CONFIG_KEY_0=core.fsmonitor GIT_CONFIG_VALUE_0=false \
    GIT_CONFIG_KEY_1=core.hooksPath GIT_CONFIG_VALUE_1=/dev/null \
    GIT_CONFIG_KEY_2=diff.external GIT_CONFIG_VALUE_2= \
    GIT_NO_LAZY_FETCH=1 GIT_PROTOCOL_FROM_USER=0 \
    GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 \
    /usr/bin/git -c core.fsmonitor=false -c core.hooksPath=/dev/null \
    -c diff.external= "$@"
}

audit_source_git_config() {
  local source=$1
  [[ -d $source/.git && ! -L $source/.git ]] || die "source must have a direct Git metadata directory"
  [[ -f $GIT_CONFIG_AUDIT && ! -L $GIT_CONFIG_AUDIT ]] || die "Git config audit program is missing"
  /usr/bin/env -i PATH=/usr/bin:/bin /usr/bin/python3 -I -S "$GIT_CONFIG_AUDIT" "$source"
}

check_root_host() {
  [[ $(/usr/bin/id -u) == 0 ]] || die "effective uid is not root"
  [[ $(/usr/bin/hostname -s) == bastion ]] || die "host is not bastion"
  [[ -d $TASK && ! -L $TASK && $(/usr/bin/stat -c '%u:%g:%a' "$TASK") == 0:0:700 ]] || die "task directory identity changed"
  if /usr/bin/pgrep -u 65534 >/dev/null 2>&1; then die "uid 65534 is already in use"; fi
  if /usr/bin/findmnt -rn -o TARGET | /usr/bin/awk -v p="$TASK" '$0==p || index($0,p"/")==1 { found=1 } END { exit !found }'; then
    die "task-owned mount already exists"
  fi
}

verify_profile() {
  local profile=$1
  case "$profile" in
    main)
      SOURCE=$MAIN_SRC
      SOURCE_COMMIT=ecf54b771c0db11e03e54fa2c98cf531e5912585
      SOURCE_TREE=0e4e8eac8d9ea32df264d74e614427936480af7d
      SOURCE_ARCHIVE=$MAIN_ARCHIVE
      SOURCE_ARCHIVE_SHA=c2b3c12edfc19e1553e8cfcf5f24f06844191a4a4cce45dc44f45a8eef20d28a
      HELPER=$MAIN_HELPER
      HELPER_SHA=048b4322ed3875941fc72cc1f5bae91c4d0b89fab203772663ff18fbe008c53c
      MANIFEST=$MAIN_MANIFEST
      MANIFEST_SHA=33fd434cf0fb99a74798946c89bcd8f2a306d2508a09efe056b4f1f16ee19049
      PRESEED_COMMIT=ecf54b771c0db11e03e54fa2c98cf531e5912585
      ;;
    pr65)
      SOURCE=$PR_SRC
      SOURCE_COMMIT=ff41745394712604bddf94b6a4e9f1069100789e
      SOURCE_TREE=1546bdd222f10b429a23769e69c02ac28610768f
      SOURCE_ARCHIVE=$PR_ARCHIVE
      SOURCE_ARCHIVE_SHA=c3a6a18ec1c80ab9e50db562df2f28fb26325a7de97ea2f95893ff77446be56d
      HELPER=$PR_HELPER
      HELPER_SHA=58b7d4c9473cdac6381b21e83f7508547e1d52c2f6f5962c4c22cf200316ba93
      MANIFEST=$PR_MANIFEST
      MANIFEST_SHA=4db220b92aafdbdb1ec949d84b53e9b6060a2a96d5a884750da8b936c6c37abf
      PRESEED_COMMIT=eec43f9c4c5f6113e4d184f9696a7a1c7c2ec6e2
      ;;
    *) die "unknown profile" ;;
  esac

  [[ -d $SOURCE && ! -L $SOURCE ]] || die "source directory missing or symlinked"
  audit_source_git_config "$SOURCE"
  [[ $(git_safe -C "$SOURCE" rev-parse HEAD) == "$SOURCE_COMMIT" ]] || die "source commit changed"
  [[ $(git_safe -C "$SOURCE" rev-parse 'HEAD^{tree}') == "$SOURCE_TREE" ]] || die "source tree changed"
  [[ $(git_safe -C "$SOURCE" remote get-url origin) == https://github.com/tailrocks/velnor-new.git ]] || die "repository identity changed"
  [[ $(git_safe -C "$SOURCE" config --local --get core.hooksPath) == /dev/null ]] || die "source hook path is not disabled"
  [[ -z $(git_safe -C "$SOURCE" config --local --get-all credential.helper 2>/dev/null || true) ]] || die "source has a credential helper"
  [[ -z $(git_safe -C "$SOURCE" config --local --get-all http.extraHeader 2>/dev/null || true) ]] || die "source has an HTTP header"
  [[ -z $(git_safe -C "$SOURCE" config --local --get-all core.sshCommand 2>/dev/null || true) ]] || die "source has an SSH command"
  [[ -z $(git_safe -C "$SOURCE" config --local --get-all include.path 2>/dev/null || true) ]] || die "source has a config include"
  [[ -z $(git_safe -C "$SOURCE" status --porcelain=v1) ]] || die "source worktree is dirty"
  [[ -f $HELPER && ! -L $HELPER && $(/usr/bin/stat -c '%u:%g:%a' "$HELPER") == 0:0:755 ]] || die "helper file identity changed"
  [[ $(/usr/bin/sha256sum "$HELPER" | /usr/bin/cut -d' ' -f1) == "$HELPER_SHA" ]] || die "helper digest mismatch"
  [[ -f $MANIFEST && ! -L $MANIFEST ]] || die "preseed manifest missing or symlinked"
  [[ $(/usr/bin/sha256sum "$MANIFEST" | /usr/bin/cut -d' ' -f1) == "$MANIFEST_SHA" ]] || die "preseed manifest digest mismatch"
  [[ -f $SOURCE_ARCHIVE && ! -L $SOURCE_ARCHIVE ]] || die "source archive missing or symlinked"
  [[ $(/usr/bin/sha256sum "$SOURCE_ARCHIVE" | /usr/bin/cut -d' ' -f1) == "$SOURCE_ARCHIVE_SHA" ]] || die "source archive digest mismatch"
  /usr/bin/python3 - "$MANIFEST" "$PRESEED_COMMIT" "$HELPER_SHA" <<'PY'
import json, os, stat, sys
path, commit, binary_sha = sys.argv[1:]
fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC)
st = os.fstat(fd)
if not stat.S_ISREG(st.st_mode) or st.st_uid != 0 or st.st_mode & 0o022 or st.st_size > 4096:
    raise SystemExit("unsafe preseed manifest")
with os.fdopen(fd, "rb") as f:
    manifest = json.load(f)
expected = {
    "schema": 1,
    "commit": commit,
    "target": "x86_64-unknown-linux-gnu",
    "toolchain": "rust@1.98.1+mr-boxington@1.21.1",
    "sha256": binary_sha,
}
if manifest != expected:
    raise SystemExit("preseed manifest fields differ from the reviewed pair")
PY
  [[ -d $ROOTFS && ! -L $ROOTFS && $(/usr/bin/stat -c '%u:%g' "$ROOTFS") == 0:0 ]] || die "staged rootfs missing or unsafe"
  [[ $(/usr/bin/sha256sum "$BASE_MANIFEST" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_BASE_MANIFEST_SHA256" ]] || die "executed base manifest digest mismatch"
  [[ $EXPECTED_ROOTFS_TREE_SHA256 =~ ^[0-9a-f]{64}$ ]] || die "rootfs tree is not sealed"
  [[ $EXPECTED_STAGE_MANIFEST_SHA256 =~ ^[0-9a-f]{64}$ ]] || die "final stage manifest seal is unavailable"
  [[ $EXPECTED_INSIDE_SHA256 =~ ^[0-9a-f]{64}$ ]] || die "inside runner seal is unavailable"
  [[ $EXPECTED_NAMESPACE_SHA256 =~ ^[0-9a-f]{64}$ ]] || die "namespace runner seal is unavailable"
  [[ -f $STAGE_MANIFEST && ! -L $STAGE_MANIFEST ]] || die "reviewed stage manifest is missing"
  [[ $(/usr/bin/sha256sum "$STAGE_MANIFEST" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_STAGE_MANIFEST_SHA256" ]] || die "stage manifest digest mismatch"
  [[ -f $ROOTFS/tools/velnor-actions && ! -L $ROOTFS/tools/velnor-actions ]] || die "staged helper target missing"
  [[ -f $ROOTFS/tools/preseed-manifest.json && ! -L $ROOTFS/tools/preseed-manifest.json ]] || die "staged manifest target missing"
  [[ -f $ROOTFS/tools/run-preview.sh && ! -L $ROOTFS/tools/run-preview.sh ]] || die "inside runner target missing"
  [[ -d $ROOTFS/input && -d $ROOTFS/work && -d $ROOTFS/out ]] || die "rootfs mountpoints are missing"
  [[ -d $WORK/$profile && $(/usr/bin/stat -c '%u:%g:%a' "$WORK/$profile") == 65534:65534:700 ]] || die "private work directory is missing"
  [[ -d $OUT/$profile && $(/usr/bin/stat -c '%u:%g:%a' "$OUT/$profile") == 65534:65534:700 ]] || die "private output directory is missing"
  [[ -z $(/usr/bin/find "$OUT/$profile" -mindepth 1 -print -quit) ]] || die "private output directory is not empty"
}

run_pair() {
  local profile=$1
  verify_profile "$profile"
  local source=$SOURCE helper=$HELPER manifest=$MANIFEST
  local inside=$TASK/preseed-preview-inside.sh
  local work=$WORK/$profile out=$OUT/$profile

  [[ $(/usr/bin/sha256sum "$inside" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_INSIDE_SHA256" ]] || die "inside runner digest mismatch"
  [[ $(/usr/bin/stat -c '%u:%g:%a' "$inside") == 0:0:700 ]] || die "inside runner file identity changed"
  [[ -x $TASK/preseed-preview-namespace.sh && ! -L $TASK/preseed-preview-namespace.sh ]] || die "namespace runner is missing or symlinked"
  [[ $(/usr/bin/sha256sum "$TASK/preseed-preview-namespace.sh" | /usr/bin/cut -d' ' -f1) == "$EXPECTED_NAMESPACE_SHA256" ]] || die "namespace runner digest mismatch"
  [[ $(/usr/bin/stat -c '%u:%g:%a' "$TASK/preseed-preview-namespace.sh") == 0:0:700 ]] || die "namespace runner file identity changed"

  ulimit -t 600
  ulimit -f 1048576
  ulimit -u 128
  ulimit -n 64
  ulimit -v 8388608
  ulimit -c 0
  /usr/bin/timeout --foreground --signal=TERM --kill-after=10s 1200s \
    /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
      /usr/bin/unshare --mount --net --pid --ipc --uts --fork --kill-child --propagation private -- \
      /root/.velnor-generator-preview-20261005/preseed-preview-namespace.sh \
      "$profile" "$ROOTFS" "$source" "$helper" "$manifest" "$inside" "$work" "$out"

  if /usr/bin/pgrep -u 65534 >/dev/null 2>&1; then die "task process remains after namespace exit"; fi
  if /usr/bin/findmnt -rn -o TARGET | /usr/bin/awk -v p="$TASK" '$0==p || index($0,p"/")==1 { found=1 } END { exit !found }'; then
    die "task mount remains after namespace exit"
  fi
  /usr/bin/diff -ru "$out/$profile-a/.github" "$out/$profile-b/.github" || die "generator output differs across identical passes"
  /usr/bin/printf '%s preview was byte-identical across two isolated passes.\n' "$profile"
}

check_root_host
case ${1:-} in
  main) run_pair main ;;
  pr65) run_pair pr65 ;;
  *) die "usage: preseed-preview-runner.sh {main|pr65}" ;;
esac
