#!/usr/bin/env bash
set -euo pipefail

fail() { printf 'paired-sdk-contract-test: %s\n' "$1" >&2; exit 1; }
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/../.." && pwd -P)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/paired-sdk-contract.XXXXXXXX")" || fail 'cannot create private test directory'
trap 'rm -rf -- "$test_root"' EXIT
workspace="$test_root/workspace"; runner_temp="$test_root/runner-temp"; fake_bin="$test_root/fake-bin"; events="$test_root/events.log"
mkdir -p "$workspace" "$runner_temp" "$fake_bin"; chmod 700 "$workspace" "$runner_temp" "$fake_bin"
copy_input() { local p="$1"; mkdir -p "$workspace/$(dirname -- "$p")"; cp -p "$repo_root/$p" "$workspace/$p"; }
for p in mise.toml rust-toolchain.toml \
  fixtures/qualification/testcontainers-rust/{Cargo.toml,Cargo.lock} \
  qualification/testcontainers-tls/server.mjs \
  qualification/testcontainers/{package.json,package-lock.json,tls.mjs} \
  scripts/qualification/{paired-sdk-build.sh,paired-sdk-run.sh,paired-sdk-contract-test.sh}; do copy_input "$p"; done
mkdir -p "$workspace/fixtures/qualification/testcontainers-rust"
cp -R "$repo_root/fixtures/qualification/testcontainers-rust/src" "$workspace/fixtures/qualification/testcontainers-rust/"
mkdir -p "$workspace/.cargo"
printf '[build]\n' > "$workspace/.cargo/config.toml"
printf 'committed workspace input\n' > "$workspace/workspace-sentinel.txt"
git -C "$workspace" init -q
git -C "$workspace" config user.name contract-test
git -C "$workspace" config user.email contract-test@example.invalid
git -C "$workspace" -c core.hooksPath=/dev/null add -- .
git -C "$workspace" -c core.hooksPath=/dev/null commit -qm 'freeze qualification inputs'
source_sha="$(git -C "$workspace" rev-parse --verify HEAD)"
root_q="$(printf '%q' "$test_root")"; host_python="$(command -v python3)"
[[ -x "$host_python" ]] || fail 'python3 is required'

# One dispatcher backs command stubs; it logs labels only and never starts Cargo, Node, Docker, or the SDK.
{
  printf '#!/usr/bin/env bash\nset -euo pipefail\n'
  printf 'HARNESS_ROOT=%s\nREAL_PYTHON=%s\n' "$root_q" "$(printf '%q' "$host_python")"
  cat <<'STUB'
tool="${1:-}"; shift
log() { printf '%s\n' "$1" >> "$HARNESS_ROOT/events.log"; }
clean_env() { [[ ! ${GITHUB_TOKEN+x} && ! ${GH_TOKEN+x} && ! ${AWS_SECRET_ACCESS_KEY+x} && ! ${NODE_TLS_REJECT_UNAUTHORIZED+x} ]]; }
case "$tool" in
  python3) exec "$REAL_PYTHON" "$@" ;;
  sha256sum) exec "$REAL_PYTHON" -c 'import hashlib,pathlib,sys; a=sys.argv[1:]; a=a[1:] if a[:1]==["--"] else a; [print(hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()+"  "+p) for p in a]' "$@" ;;
  uname) case "${1:-}" in -s) echo Linux;; -m) echo x86_64;; *) exit 2;; esac ;;
  mise)
    clean_env || exit 91; log mise
    [[ "${1:-}" == --no-config && "${2:-}" == --no-env && "${3:-}" == --no-hooks && "${4:-}" == exec && "${5:-}" == -- && "${6:-}" == mbx && "${7:-}" == build ]] || exit 92
    shift 5; exec "$@" ;;
  mbx)
    log mbx; clean_env || exit 93
    [[ "${1:-}" == build && "${2:-}" == --locked && "${3:-}" == --release ]] || exit 94
    mode=valid; [[ ! -f "$HARNESS_ROOT/build-mode" ]] || mode="$(<"$HARNESS_ROOT/build-mode")"
    exec "$REAL_PYTHON" - "$HARNESS_ROOT" "$mode" "$@" <<'PY'
