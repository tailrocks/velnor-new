pub(super) const PROCESS_GROUP_CONTROL: &str = r#"set -m
mbx_pid=; mbx_pgid=; mbx_launching=false; mbx_group_captured=false; mbx_child_waited=false; mbx_status=0
mbx_quiesced_emitted=false; cleanup_safe_emitted=false
group_has_live_processes() {
    target_group="$1"
    process_list=$(ps -A -o pid= -o pgid= -o stat=) || return 2
    if awk -v target="$target_group" '$2 == target && $3 !~ /^Z/ { found=1 } END { exit !found }' <<<"$process_list"; then
        return 0
    else
        scan_status=$?
        [ "$scan_status" -eq 1 ] && return 1
        return 2
    fi
}
wait_for_mbx_group() {
    target_pid="$1"; target_group="$2"
    case "$target_group" in ''|*[!0-9]*) return 1 ;; esac
    [ "$target_group" -gt 1 ] || return 1
    kill -TERM -- "-$target_group" 2>/dev/null || :
    attempt=0
    while [ "$attempt" -lt 5 ]; do
        if group_has_live_processes "$target_group"; then sleep 1; attempt=$((attempt + 1))
        else scan_status=$?; [ "$scan_status" -eq 1 ] || return 1; break
        fi
    done
    if group_has_live_processes "$target_group"; then
        kill -KILL -- "-$target_group" 2>/dev/null || :
        attempt=0
        while [ "$attempt" -lt 5 ]; do
            if group_has_live_processes "$target_group"; then sleep 1; attempt=$((attempt + 1))
            else scan_status=$?; [ "$scan_status" -eq 1 ] || return 1; break
            fi
        done
    fi
    if group_has_live_processes "$target_group"; then return 1
    else scan_status=$?; [ "$scan_status" -eq 1 ] || return 1
    fi
    wait "$target_pid" 2>/dev/null || :
}
capture_mbx_group() {
    case "$mbx_pid" in ''|*[!0-9]*) return 1 ;; esac
    [ "$mbx_pid" -gt 1 ] || return 1
    mbx_pgid="$mbx_pid"
    process_list=$(ps -A -o pid= -o pgid= -o stat=) || return 1
    if awk -v child="$mbx_pid" -v group="$mbx_pgid" '
        $1 == child { direct=1; if ($2 != group) mismatch=1 }
        $2 == group && $3 !~ /^Z/ { active=1 }
        END { if (mismatch) exit 3; if (direct || active) exit 0; exit 1 }
    ' <<<"$process_list"; then
        :
    else
        scan_status=$?
        [ "$scan_status" -eq 1 ] || return 1
    fi
    mbx_group_captured=true
}
start_mbx() {
    mbx_launching=true
    mbx "$@" & mbx_pid=$!
    mbx_pgid="$mbx_pid"
    capture_mbx_group || fail 'cannot validate MBX process group'
    mbx_launching=false
}
wait_mbx() {
    if wait "$mbx_pid"; then mbx_status=0; else mbx_status=$?; fi
    mbx_child_waited=true
    [ "$mbx_group_captured" = true ] && [ "$mbx_pgid" = "$mbx_pid" ] || fail 'MBX process group was not captured'
    if group_has_live_processes "$mbx_pgid"; then
        wait_for_mbx_group "$mbx_pid" "$mbx_pgid" || fail 'cannot quiesce MBX process group'
        mbx_status=125
    else
        scan_status=$?
        [ "$scan_status" -eq 1 ] || fail 'cannot verify MBX process-group quiescence'
    fi
    mbx_pid=; mbx_pgid=; mbx_launching=false; mbx_group_captured=false; mbx_child_waited=false
    return "$mbx_status"
}
prepare_mbx_signal() {
    if [ "$mbx_launching" = true ] && [ -z "$mbx_pid" ]; then
        mbx_pid="${!:-}"; mbx_pgid="$mbx_pid"
    fi
    if [ -n "$mbx_pid" ] && [ -z "$mbx_pgid" ]; then mbx_pgid="$mbx_pid"; fi
}
stop_mbx_group() {
    if [ "$mbx_launching" = true ] && [ -z "$mbx_pid" ]; then return 1; fi
    if [ -n "$mbx_pid" ]; then
        [ "$mbx_group_captured" = true ] && [ -n "$mbx_pgid" ] && [ "$mbx_pgid" = "$mbx_pid" ] || return 1
        wait_for_mbx_group "$mbx_pid" "$mbx_pgid" || return 1
    fi
    mbx_pid=; mbx_pgid=; mbx_launching=false; mbx_group_captured=false; mbx_child_waited=false
}
stop_unverified_mbx_child() {
    target_pid="$1"
    case "$target_pid" in ''|*[!0-9]*) return 1 ;; esac
    kill -TERM "$target_pid" 2>/dev/null || :
    attempt=0
    while [ "$attempt" -lt 5 ] && kill -0 "$target_pid" 2>/dev/null; do sleep 1; attempt=$((attempt + 1)); done
    if kill -0 "$target_pid" 2>/dev/null; then kill -KILL "$target_pid" 2>/dev/null || :; fi
    wait "$target_pid" 2>/dev/null || :
}
mark_mbx_quiesced() {
    if [ "$mbx_quiesced_emitted" != true ]; then
        printf 'mbx_quiesced=true\n' >> "$GITHUB_OUTPUT" || return 1
        mbx_quiesced_emitted=true
    fi
}
mark_cleanup_safe() {
    if [ "$cleanup_safe_emitted" != true ]; then
        printf 'cleanup_safe=true\n' >> "$GITHUB_OUTPUT" || return 1
        cleanup_safe_emitted=true
    fi
}
on_mbx_exit() {
    exit_status=$?
    trap - EXIT
    safe_to_report=true
    prepare_mbx_signal
    if [ -n "$mbx_pid" ]; then
        if [ "$mbx_group_captured" = true ] && stop_mbx_group; then :
        elif [ "$mbx_child_waited" != true ]; then
            stop_unverified_mbx_child "$mbx_pid"; safe_to_report=false; exit_status=1
        else safe_to_report=false; exit_status=1
        fi
    elif [ "$mbx_launching" = true ]; then
        safe_to_report=false; exit_status=1
    fi
    if [ "$safe_to_report" = true ]; then
        if [ "$mbx_operation" = import ]; then mark_mbx_quiesced || exit_status=1
        elif [ "$mbx_operation" = export ]; then
            capture_bundle_identity || :
            mark_cleanup_safe || exit_status=1
        fi
    fi
    exit "$exit_status"
}
run_mbx() { start_mbx "$@"; wait_mbx; }"#;

pub(super) fn append_process_group_control(lines: &mut Vec<String>) {
    lines.push(PROCESS_GROUP_CONTROL.to_owned());
}
