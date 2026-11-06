// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) const GH: &str = r#"#!/bin/sh
d="${FAKE_GH_DIR:?}"
[ -n "${GH_TOKEN-}" ] || : > "$d/token-missing"
env | while IFS= read -r entry; do
  name=${entry%%=*}
  case "$name" in
    VELNOR_STATE_SELECT_SUPERVISOR_HEX|VELNOR_STATE_SELECT_UNIT_COUNT|VELNOR_STATE_SELECT_UNIT_R*|VELNOR_STATE_COMPAT_R*|VELNOR_STATE_SNAPSHOT_R*|ordinals|i|seen|line|record_name|record|prefix|rest|attested|produce|unit_name|root|slug|remaining|character|compat_name|snapshot_name|compat|snapshot|result|extra|ordinal|state_prefix|IFS)
      if command -p printenv "$name" >/dev/null 2>&1; then : > "$d/supervisor-payload-leaked"; fi ;;
  esac
done
mode=ok
[ -f "$d/mode" ] && read -r mode < "$d/mode"
case "$mode" in
  fail) exit 1 ;;
  hang) sleep 30; exit 0 ;;
esac
printf '%s\n' + >> "$d/activity"
tracked=1
finish() { [ "$tracked" -eq 1 ] || return 0; printf '%s\n' - >> "$d/activity"; tracked=0; }
trap finish EXIT
trap 'if [ -n "${FAKE_GH_SIGNAL_MARKER-}" ]; then : > "$FAKE_GH_SIGNAL_MARKER"; fi; if [ -n "${FAKE_GH_SIGNAL_ACK-}" ]; then while [ ! -f "$FAKE_GH_SIGNAL_ACK" ]; do sleep 0.01; done; fi; finish; exit 143' TERM INT HUP
verb="$1 $2"
shift 2
case "$verb" in
  "cache list")
    tab=$(printf '\t')
    key=; ref=; fields=; repo=${GH_REPO-}; sortf=last_accessed_at; order=desc; limit=30
    while [ $# -gt 0 ]; do
      case "$1" in --key) key=$2; shift ;; --ref) ref=$2; shift ;; --json) fields=$2; shift ;; --jq) shift ;; -R) repo=$2; shift ;; --sort) sortf=$2; shift ;; --order) order=$2; shift ;; --limit) limit=$2; shift ;; esac
      shift
    done
    [ -n "$repo" ] || exit 1
    if [ "$mode" = list-hang ]; then printf '%s\n' "$$" > "$d/list-pid"; sleep 30; exit 0; fi
    if [ -f "$d/list-output-override" ]; then cat "$d/list-output-override"; exit 0; fi
    if [ -f "$d/lag" ]; then
      read -r lag < "$d/lag"
      if [ "$lag" -gt 0 ]; then printf '%s\n' "$((lag - 1))" > "$d/lag"; exit 0; fi
    fi
    if [ "$mode" = garbage ]; then printf '%s\n' '<<<not json>>>' '{"oops"'; exit 0; fi
    sflag=-r
    [ "$order" = asc ] && sflag=
    while IFS="$tab" read -r created lref lkey lid lsize accessed; do
      [ "$lref" = "$ref" ] || continue
      case "$lkey" in "$key"*) ;; *) continue ;; esac
      [ -n "$accessed" ] || accessed=$created
      case "$sortf" in created_at) sk=$created ;; *) sk=$accessed ;; esac
      printf '%s\t%s\t%s\t%s\t%s\n' "$sk" "$created" "$lkey" "$lid" "$lsize"
    done < "$d/list" | LC_ALL=C sort $sflag | head -n "$limit" | while IFS="$tab" read -r sk created lkey lid lsize; do
      case "$fields" in
        *createdAt*) printf '%s\t%s\t%s\t%s\n' "$created" "$lkey" "$lid" "$lsize" ;;
        *) printf '%s\t%s\t%s\n' "$lkey" "$lid" "$lsize" ;;
      esac
    done
    ;;
  "cache delete")
    id=; repo=${GH_REPO-}
    while [ $# -gt 0 ]; do
      case "$1" in -R) repo=$2; shift ;; *) id=$1 ;; esac
      shift
    done
    [ -n "$repo" ] || exit 1
    if [ -f "$d/refuse" ] && grep -qx "$id" "$d/refuse"; then exit 1; fi
    tab=$(printf '\t')
    awk -F "$tab" -v id="$id" '$4 != id' "$d/list" > "$d/list.next" || :
    mv "$d/list.next" "$d/list"
    printf 'delete cache %s\n' "$id" >> "$d/log"
    ;;
  "api repos/${GITHUB_REPOSITORY:?}/actions/cache/usage")
    [ "$mode" = usage_fail ] && exit 1
    read -r usage < "$d/usage"
    printf '%s\n' "$usage"
    ;;
  "api repos/${GITHUB_REPOSITORY:?}/git/ref/"*)
    [ "$mode" = tip_fail ] && exit 1
    read -r tip < "$d/tip" || exit 1
    printf '%s\n' "$tip"
    ;;
  "api -X")
    [ "$1" = DELETE ] || exit 2
    case "$2" in "repos/${GITHUB_REPOSITORY:?}/actions/artifacts/"*) ;; *) exit 1 ;; esac
    id=${2##*/}
    grep -qx "$id" "$d/artifacts" || exit 1
    grep -vx "$id" "$d/artifacts" > "$d/artifacts.next" || :
    mv "$d/artifacts.next" "$d/artifacts"
    printf 'delete artifact %s\n' "$id" >> "$d/log"
    ;;
  "attestation verify")
    case "$mode" in
      verify-fail) exit 1 ;;
      verify-hang) sleep 30; exit 0 ;;
      verify-garbage) printf '%s\n' '<<<not a verdict>>>' '{"oops"'; exit 3 ;;
    esac
    file=$1
    shift
    read -r skey sid ssize < "$file"
    cat "$file" >> "$d/subjects"
    printf '%s\n' "$$" >> "$d/verify-pids"
    if [ -f "$d/rendezvous" ]; then
      read -r want < "$d/rendezvous"
      slot=1
      while [ "$slot" -le "$want" ]; do
        if mkdir "$d/rv/$slot" 2>/dev/null; then
          waited=0
          while [ "$(ls "$d/rv" | wc -l | tr -d ' ')" -lt "$want" ] && [ "$waited" -lt 100 ]; do sleep 0.1; waited=$((waited + 1)); done
          break
        fi
        slot=$((slot + 1))
      done
    fi
    if [ -f "$d/hang-keys" ] && grep -qxF "$skey" "$d/hang-keys"; then
      (
        trap '' TERM
        (trap '' TERM; while :; do sleep 1; done) &
        printf '%s\n' "$!" >> "$d/hung-pids"
        while :; do sleep 1; done
      ) &
      child=$!
      printf '%s\n' "$child" >> "$d/hung-pids"
      wait "$child"
      exit 1
    fi
    if [ -f "$d/fail-keys" ] && grep -qxF "$skey" "$d/fail-keys"; then
      : > "$d/failed-$skey"
      exit 124
    fi
    if [ -n "${FAKE_GH_VERIFY_GATE-}" ] && [ "$skey" = "${FAKE_GH_VERIFY_GATE_KEY-}" ]; then
      : > "${FAKE_GH_VERIFY_STARTED:?}"
      while [ ! -e "$FAKE_GH_VERIFY_GATE" ]; do sleep 0.01; done
    fi
    if [ "${FAKE_GH_CLOCK-}" = 1 ]; then
      clock_id=$(printf '%s' "$skey" | sha256sum)
      clock_id=${clock_id%% *}
      marker="$d/clock-marker-$clock_id"
      ack="$d/clock-ack-$clock_id"
      printf '%s\n' "${FAKE_GH_CLOCK_STEP:?}" > "$marker.next"
      mv -f "$marker.next" "$marker"
      waited=0
      while [ ! -f "$ack" ] && [ "$waited" -lt 100 ]; do sleep 0.01; waited=$((waited + 1)); done
      [ -f "$ack" ] || exit 1
    fi
    repo=; signer=; sref=; pred=
    while [ $# -gt 0 ]; do
      case "$1" in -R) repo=$2; shift ;; --signer-workflow) signer=$2; shift ;; --source-ref) sref=$2; shift ;; --predicate-type) pred=$2; shift ;; esac
      shift
    done
    [ -n "$repo" ] || exit 1
    sum=$(sha256sum "$file")
    sum=${sum%% *}
    while read -r tdigest trepo tsigner tref tpred; do
      [ "$tdigest" = "$sum" ] || continue
      [ "$repo" = "$trepo" ] || continue
      [ -z "$signer" ] || [ "$signer" = "$tsigner" ] || continue
      [ -z "$sref" ] || [ "$sref" = "$tref" ] || continue
      [ -z "$pred" ] || [ "$pred" = "$tpred" ] || continue
      printf '%s\n' "$skey" >> "$d/completed"
      pgid=$(ps -o pgid= -p "$$" | tr -d ' ')
      printf '%s %s %s\n' "$skey" "$$" "$pgid" >> "$d/verified"
      if [ -f "$d/hung-pids" ]; then
        while IFS= read -r pid; do
          if kill -0 "$pid" 2>/dev/null; then : > "$d/healthy-while-hung"; break; fi
        done < "$d/hung-pids"
      fi
      exit 0
    done < "$d/attest"
    exit 1
    ;;
  *) exit 2 ;;
esac
"#;