import json,pathlib,sys
r=pathlib.Path(sys.argv[1]); mode=sys.argv[2]; argv=sys.argv[3:]
if "--target" not in argv or argv[argv.index("--target")+1]!="x86_64-unknown-linux-gnu" or "--message-format=json-render-diagnostics" not in argv: raise SystemExit("bad build flags")
p=r/"compiler-artifact"; manifest=r/"workspace/fixtures/qualification/testcontainers-rust/Cargo.toml"; source=r/"workspace/fixtures/qualification/testcontainers-rust/src/main.rs"
if p.is_symlink() or p.is_file(): p.unlink()
elif p.exists():
 import shutil; shutil.rmtree(p)
if mode=="missing": raise SystemExit(0)
if mode=="directory": p.mkdir()
elif mode=="symlink":
 q=r/"valid-elf"; q.write_bytes(b"\x7fELF\x02\x01"+b"\0"*12+b"\x3e\x00"); q.chmod(0o755); p.symlink_to(q)
else:
 p.write_bytes(b"bad-elf" if mode=="bad-elf" else b"\x7fELF\x02\x01"+b"\0"*12+(b"\0\0" if mode=="wrong-machine" else b"\x3e\x00"))
 p.chmod(0o644 if mode=="not-executable" else 0o755)
event={"reason":"compiler-artifact","manifest_path":str(r/"elsewhere/Cargo.toml") if mode=="wrong-manifest" else str(manifest),"target":{"name":"other" if mode=="wrong-target" else "velnor-testcontainers-tls-fixture","kind":["bin"],"src_path":str(r/"elsewhere/main.rs") if mode=="wrong-src-path" else str(source)},"executable":"../compiler-artifact" if mode=="relative-executable" else str(p)}
print(json.dumps(event,separators=(",",":")))
PY
    ;;
  chmod)
    target=''; for arg in "$@"; do target="$arg"; done
    if [[ -f "$HARNESS_ROOT/fail-manifest-chmod" && "$target" == */manifest.txt ]]; then
      [[ ! -f "$HARNESS_ROOT/create-unowned-entry" ]] || : > "${target%/manifest.txt}/unowned-entry"
      exit 73
    fi
    exec /bin/chmod "$@" ;;
  timeout)
    if [[ "${1:-}" == --version ]]; then echo 'timeout (GNU coreutils) 9.0'; exit 0; fi
    clean_env || exit 96
    [[ "${1:-}" == --signal=TERM && "${2:-}" == --kill-after=10s && "${3:-}" == 180s ]] || exit 95
    log timeout; [[ ! -f "$HARNESS_ROOT/fail-timeout" ]] || exit "$(<"$HARNESS_ROOT/fail-timeout")"
    shift 3; exec "$@" ;;
  npm) [[ $# == 3 && "$1" == --prefix && "$3" == ci ]] || exit 99; log npm; [[ ! -f "$HARNESS_ROOT/fail-npm" ]] || exit 24 ;;
  node)
    [[ $# == 2 && "$1" == */qualification/testcontainers/tls.mjs ]] || exit 100
    log node; cert="$2"; [[ -f "$cert/ca.pem" && -f "$cert/server.pem" && -f "$cert/server-key.pem" ]] || exit 101
    mode="$("$REAL_PYTHON" -c 'import pathlib,stat,sys; print(format(stat.S_IMODE(pathlib.Path(sys.argv[1]).lstat().st_mode),"o"))' "$cert/server-key.pem")"
    [[ "$mode" == 600 && ! -f "$HARNESS_ROOT/fail-node" ]] || exit 25 ;;
  *) exit 90 ;;
esac
STUB
} > "$fake_bin/fake-tool"
chmod 755 "$fake_bin/fake-tool"
fake_tool_q="$(printf '%q' "$fake_bin/fake-tool")"
for tool in python3 sha256sum uname mise mbx chmod timeout npm node; do
  printf '#!/usr/bin/env bash\nset -euo pipefail\nexec %s %s "$@"\n' "$fake_tool_q" "$tool" > "$fake_bin/$tool"
  chmod 755 "$fake_bin/$tool"
done
mkdir -m 700 "$workspace/relative-bin" "$test_root/function-only-bin"
ln -s "$fake_bin/node" "$workspace/relative-bin/node"
git -C "$workspace" add -- relative-bin/node
git -C "$workspace" commit -qm 'add relative-path tool fixture'
source_sha="$(git -C "$workspace" rev-parse --verify HEAD)"
for tool in python3 sha256sum npm timeout uname; do ln -s "$fake_bin/$tool" "$test_root/function-only-bin/$tool"; done

# This artifact is executable shell test code; the builder path below emits a separate ELF fixture.
{
  printf '#!/usr/bin/env bash\nset -euo pipefail\nHARNESS_ROOT=%s\n' "$root_q"
  cat <<'STUB'
[[ $# == 2 ]] || exit 103
case "$1" in
  certs) printf '%s\n' certs >> "$HARNESS_ROOT/events.log"; [[ ! -f "$HARNESS_ROOT/fail-certs" ]] || exit 26; d="$2"; printf ca > "$d/ca.pem"; printf cert > "$d/server.pem"; printf key > "$d/server-key.pem"; /bin/chmod 600 "$d/server-key.pem" ;;
  sdk) printf '%s\n' sdk >> "$HARNESS_ROOT/events.log"; [[ ! -f "$HARNESS_ROOT/fail-sdk" ]] || exit 27 ;;
  *) exit 105 ;;
esac
STUB
} > "$test_root/runtime-binary"
chmod 755 "$test_root/runtime-binary"

