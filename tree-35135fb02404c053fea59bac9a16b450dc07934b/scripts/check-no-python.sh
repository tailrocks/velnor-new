#!/usr/bin/env bash
set -euo pipefail
# Lexical only: 40-line process and 80-line config windows; computed argv, aliases/functions, and unknown or implicit launchers remain unresolved.
# Git inventory bypasses Alint exclusions and includes ignored files; only untracked root/runner Cargo output is excluded.

ROOT=""
SELF_TEST=0
VIOLATIONS=0
SCAN_ERRORS=0
REPORTED=0
SCANNED=0
MAX_REPORTS=25
LAST_FINDING=""
SELF_TEST_ROOT=""
INTERPRETER='[pP][yY][tT][hH][oO][nN]([0-9]+([.][0-9]+)*)?'
VERSIONED_INTERPRETER="${INTERPRETER}(@[0-9]+([.][0-9]+)*)?"
TOKEN="(^|[^[:alnum:]_])${VERSIONED_INTERPRETER}([[:space:]]|[\"']|$)"
SHEBANG="^#!.*(^|[^[:alnum:]_])${INTERPRETER}([[:space:]]|$)"
SHELL_SHEBANG='^#!.*(^|[^[:alnum:]_])(bash|sh|zsh|fish)([[:space:]]|$)'
CONFIG_KEY='(^|[^[:alnum:]_])(run|command|cmd|cmds|exec|script|scripts|args|argv|shell|entrypoint)"?[[:space:]]*[:=]'
COMMAND_TOKEN="(^[[:space:]]*@?[[:space:]]*|[;&|()][[:space:]]*)(if[[:space:]]+|then[[:space:]]+|else[[:space:]]+|exec[[:space:]]+|command[[:space:]]+|env([[:space:]]+-S)?[[:space:]]+)*([[:alnum:]_]+=[^[:space:]]+[[:space:]]+)*(mise[[:space:]]+exec[[:space:]]+)?([\"']?)([[:alnum:]_./-]+/)?${VERSIONED_INTERPRETER}([\"']?)([[:space:]]|$)"
RUN_SCRIPT='(^[[:space:]]*|[;&|()][[:space:]]*)(uv|poetry|pipenv|pdm|rye|hatch)[[:space:]]+run[[:space:]].*[.][pP][yY]'
RUN_INTERPRETER="(^[[:space:]]*|[;&|()][[:space:]]*)(uv|poetry|pipenv|pdm|rye|hatch)[[:space:]]+run[[:space:]].*${VERSIONED_INTERPRETER}([[:space:]]|$)"
RUN_SOURCE='(uv|poetry|pipenv|pdm|rye|hatch)[[:space:]]+run[[:space:]].*[.][pP][yY]'
RUNNER='(^|[^[:alnum:]_])(uv|poetry|pipenv|pdm|rye|hatch)([^[:alnum:]_]|$)'
RUN_WORD='(^|[^[:alnum:]_])run([^[:alnum:]_]|$)'
PY_SUFFIX='[.][pP][yY]'
DOCKER_ARRAY="^[[:space:]]*\\[[[:space:]]*([\"']?)([[:alnum:]_./-]+/)?${VERSIONED_INTERPRETER}([\"']?)([,[:space:]]|$)"
RUST_API='([.]args?|(^|[^[:alnum:]_])Command::new)[[:space:]]*[(]'
RUST_START='(^|[^[:alnum:]_])Command::new[[:space:]]*[(]'
NODE_API='(^|[^[:alnum:]_])(spawn|exec|execFile|spawnSync|execSync|execFileSync)[[:space:]]*[(]'

usage() { printf 'usage: %s [--root DIR] [--self-test]\n' "${0##*/}"; }

is_py_source() {
  case "$1" in *.[pP][yY]|*.[pP][yY][wW]|*.[pP][yY][iI]|*.[pP][yY][cC]|*.[pP][yY][oO]) return 0;; *) return 1;; esac
}

