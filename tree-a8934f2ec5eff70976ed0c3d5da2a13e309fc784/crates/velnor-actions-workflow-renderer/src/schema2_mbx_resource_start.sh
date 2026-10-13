set -euo pipefail
umask 077
test "$GITHUB_EVENT_NAME" = workflow_dispatch
test "$GITHUB_REF" = refs/heads/main
test "$GITHUB_REF_PROTECTED" = true
evidence="$RUNNER_TEMP/mbx-cache-evidence"
runner_temp="$(realpath -e -- "$RUNNER_TEMP")"
test "$runner_temp" = "$RUNNER_TEMP"
test ! -e "$evidence"
test ! -L "$evidence"
mkdir -m 700 "$evidence"
test "$(realpath -e -- "$evidence")" = "$evidence"
test "$(stat -c '%u:%g:%a' -- "$evidence")" = "$(id -u):$(id -g):700"
(
  set -o noclobber
  printf 'mbx-cache-evidence-v1\t%s\t%s\t%s\n' \
    "$GITHUB_RUN_ID" "$GITHUB_RUN_ATTEMPT" "$MBX_QUALIFICATION_JOB_ID" > "$evidence/private.marker"
)
chmod 600 "$evidence/private.marker"
test ! -e "$evidence/private.identity"
test ! -L "$evidence/private.identity"
(
  set -o noclobber
  stat -c '%d:%i:%u:%g' -- "$evidence" > "$evidence/private.identity"
)
chmod 600 "$evidence/private.identity"
test "$MBX_QUALIFICATION_PHASE_FILE" = "$evidence/phases.tsv"
test ! -e "$evidence/phases.tsv"
test ! -L "$evidence/phases.tsv"
(
  set -o noclobber
  : > "$evidence/phases.tsv"
)
chmod 600 "$evidence/phases.tsv"
(
  set -o noclobber
  printf 'snapshot\tname\tpath_percent_escaped\tdevice\tinode\n' > "$evidence/root-registry.tsv"
  : > "$evidence/inventory-errors.txt"
)
test ! -e "$evidence/sampler.tmp"
test ! -L "$evidence/sampler.tmp"
(
  set -o noclobber
  printf '%s' '__SAMPLER_BASE64__' | base64 --decode > "$evidence/sampler.tmp"
)
printf '%s  %s\n' '__SAMPLER_SHA256__' "$evidence/sampler.tmp" | sha256sum --check --status
chmod 700 "$evidence/sampler.tmp"
mv -T -- "$evidence/sampler.tmp" "$evidence/sampler.sh"
test ! -e "$evidence/path-validation.tmp"
test ! -L "$evidence/path-validation.tmp"
(
  set -o noclobber
  printf '%s' '__PATH_VALIDATION_BASE64__' | base64 --decode > "$evidence/path-validation.tmp"
)
printf '%s  %s\n' '__PATH_VALIDATION_SHA256__' "$evidence/path-validation.tmp" | sha256sum --check --status
chmod 700 "$evidence/path-validation.tmp"
mv -T -- "$evidence/path-validation.tmp" "$evidence/path-validation.sh"
bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate
test ! -e "$evidence/sampler.log"
test ! -L "$evidence/sampler.log"
(
  set -o noclobber
  : > "$evidence/sampler.log"
)
count=0
setsid --fork bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" sample >> "$evidence/sampler.log" 2>&1 < /dev/null
while { [ ! -s "$evidence/sampler.pid" ] || [ ! -s "$evidence/runner-metadata.json" ]; } && [ "$count" -lt 10 ]; do
  if [ -s "$evidence/sampler.exit" ]; then break; fi
  sleep 1
  count=$((count + 1))
done
test -s "$evidence/sampler.pid"
IFS= read -r sampler_pid < "$evidence/sampler.pid"
[[ "$sampler_pid" =~ ^[0-9]+$ ]]
test -s "$evidence/runner-metadata.json"
kill -0 "$sampler_pid"
proc_identity() {
  local row remainder
  local -a fields=()
  IFS= read -r row < "/proc/$1/stat"
  remainder="${row##*) }"
  read -r -a fields <<< "$remainder"
  ((${#fields[@]} > 19)) || return 1
  printf '%s\t%s\t%s\n' "${fields[2]}" "${fields[3]}" "${fields[19]}"
}
identity="$(proc_identity "$sampler_pid")"
IFS=$'\t' read -r sampler_pgid sampler_sid sampler_start_ticks <<< "$identity"
[[ "$sampler_pgid" == "$sampler_pid" && "$sampler_sid" == "$sampler_pid" && "$sampler_start_ticks" =~ ^[0-9]+$ ]]
case "$(ps -p "$sampler_pid" -o args=)" in *"$evidence/sampler.sh"*) ;; *) echo 'sampler pid does not identify private sampler' >&2; exit 1 ;; esac
test ! -e "$evidence/sampler.session.tsv"
test ! -L "$evidence/sampler.session.tsv"
(
  set -o noclobber
  printf 'pid\t%s\npgid\t%s\nsid\t%s\nstart_ticks\t%s\nrun_id\t%s\nrun_attempt\t%s\njob_id\t%s\nuid\t%s\ngid\t%s\nevidence_identity\t%s\n' \
    "$sampler_pid" "$sampler_pgid" "$sampler_sid" "$sampler_start_ticks" \
    "$GITHUB_RUN_ID" "$GITHUB_RUN_ATTEMPT" "$MBX_QUALIFICATION_JOB_ID" "$(id -u)" "$(id -g)" \
    "$(cat -- "$evidence/private.identity")" > "$evidence/sampler.session.tsv"
)
chmod 600 "$evidence/sampler.session.tsv"
printf 'sampler_started_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" > "$evidence/sampler-state.txt"