canary='PAIRED_SDK_CONTRACT_CANARY_DO_NOT_LOG'; printf 'sentinel\n' > "$runner_temp/valuable-sentinel"; chmod 600 "$runner_temp/valuable-sentinel"
run_build() (
  local artifact="$1" build_workspace="${2:-$workspace}" sha
  sha="$(git -C "$build_workspace" rev-parse --verify HEAD)"
  [[ ! -f "$test_root/wrong-build-sha" ]] || sha=0000000000000000000000000000000000000000
  cd "$build_workspace"
  env PATH="$fake_bin:$PATH" GITHUB_WORKSPACE="$build_workspace" GITHUB_SHA="$sha" RUNNER_TEMP="$runner_temp" \
    GITHUB_TOKEN="$canary" GH_TOKEN="$canary" AWS_SECRET_ACCESS_KEY="$canary" HARNESS_ROOT="$test_root" \
    bash "$build_workspace/scripts/qualification/paired-sdk-build.sh" "$artifact"
)
run_test() (
  local artifact="$1" expected mode path="$fake_bin:$PATH"
  expected="$(sed -n '6s/^binary_sha256=//p' "$runner_temp/run-template/manifest.txt")"
  unset DOCKER_HOST TESTCONTAINERS_HOST_OVERRIDE DOCKER_CONTEXT; DOCKER_HOST=tcp://localhost:2375
  mode=defaults; [[ ! -f "$test_root/run-env-mode" ]] || mode="$(cat "$test_root/run-env-mode")"
  case "$mode" in
    defaults) ;;
    empty-override) export TESTCONTAINERS_HOST_OVERRIDE='' ;;
    set-override) export TESTCONTAINERS_HOST_OVERRIDE=spoofed.invalid ;;
    empty-context) export DOCKER_CONTEXT='' ;;
    wrong-docker-host) DOCKER_HOST=tcp://wrong.invalid:2375 ;;
    invalid-expected) expected=invalid ;;
    wrong-expected) expected=ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff ;;
    exported-node-function)
      # shellcheck disable=SC2329 # Exported for the child Bash to prove PATH executables take precedence.
      node() { printf '%s\n' node-function >> "$test_root/events.log"; return 111; }; export -f node ;;
    function-only-node)
      path="$test_root/function-only-bin:/usr/bin:/bin"
      # shellcheck disable=SC2329 # Exported for the child Bash; it must not satisfy an external-tool lookup.
      node() { printf '%s\n' node-function >> "$test_root/events.log"; return 111; }; export -f node ;;
    relative-node) path="relative-bin:$fake_bin:$PATH"; cd "$test_root" ;;
    *) fail 'unknown run test mode' ;;
  esac
  export PATH="$path" GITHUB_WORKSPACE="$workspace" GITHUB_SHA="$source_sha" RUNNER_TEMP="$runner_temp"
  export VELNOR_PAIRED_SDK_BINARY_SHA256="$expected" DOCKER_HOST GITHUB_TOKEN="$canary" GH_TOKEN="$canary"
  export AWS_SECRET_ACCESS_KEY="$canary" NODE_TLS_REJECT_UNAUTHORIZED=0 HARNESS_ROOT="$test_root"
  bash "$workspace/scripts/qualification/paired-sdk-run.sh" "$artifact"
)
expect_fail() {
  local label="$1" status=0; shift; set +e
  "$@" > "$test_root/out" 2> "$test_root/err" || status=$?
  set -e; [[ "$status" != 0 ]] || fail "$label unexpectedly succeeded"
  ! grep -Fq "$canary" "$test_root/out" "$test_root/err" || fail "$label leaked a canary"
}
assert_events() { printf '%s\n' "$@" > "$test_root/expected-events"; cmp -s "$test_root/expected-events" "$events" || { diff -u "$test_root/expected-events" "$events" >&2 || true; fail 'wrong command sequence'; }; }
no_scratch() { local prefix="$1" p; for p in "$runner_temp"/"$prefix".*; do [[ ! -e "$p" && ! -L "$p" ]] || fail "leftover $prefix scratch"; done; }
reject_run() { : > "$events"; expect_fail "$1" run_test "$2"; [[ ! -s "$events" ]] || fail "$1 entered SDK runtime"; no_scratch velnor-paired-sdk-run; }
reject_build_preflight() { local workspace_path="${3:-$workspace}"; : > "$events"; expect_fail "$1" run_build "$2" "$workspace_path"; [[ ! -s "$events" ]] || fail "$1 invoked Mise"; no_scratch velnor-paired-sdk-build; }