is_automation_py() {
  case "$1" in scripts/*.[pP][yY]|scripts/*.[pP][yY][wW]|scripts/*.[pP][yY][iI]|scripts/*.[pP][yY][cC]|scripts/*.[pP][yY][oO]) return 0;; *) return 1;; esac
}

is_shell_name() {
  case "$1" in *.[sS][hH]|*.[bB][aA][sS][hH]|*.[zZ][sS][hH]|*.[fF][iI][sS][hH]|*/Makefile|*/GNUmakefile|*/makefile|*/justfile|Makefile|GNUmakefile|makefile|justfile) return 0;; *) return 1;; esac
}

is_text_config() {
  case "$1" in *.[yY][mM][lL]|*.[yY][aA][mM][lL]|Dockerfile|Containerfile|*.Dockerfile|*/Dockerfile|*/Containerfile|*/Dockerfile.*|*/Containerfile.*|package.json|*/package.json|package.jsonc|*/package.jsonc|package.json5|*/package.json5|tasks.json|*/tasks.json|tasks.jsonc|*/tasks.jsonc|tasks.json5|*/tasks.json5|Taskfile.json|*/Taskfile.json|Taskfile.jsonc|*/Taskfile.jsonc|Taskfile.json5|*/Taskfile.json5|taskfile.json|*/taskfile.json|taskfile.jsonc|*/taskfile.jsonc|taskfile.json5|*/taskfile.json5) return 0;; *) return 1;; esac
}

is_process_source() {
  case "$1" in *.[rR][sS]|*.[jJ][sS]|*.[mM][jJ][sS]|*.[cC][jJ][sS]|*.[tT][sS]|*.[tT][sS][xX]|*.[tT][oO][mM][lL]) return 0;; *) return 1;; esac
}

report_violation() {
  LAST_FINDING="$1: $2"; VIOLATIONS=$((VIOLATIONS+1))
  if ((REPORTED<MAX_REPORTS)); then printf 'check-no-python: %q: %s\n' "$1" "$2" >&2; REPORTED=$((REPORTED+1)); fi
}

report_error() {
  LAST_FINDING="$1: $2"; SCAN_ERRORS=$((SCAN_ERRORS+1))
  if ((REPORTED<MAX_REPORTS)); then printf 'check-no-python: scan error at %q: %s\n' "$1" "$2" >&2; REPORTED=$((REPORTED+1)); fi
}

# Return 0 on match, 1 on no match, 2 on read or scan error.
match_file() {
  local path="$1" mode="$2" regex="" reset="" output="" status=0
  case "$mode" in
    text)
      if grep -IqE -- "$COMMAND_TOKEN|$RUN_SCRIPT|$RUN_INTERPRETER" "$path" >/dev/null; then return 0; else status=$?; fi
      ((status==1)) && return 1
      return 2
      ;;
    config)
      if output="$(awk -v key="$CONFIG_KEY" -v token="$TOKEN" -v run="$RUN_SOURCE" '{if($0~/^[[:space:]]*(#|\/\/)/)next; if($0~key)context=80; if(context>0&&($0~token||$0~run)){print "hit";exit} if(context>0)context--}' "$path")"; then [[ "$output" == hit ]]; return $?; fi
      return 2
      ;;
    yaml)
      if output="$(awk -v key="$CONFIG_KEY" -v token="$TOKEN" -v command="$COMMAND_TOKEN" -v script="$RUN_SCRIPT" -v interpreter="$RUN_INTERPRETER" '{if($0~/^[[:space:]]*(#|\/\/)/)next; if($0~key){context=80;if($0~token){print "hit";exit}} if(context>0&&($0~command||$0~script||$0~interpreter)){print "hit";exit} if(context>0)context--}' "$path")"; then [[ "$output" == hit ]]; return $?; fi
      return 2
      ;;
    docker)
      if output="$(awk -v command="$COMMAND_TOKEN" -v run="$RUN_SOURCE" -v interpreter="$RUN_INTERPRETER" -v array="$DOCKER_ARRAY" -v docker='^[[:space:]]*(RUN|CMD|ENTRYPOINT)[[:space:]]+' 'function suspicious(text, clean){clean=text;sub(/^[[:space:]]*(RUN|CMD|ENTRYPOINT)[[:space:]]+/,"",clean);gsub(/\\[[:space:]]*/," ",clean);return clean~command||clean~run||clean~interpreter||clean~array} {if(active){block=block " " $0;if(suspicious(block)){print "hit";exit}if($0!~/\\[[:space:]]*$/)active=0}if(!active&&$0~docker){block=$0;if(suspicious(block)){print "hit";exit}active=($0~/\\[[:space:]]*$/)}}' "$path")"; then [[ "$output" == hit ]]; return $?; fi
      return 2
      ;;
    rust) regex="$RUST_API"; reset="$RUST_START" ;;
    node) regex="$NODE_API"; reset="$NODE_API" ;;
    *) return 2 ;;
  esac
  if output="$(awk -v api="$regex" -v reset="$reset" -v token="$TOKEN" -v runner="$RUNNER" -v run="$RUN_WORD" -v suffix="$PY_SUFFIX" '{if($0~api&&(!active||$0~reset)){active=1;age=0;window=""} if(active){window=window " " $0; if($0~token||(window~runner&&window~run&&window~suffix)){print "hit";exit} age++;if(age>40)active=0}}' "$path")"; then [[ "$output" == hit ]]; return $?; fi
  return 2
}

