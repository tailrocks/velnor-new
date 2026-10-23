pub(super) const MISE_MOCK: &str = r#"#!/bin/bash
printf '%s\n' "$*" >> "$MISE_CALLS"
printf 'mise\n' >> "$TRACE"
[[ "$*" == "--no-config --no-env --no-hooks exec rust@${MOCK_RUST_VERSION} -- cargo build --locked --manifest-path crates/velnor-runner/Cargo.toml --package velnor-resource-probe --bin velnor-storage-init --bin velnor-job-start-hook --release --target x86_64-unknown-linux-musl" ]] || exit 97
mkdir -p "$(dirname "$MOCK_STORAGE_INIT")"
rm -f "$MOCK_STORAGE_INIT"
[[ "$MOCK_UTILITY_KIND" == missing ]] || printf utility > "$MOCK_STORAGE_INIT"
rm -f "$MOCK_JOB_START_HOOK"
[[ "$MOCK_HOOK_KIND" == missing ]] || printf utility > "$MOCK_JOB_START_HOOK"
"#;

pub(super) const FILE_MOCK: &str = r#"#!/bin/bash
case "$*:$MOCK_UTILITY_KIND:$MOCK_HOOK_KIND" in
  *velnor-storage-init:wrong-arch:*) printf 'Mach-O arm64\n' ;;
  *velnor-storage-init:dynamic:*) printf 'ELF 64-bit LSB pie executable, x86-64, dynamically linked\n' ;;
  *velnor-storage-init:bad:*) printf 'data\n' ;;
  *velnor-job-start-hook:*:wrong-arch) printf 'Mach-O arm64\n' ;;
  *velnor-job-start-hook:*:dynamic) printf 'ELF 64-bit LSB pie executable, x86-64, dynamically linked\n' ;;
  *velnor-job-start-hook:*:bad) printf 'data\n' ;;
  *) printf 'ELF 64-bit LSB pie executable, x86-64, static-pie linked\n' ;;
esac
"#;

pub(super) const DOCKER_MOCK: &str = r#"#!/bin/bash
printf 'docker\n' >> "$TRACE"
printf '<%s>' "$@" >> "$DOCKER_CALLS"
printf '\n' >> "$DOCKER_CALLS"
previous=''
for argument in "$@"; do
  if [[ "$previous" == '--format' && "$argument" == '{{.Architecture}}' ]]; then printf 'amd64\n'; fi
  if [[ "$previous" == '--format' && "$argument" == '{{.Id}}' ]]; then printf 'sha256:%064d\n' 0; fi
  if [[ "$previous" == '--entrypoint' && "$argument" == '/usr/bin/uname' ]]; then printf 'x86_64\n'; fi
  if [[ "$previous" == '--entrypoint' && "$argument" == '/usr/bin/sha256sum' ]]; then entrypoint=sha256sum; fi
  if [[ "$previous" == '--entrypoint' && "$argument" == '/usr/bin/stat' ]]; then entrypoint=stat; fi
  previous="$argument"
done
case " $* " in
  *'/velnor/storage-init'*)
    if [[ "$entrypoint" == sha256sum ]]; then
      if [[ "$ACTUAL_INIT_SHA" == good ]]; then printf '%s  /velnor/storage-init\n' "$EXPECTED_INIT_SHA"; else printf '%s  /velnor/storage-init\n' "$ACTUAL_INIT_SHA"; fi
    elif [[ "$entrypoint" == stat ]]; then printf '%s\n' "$ACTUAL_INIT_MODE"; fi
    ;;
  *'/velnor/job-start-hook'*)
    if [[ "$entrypoint" == sha256sum ]]; then
      if [[ "$ACTUAL_HOOK_SHA" == good ]]; then printf '%s  /velnor/job-start-hook\n' "$EXPECTED_HOOK_SHA"; else printf '%s  /velnor/job-start-hook\n' "$ACTUAL_HOOK_SHA"; fi
    elif [[ "$entrypoint" == stat ]]; then
      if [[ "$*" == *' -c %u:%g '* ]]; then printf '%s\n' "$ACTUAL_HOOK_OWNER"; else printf '%s\n' "$ACTUAL_HOOK_MODE"; fi
    fi
    ;;
  *'/usr/local/bin/velnor-job-start-hook'*)
    if [[ "$entrypoint" == sha256sum ]]; then printf '%s  /usr/local/bin/velnor-job-start-hook\n' "$EXPECTED_WRAPPER_SHA"; fi
    if [[ "$entrypoint" == stat ]]; then
      if [[ "$*" == *' -c %u:%g '* ]]; then printf '0:0\n'; else printf '555\n'; fi
    fi
    ;;
  *'/velnor/resource-probe'*)
    [[ "$entrypoint" == sha256sum ]] && printf '%s  /velnor/resource-probe\n' "$EXPECTED_PROBE_SHA"
    ;;
esac
"#;