# Builder success binds the selected package, binary source, ELF architecture, modes, and exact seven-line handoff.
build_artifact="$runner_temp/build-success"; : > "$events"
run_build "$build_artifact" > "$test_root/out" 2> "$test_root/err" || fail 'valid builder contract failed'
binary_sha="$(sha256sum -- "$build_artifact/velnor-testcontainers-tls-fixture" | awk '{print $1}')"
[[ "$(<"$test_root/out")" == "binary_sha256=$binary_sha" ]] || fail 'builder stdout contract changed'
assert_events mise mbx
python3 - "$build_artifact" <<'PY' || fail 'builder output shape/modes/ELF check failed'
import os,pathlib,stat,sys
r=pathlib.Path(sys.argv[1]); expected={"manifest.txt","velnor-testcontainers-tls-fixture"}
if set(os.listdir(r))!=expected: raise SystemExit(1)
for n,m in (("manifest.txt",0o644),("velnor-testcontainers-tls-fixture",0o755)):
 p=r/n; s=p.lstat()
 if not stat.S_ISREG(s.st_mode) or s.st_nlink!=1 or stat.S_IMODE(s.st_mode)!=m: raise SystemExit(1)
with (r/"velnor-testcontainers-tls-fixture").open("rb") as f: h=f.read(20)
if h[:6]!=b"\x7fELF\x02\x01" or h[18:20]!=b"\x3e\x00": raise SystemExit(1)
PY
printf '%s\n' 'schema=velnor-paired-sdk-artifact-v1' "source_sha=$source_sha" \
  "fixture_manifest_sha256=$(sha256sum -- "$workspace/fixtures/qualification/testcontainers-rust/Cargo.toml" | awk '{print $1}')" \
  "fixture_lock_sha256=$(sha256sum -- "$workspace/fixtures/qualification/testcontainers-rust/Cargo.lock" | awk '{print $1}')" \
  "server_sha256=$(sha256sum -- "$workspace/qualification/testcontainers-tls/server.mjs" | awk '{print $1}')" \
  "binary_sha256=$binary_sha" 'target=x86_64-unknown-linux-gnu' > "$test_root/expected-manifest"