scan_file() {
  local path="$1" rel="$2" mode="$3" message="$4" status=0
  if match_file "$path" "$mode"; then report_violation "$rel" "$message"; else status=$?; ((status==2)) && report_error "$rel" "could not read or scan source"; fi
}

inspect() {
  local root="$1" rel="$2" file="$root/$2" first="" parent="" part="" index=0
  local -a parts=()
  case "/$rel/" in *"/../"*|*"/./"*) report_error "$rel" "invalid candidate path"; return;; esac
  case "$rel" in ""|/*) report_error "$rel" "empty or absolute candidate path"; return;; esac
  [[ -e "$file" || -L "$file" ]] || return 0
  SCANNED=$((SCANNED+1))
  IFS='/' read -r -a parts <<< "$rel"
  for ((index=0;index<${#parts[@]}-1;index++)); do
    part="${parts[index]}"; parent="${parent:+$parent/}$part"
    [[ -L "$root/$parent" ]] && { report_error "$rel" "candidate traverses a symlinked directory"; return; }
  done
  if is_automation_py "$rel"; then report_violation "$rel" "interpreter source is not allowed in the automation scripts namespace"; return; fi
  if [[ -L "$file" ]]; then
    if is_py_source "$rel" || [[ -x "$file" ]] || is_shell_name "$rel" || is_text_config "$rel" || is_process_source "$rel"; then report_violation "$rel" "active source symlink cannot be inspected safely"; fi
    return
  fi
  [[ -f "$file" ]] || return 0
  [[ -r "$file" ]] || { report_error "$rel" "candidate is not readable"; return; }
  if ! first="$(head -n 1 -- "$file")"; then report_error "$rel" "could not read candidate header"; return; fi
  if [[ "$first" =~ $SHEBANG ]]; then report_violation "$rel" "interpreter appears in a script shebang"; return; fi
  if is_py_source "$rel" && [[ -x "$file" ]]; then report_violation "$rel" "executable interpreter source is not allowed"; return; fi
  if is_shell_name "$rel" || [[ -x "$file" ]] || [[ "$first" =~ $SHELL_SHEBANG ]]; then
    scan_file "$file" "$rel" text "possible interpreter launch in active shell source"; return
  fi
  case "$rel" in
    *.[yY][mM][lL]|*.[yY][aA][mM][lL]) scan_file "$file" "$rel" yaml "possible interpreter launch in command configuration";;
    Dockerfile|Containerfile|*.Dockerfile|*/Dockerfile|*/Containerfile|*/Dockerfile.*|*/Containerfile.*) scan_file "$file" "$rel" docker "possible interpreter launch in container command";;
    *.[tT][oO][mM][lL]|package.json|*/package.json|package.jsonc|*/package.jsonc|package.json5|*/package.json5|tasks.json|*/tasks.json|tasks.jsonc|*/tasks.jsonc|tasks.json5|*/tasks.json5|Taskfile.json|*/Taskfile.json|Taskfile.jsonc|*/Taskfile.jsonc|Taskfile.json5|*/Taskfile.json5|taskfile.json|*/taskfile.json|taskfile.jsonc|*/taskfile.jsonc|taskfile.json5|*/taskfile.json5) scan_file "$file" "$rel" config "possible interpreter token in task configuration";;
    *.[rR][sS]) scan_file "$file" "$rel" rust "possible interpreter token near process-launch call";;
    *.[jJ][sS]|*.[mM][jJ][sS]|*.[cC][jJ][sS]|*.[tT][sS]|*.[tT][sS][xX]) scan_file "$file" "$rel" node "possible interpreter token near process-launch call";;
    *) ;;
  esac
}