cmp -s "$test_root/expected-manifest" "$build_artifact/manifest.txt" || fail 'builder manifest fields/order differ'
no_scratch velnor-paired-sdk-build

# Preflight rejection preserves pre-existing exact-name and unrelated entries; late failure removes only owned files.
: > "$test_root/wrong-build-sha"; reject_build_preflight 'wrong build SHA' "$runner_temp/wrong-sha"; rm -f "$test_root/wrong-build-sha"
prefilled="$runner_temp/prefilled"; mkdir -m 700 "$prefilled"; printf old > "$prefilled/manifest.txt"; printf keep > "$prefilled/unrelated"
before="$(sha256sum -- "$prefilled/manifest.txt" "$prefilled/unrelated")"
reject_build_preflight 'nonempty existing artifact' "$prefilled"
[[ "$(sha256sum -- "$prefilled/manifest.txt" "$prefilled/unrelated")" == "$before" ]] || fail 'preflight modified existing artifact files'
printf fail > "$test_root/fail-manifest-chmod"; partial="$runner_temp/partial"
expect_fail 'owned partial-output cleanup' run_build "$partial"
[[ ! -e "$partial" && -f "$runner_temp/valuable-sentinel" ]] || fail 'partial cleanup removed a sibling or retained owned output'
: > "$test_root/create-unowned-entry"; partial="$runner_temp/partial-with-unowned"
expect_fail 'unowned-entry preservation' run_build "$partial"
[[ -f "$partial/unowned-entry" && ! -e "$partial/manifest.txt" && ! -e "$partial/velnor-testcontainers-tls-fixture" ]] || fail 'cleanup crossed its exact-file ownership boundary'
rm -rf -- "$partial"; rm -f "$test_root/fail-manifest-chmod" "$test_root/create-unowned-entry"

# Whole-checkout cleanliness includes unrelated tracked files; tracked Cargo config is accepted only unchanged.
git -C "$workspace" ls-files --error-unmatch -- .cargo/config.toml >/dev/null || fail 'clean Cargo config is not Git-bound'
printf 'modified committed input\n' > "$workspace/workspace-sentinel.txt"
reject_build_preflight 'dirty tracked checkout' "$runner_temp/dirty-checkout"
printf 'committed workspace input\n' > "$workspace/workspace-sentinel.txt"
printf '[build]\nmodified=true\n' > "$workspace/.cargo/config.toml"
reject_build_preflight 'modified tracked Cargo config' "$runner_temp/modified-config"
printf '[build]\n' > "$workspace/.cargo/config.toml"

# Ignored Cargo config must be rejected independently of git status at every workspace/fixture ancestor.
untracked_workspace="$test_root/untracked-config-workspace"
git clone -q --local "$workspace" "$untracked_workspace"
git -C "$untracked_workspace" rm -q -- .cargo/config.toml
git -C "$untracked_workspace" -c user.name=contract-test -c user.email=contract-test@example.invalid commit -qm 'remove tracked test config'
cargo_config_dirs=(.cargo fixtures/.cargo fixtures/qualification/.cargo fixtures/qualification/testcontainers-rust/.cargo)
for cargo_config_dir in "${cargo_config_dirs[@]}"; do
  for cargo_config_name in config.toml config; do
    cargo_config_relative="$cargo_config_dir/$cargo_config_name"
    cargo_config_file="$untracked_workspace/$cargo_config_relative"
    mkdir -p "$(dirname -- "$cargo_config_file")"
    printf '[build]\n' > "$cargo_config_file"
    reject_build_preflight 'untracked Cargo config' "$runner_temp/untracked-config" "$untracked_workspace"
    rm -f -- "$cargo_config_file"
    printf '/%s\n' "$cargo_config_relative" >> "$untracked_workspace/.git/info/exclude"
    printf '[build]\n' > "$cargo_config_file"
    [[ -z "$(git -C "$untracked_workspace" status --porcelain --untracked-files=all -- "$cargo_config_relative")" ]] || fail 'ignored Cargo config appeared in Git status'
    git -C "$untracked_workspace" check-ignore -q -- "$cargo_config_relative" || fail 'Cargo config fixture was not Git-ignored'
    reject_build_preflight 'ignored Cargo config' "$runner_temp/ignored-config" "$untracked_workspace"
    rm -f -- "$cargo_config_file"
    rmdir -- "$(dirname -- "$cargo_config_file")" 2>/dev/null || true
  done
done

# Malformed or mismatched Cargo events never produce a handoff artifact.
for mode in missing wrong-target wrong-manifest wrong-src-path relative-executable bad-elf wrong-machine directory symlink not-executable; do
  printf '%s\n' "$mode" > "$test_root/build-mode"; output="$runner_temp/rejected-$mode"
  : > "$events"; expect_fail "compiler event $mode" run_build "$output"; assert_events mise mbx
  [[ ! -e "$output" && ! -L "$output" ]] || fail "bad Cargo event left $mode output"
done
rm -f "$test_root/build-mode"

# Make a runnable shell stub from the builder manifest; runtime verification still checks its exact independent digest.
run_template="$runner_temp/run-template"; mkdir -m 700 "$run_template"
cp -p "$build_artifact/manifest.txt" "$run_template/manifest.txt"; cp -p "$test_root/runtime-binary" "$run_template/velnor-testcontainers-tls-fixture"
runtime_sha="$(sha256sum -- "$run_template/velnor-testcontainers-tls-fixture" | awk '{print $1}')"
python3 - "$run_template/manifest.txt" "$runtime_sha" <<'PY'
import pathlib,sys
p=pathlib.Path(sys.argv[1]); a=p.read_text().splitlines()
if len(a)!=7 or not a[5].startswith("binary_sha256="): raise SystemExit(1)
a[5]="binary_sha256="+sys.argv[2]; p.write_text("\n".join(a)+"\n")
PY
chmod 644 "$run_template/manifest.txt"
clone_artifact() { local d="$runner_temp/$1"; mkdir -m 700 "$d"; cp -p "$run_template/manifest.txt" "$run_template/velnor-testcontainers-tls-fixture" "$d/"; printf '%s\n' "$d"; }
wrong_field() { python3 - "$1" "$2" <<'PY'
import pathlib,sys
p=pathlib.Path(sys.argv[1]); k=sys.argv[2]; a=p.read_text().splitlines(); ix=[i for i,x in enumerate(a) if x.startswith(k+"=")]
if len(ix)!=1: raise SystemExit(1)
a[ix[0]]=k+"="+("0"*64); p.write_text("\n".join(a)+"\n")
PY
}