scan_list() {
  local root="$1" list="$2" rel=""
  VIOLATIONS=0; SCAN_ERRORS=0; REPORTED=0; SCANNED=0; LAST_FINDING=""
  while IFS= read -r -d '' rel; do inspect "$root" "$rel"; done < "$list"
  ((SCAN_ERRORS>0)) && return 2
  ((VIOLATIONS==0))
}

check_tree() {
  local root="$1" list="" git_root="" result=0
  if ! git_root="$(git -C "$root" rev-parse --show-toplevel 2>/dev/null)"; then printf 'check-no-python: root is not inside a Git worktree: %q\n' "$root" >&2; return 2; fi
  [[ "$git_root" == "$root" ]] || { printf 'check-no-python: root must be the worktree root: %q\n' "$root" >&2; return 2; }
  if ! list="$(mktemp "${TMPDIR:-/tmp}/check-no-python.XXXXXX")"; then printf 'check-no-python: could not create temporary path list\n' >&2; return 2; fi
  if ! git -C "$root" ls-files -z --cached --no-directory > "$list"; then rm -f -- "$list" || true; printf 'check-no-python: could not enumerate tracked files\n' >&2; return 2; fi
  if ! git -C "$root" ls-files -z --others --exclude-standard --no-directory -- . ':(exclude,glob)target/**' ':(exclude,glob)crates/velnor-runner/target/**' >> "$list"; then rm -f -- "$list" || true; printf 'check-no-python: could not enumerate untracked candidates\n' >&2; return 2; fi
  if ! git -C "$root" ls-files -z --others --ignored --exclude-standard --no-directory -- . ':(exclude,glob)target/**' ':(exclude,glob)crates/velnor-runner/target/**' >> "$list"; then rm -f -- "$list" || true; printf 'check-no-python: could not enumerate Git-ignored candidates\n' >&2; return 2; fi
  if scan_list "$root" "$list"; then result=0; else result=$?; fi
  if ! rm -f -- "$list"; then report_error "$list" "could not remove temporary path list"; return 2; fi
  return "$result"
}

expect_clean() {
  if check_tree "$2" >/dev/null 2>&1; then printf 'check-no-python: self-test accepted %s\n' "$1"; return 0; fi
  printf 'check-no-python: self-test rejected clean case %s\n' "$1" >&2; return 1
}

expect_rejected() {
  if check_tree "$2" >/dev/null 2>&1; then printf 'check-no-python: self-test failed to reject %s\n' "$1" >&2; return 1; fi
  if ((SCAN_ERRORS||!VIOLATIONS)) || [[ "$LAST_FINDING" != *"$3"* ]]; then printf 'check-no-python: self-test lacked expected finding for %s\n' "$1" >&2; return 1; fi
  printf 'check-no-python: self-test rejected %s through Git enumeration\n' "$1"
}

cleanup_test() {
  case "$SELF_TEST_ROOT" in "${TMPDIR:-/tmp}"/check-no-python-test.*) rm -rf -- "$SELF_TEST_ROOT";; esac
}