# Shape, raw bytes, source, lock, server, and both binary digests are validated before timeout.
for kind in extra missing-manifest missing-binary symlink nonregular trailing nul source_sha fixture_manifest_sha256 fixture_lock_sha256 server_sha256 binary_sha256; do
  bad="$(clone_artifact "bad-$kind")"
  case "$kind" in
    extra) : > "$bad/extra";; missing-manifest) rm -f "$bad/manifest.txt";; missing-binary) rm -f "$bad/velnor-testcontainers-tls-fixture";;
    symlink) rm -f "$bad/velnor-testcontainers-tls-fixture"; ln -s "$run_template/velnor-testcontainers-tls-fixture" "$bad/velnor-testcontainers-tls-fixture";;
    nonregular) rm -f "$bad/velnor-testcontainers-tls-fixture"; mkdir "$bad/velnor-testcontainers-tls-fixture";;
    trailing) printf 'unbound=tail' >> "$bad/manifest.txt";; nul) printf '\0' >> "$bad/manifest.txt";;
    *) wrong_field "$bad/manifest.txt" "$kind";;
  esac
  reject_run "hostile artifact $kind" "$bad"
done
bad="$(clone_artifact tampered-binary)"; printf tamper >> "$bad/velnor-testcontainers-tls-fixture"; reject_run 'binary byte tampering' "$bad"
bad="$(clone_artifact wrong-builder-digest)"; printf wrong-expected > "$test_root/run-env-mode"
expect_fail 'independent builder digest mismatch' run_test "$bad"; [[ ! -s "$events" ]] || fail 'wrong builder digest reached runtime'; no_scratch velnor-paired-sdk-run; rm -f "$test_root/run-env-mode"

# Poisoned Docker routing, function-only tools, and relative PATH hits are rejected before child execution.
valid="$runner_temp/valid"; mkdir -m 700 "$valid"; cp -p "$run_template/manifest.txt" "$run_template/velnor-testcontainers-tls-fixture" "$valid/"
for mode in empty-override set-override empty-context wrong-docker-host invalid-expected function-only-node relative-node; do
  printf '%s\n' "$mode" > "$test_root/run-env-mode"; reject_run "unsafe run environment $mode" "$valid"
done

# A hostile exported function must not shadow the validated external Node executable.
printf exported-node-function > "$test_root/run-env-mode"; : > "$events"
if ! run_test "$valid" > "$test_root/out" 2> "$test_root/err"; then
  ! grep -Fq "$canary" "$test_root/err" || fail 'external Node failure exposed a canary'
  sed -n '1p' "$test_root/err" >&2
  fail 'external Node resolution lost to an exported function'
fi
[[ "$(<"$test_root/out")" == paired-sdk-tls-ok ]] || fail 'positive run marker changed'
assert_events timeout certs npm node sdk; ! grep -Fq node-function "$events" || fail 'run invoked exported Node function'
no_scratch velnor-paired-sdk-run; rm -f "$test_root/run-env-mode"

# Positive order and per-stage failures prove short-circuiting, finite timeout arguments, env scrubbing, and cleanup.
: > "$events"; run_test "$valid" > "$test_root/out" 2> "$test_root/err" || fail 'positive run contract failed'
[[ "$(<"$test_root/out")" == paired-sdk-tls-ok ]] || fail 'run success marker was not exact'
assert_events timeout certs npm node sdk; no_scratch velnor-paired-sdk-run
for stage in timeout certs npm node sdk; do
  printf fail > "$test_root/fail-$stage"; [[ "$stage" != timeout ]] || printf 124 > "$test_root/fail-timeout"
  : > "$events"; expect_fail "runtime stage $stage" run_test "$valid"
  case "$stage" in
    timeout) assert_events timeout;; certs) assert_events timeout certs;; npm) assert_events timeout certs npm;;
    node) assert_events timeout certs npm node;; sdk) assert_events timeout certs npm node sdk;;
  esac
  no_scratch velnor-paired-sdk-run; rm -f "$test_root/fail-$stage"
done
grep -Fq sentinel "$runner_temp/valuable-sentinel" || fail 'cleanup removed unrelated RUNNER_TEMP data'
! grep -Fq "$canary" "$test_root/out" "$test_root/err" "$events" || fail 'a diagnostic exposed the canary value'
printf '%s\n' 'paired-sdk-contract-test: build and run stub contracts passed'