self_test() {
  local tree="$SELF_TEST_ROOT/repo" runner="py" listed="" ignored_status=0
  runner+="thon3"
  mkdir -p "$tree/fixtures/alint-ignored" "$tree/fixtures/git-ignored" "$tree/fixtures/target" "$tree/crates/oci/tests" "$tree/scripts" "$tree/bin" "$tree/nested" "$tree/images/build" "$tree/.github/workflows" || return 1
  git init -q -- "$tree" || { printf 'check-no-python: could not initialize temporary Git fixture\n' >&2; return 1; }
  printf 'ignore:\n  - "fixtures/**"\n' > "$tree/.alint.yml"
  printf 'fixtures/git-ignored/**\n' > "$tree/.gitignore"
  printf 'opaque inert fixture bytes\n' > "$tree/crates/oci/tests/static_input.py"
  git -C "$tree" add -- .alint.yml .gitignore crates/oci/tests/static_input.py || return 1
  expect_clean "passive source outside the automation namespace" "$tree" || return 1
  printf '# example only: %s -c :\n' "$runner" > "$tree/scripts/comment.sh"
  printf '{"devDependencies":{"%s":"3"}}\n' "$runner" > "$tree/package.json"
  printf 'RUN echo %s\n' "$runner" > "$tree/images/build/Dockerfile"
  expect_clean "comments, dependency data, and echoed token" "$tree" || return 1
  rm -f -- "$tree/scripts/comment.sh" "$tree/package.json" "$tree/images/build/Dockerfile"
  printf 'exec %s -c :\n' "$runner" > "$tree/fixtures/target/launch.sh"
  git -C "$tree" add -- fixtures/target/launch.sh || return 1
  expect_rejected "tracked source below a target-named fixture directory" "$tree" "fixtures/target/launch.sh" || return 1
  rm -f -- "$tree/fixtures/target/launch.sh"

  printf '#!/usr/bin/env %s\n' "$runner" > "$tree/fixtures/alint-ignored/untracked.py"
  if git -C "$tree" check-ignore -q -- fixtures/alint-ignored/untracked.py; then printf 'check-no-python: fixture unexpectedly Git-ignored\n' >&2; return 1; else ignored_status=$?; fi
  ((ignored_status==1)) || { printf 'check-no-python: Git ignore query failed\n' >&2; return 1; }
  if ! listed="$(git -C "$tree" ls-files --others --exclude-standard --no-directory -- fixtures/alint-ignored/untracked.py)" || [[ "$listed" != fixtures/alint-ignored/untracked.py ]]; then printf 'check-no-python: Git enumeration missed Alint-ignored candidate\n' >&2; return 1; fi
  expect_rejected "untracked Alint-ignored script shebang" "$tree" "untracked.py" || return 1
  rm -f -- "$tree/fixtures/alint-ignored/untracked.py"
  printf 'exec %s -c :\n' "$runner" > "$tree/fixtures/git-ignored/launch.sh"
  if ! git -C "$tree" check-ignore -q -- fixtures/git-ignored/launch.sh; then printf 'check-no-python: Git-ignored fixture was not ignored\n' >&2; return 1; fi
  if ! listed="$(git -C "$tree" ls-files --others --ignored --exclude-standard --no-directory -- fixtures/git-ignored/launch.sh)" || [[ "$listed" != fixtures/git-ignored/launch.sh ]]; then printf 'check-no-python: Git enumeration missed ignored candidate\n' >&2; return 1; fi
  expect_rejected "Git-ignored fixture launcher" "$tree" "launch.sh" || return 1
  rm -f -- "$tree/fixtures/git-ignored/launch.sh"

  : > "$tree/scripts/future.py"
  expect_rejected "new source in automation namespace" "$tree" "future.py" || return 1
  rm -f -- "$tree/scripts/future.py"
  printf 'exec %s -c :\n' "$runner" > "$tree/scripts/inline.sh"
  expect_rejected "inline interpreter launch" "$tree" "inline.sh" || return 1
  rm -f -- "$tree/scripts/inline.sh"
  printf '\t@%s -c :\n' "$runner" > "$tree/nested/Makefile"
  expect_rejected "nested Makefile recipe" "$tree" "Makefile" || return 1
  rm -f -- "$tree/nested/Makefile"
  printf 'FOO=bar %s -c :\n' "$runner" > "$tree/scripts/assignment.sh"
  expect_rejected "shell assignment launch" "$tree" "assignment.sh" || return 1
  rm -f -- "$tree/scripts/assignment.sh"
  printf 'RUN \\\n  %s -c :\n' "$runner" > "$tree/images/build/Dockerfile.dev"
  expect_rejected "continued nested Dockerfile command" "$tree" "Dockerfile.dev" || return 1
  rm -f -- "$tree/images/build/Dockerfile.dev"
  printf 'jobs:\n  test:\n    steps:\n      - run: |\n          %s -c :\n' "$runner" > "$tree/.github/workflows/test.yml"
  expect_rejected "workflow run command" "$tree" "test.yml" || return 1
  rm -f -- "$tree/.github/workflows/test.yml"
  printf '#!/usr/bin/env bash\nexec %s -I -S -c :\n' "$runner" > "$tree/bin/extensionless"
  expect_rejected "non-executable env shell entrypoint" "$tree" "extensionless" || return 1
  rm -f -- "$tree/bin/extensionless"
  printf 'mise exec %s@3 -- -c :\n' "${runner%3}" > "$tree/scripts/mise.sh"
  expect_rejected "mise interpreter version launch" "$tree" "mise.sh" || return 1
  rm -f -- "$tree/scripts/mise.sh"
  printf 'uv run build_task.py\n' > "$tree/scripts/uv.sh"
  expect_rejected "uv script launch" "$tree" "uv.sh" || return 1
  rm -f -- "$tree/scripts/uv.sh"
  printf 'Command::new(\n  "%s"\n);\n' "$runner" > "$tree/crates/oci/tests/launch.rs"
  expect_rejected "multiline Rust process launch" "$tree" "launch.rs" || return 1
  rm -f -- "$tree/crates/oci/tests/launch.rs"
  printf 'let mut command = Command::new("uv");\ncommand.arg("run");\ncommand.arg("task.py");\n' > "$tree/crates/oci/tests/uv_chain.rs"
  expect_rejected "chained Rust wrapper args" "$tree" "uv_chain.rs" || return 1
  rm -f -- "$tree/crates/oci/tests/uv_chain.rs"
  : > "$tree/crates/oci/tests/source.txt"
  ln -s source.txt "$tree/crates/oci/tests/launch.rs"
  expect_rejected "process-source symlink" "$tree" "launch.rs" || return 1
  rm -f -- "$tree/crates/oci/tests/launch.rs" "$tree/crates/oci/tests/source.txt"
  printf 'command.arg("%s");\n' "$runner" > "$tree/crates/oci/tests/arg.rs"
  expect_rejected "Rust receiver arg launch" "$tree" "arg.rs" || return 1
  rm -f -- "$tree/crates/oci/tests/arg.rs"
  printf 'execFileSync(\n  "%s",\n  ["-c", ":"]\n);\n' "$runner" > "$tree/scripts/launch.mjs"
  expect_rejected "multiline Node process launch" "$tree" "launch.mjs" || return 1
  rm -f -- "$tree/scripts/launch.mjs"
  printf '{"scripts":{"test":"%s -c :"}}\n' "$runner" > "$tree/package.json"
  expect_rejected "package task command" "$tree" "package.json" || return 1
  rm -f -- "$tree/package.json"
  printf '[tasks.check]\nrun = "%s -c :"\n' "$runner" > "$tree/Taskfile.toml"
  expect_rejected "TOML task command" "$tree" "Taskfile.toml" || return 1
  printf 'check-no-python: self-tests passed\n'
}

main() {
  local script_dir="" status=0
  while (($#)); do
    case "$1" in
      --root) (($#>=2)) || { usage >&2; return 2; }; ROOT="$2"; shift 2;;
      --self-test) SELF_TEST=1; shift;;
      -h|--help) usage; return 0;;
      *) printf 'check-no-python: unknown argument: %q\n' "$1" >&2; usage >&2; return 2;;
    esac
  done
  if ((SELF_TEST)); then
    SELF_TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/check-no-python-test.XXXXXX")" || return 1
    trap cleanup_test EXIT
    self_test; return $?
  fi
  if [[ -z "$ROOT" ]]; then script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)" || return 2; ROOT="$(cd "$script_dir/.." && pwd -P)" || return 2
  else ROOT="$(cd "$ROOT" && pwd -P)" || { printf 'check-no-python: invalid root directory\n' >&2; return 2; }; fi
  if check_tree "$ROOT"; then printf 'check-no-python: inspected %d tracked, untracked, and Git-ignored candidates outside untracked root/runner Cargo output; no findings\n' "$SCANNED"; return 0; else status=$?; fi
  printf 'check-no-python: %d policy finding(s), %d scan error(s) in %d tracked, untracked, and Git-ignored candidates outside untracked root/runner Cargo output' "$VIOLATIONS" "$SCAN_ERRORS" "$SCANNED" >&2
  ((VIOLATIONS+SCAN_ERRORS<=MAX_REPORTS)) || printf ' (first %d shown)' "$MAX_REPORTS" >&2
  printf '\n' >&2; return "$status"
}

main "$@"
