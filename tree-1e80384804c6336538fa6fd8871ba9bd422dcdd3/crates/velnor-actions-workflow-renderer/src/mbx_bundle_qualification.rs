//! Qualification-only MBX phase and raw-command receipts.

const EXPORT_OBSERVER: &str = concat!(
    "set -eu; set -o pipefail; ",
    "new_temp() { marker=\"$GITHUB_OUTPUT.mbx-temp-path\"; if [ -e \"$marker\" ] || [ -L \"$marker\" ]; then echo \"MBX temp marker already exists\" >&2; return 1; fi; if ! mktemp \"$1\" > \"$marker\"; then return 1; fi; IFS= read -r temp_path < \"$marker\" || return 1; rm -f \"$marker\" || return 1; test -n \"$temp_path\"; }; ",
    "phase() { if [ -n \"${MBX_QUALIFICATION_PHASE_FILE:-}\" ]; then new_temp \"$RUNNER_TEMP/velnor-mbx-phase-time.XXXXXXXXXX\" || return 1; timestamp_file=\"$temp_path\"; if ! date -u +'%Y-%m-%dT%H:%M:%S.%NZ' > \"$timestamp_file\"; then rm -f \"$timestamp_file\"; return 1; fi; IFS= read -r timestamp < \"$timestamp_file\" || return 1; rm -f \"$timestamp_file\" || return 1; printf '%s\\t%s\\n' \"$timestamp\" \"$1\" >> \"$MBX_QUALIFICATION_PHASE_FILE\" || return 1; fi; return 0; }; ",
    "stream() { count_file=\"$GITHUB_OUTPUT.mbx-byte-count\"; if [ -e \"$count_file\" ] || [ -L \"$count_file\" ]; then echo \"MBX byte-count marker already exists\" >&2; return 1; fi; wc -c < \"$2\" > \"$count_file\" || return 1; read -r bytes < \"$count_file\" || return 1; rm -f \"$count_file\" || return 1; printf '%s_bytes=%s\\n' \"$1\" \"$bytes\"; cat \"$2\" || return 1; }; ",
    "record() { if [ -n \"${MBX_QUALIFICATION_EXPORT_RECEIPT:-}\" ]; then printf 'command=%s\\n' \"$1\" >> \"$MBX_QUALIFICATION_EXPORT_RECEIPT\" || return 1; stream stdout \"$2\" >> \"$MBX_QUALIFICATION_EXPORT_RECEIPT\" || return 1; stream stderr \"$3\" >> \"$MBX_QUALIFICATION_EXPORT_RECEIPT\" || return 1; printf '\\nexit_status=%s\\n' \"$4\" >> \"$MBX_QUALIFICATION_EXPORT_RECEIPT\" || return 1; fi; return 0; }; ",
    "if [ -n \"${MBX_QUALIFICATION_EXPORT_RECEIPT:-}\" ]; then if [ -e \"$MBX_QUALIFICATION_EXPORT_RECEIPT\" ] || [ -L \"$MBX_QUALIFICATION_EXPORT_RECEIPT\" ]; then echo \"MBX export receipt already exists\" >&2; exit 1; fi; : > \"$MBX_QUALIFICATION_EXPORT_RECEIPT\"; fi; ",
    "snapshot() { label=\"$1\"; if ! new_temp \"$RUNNER_TEMP/velnor-mbx-snapshot.stdout.XXXXXXXXXX\"; then echo \"MBX sampler stdout capture could not be created\" >&2; return 0; fi; stdout=\"$temp_path\"; if ! new_temp \"$RUNNER_TEMP/velnor-mbx-snapshot.stderr.XXXXXXXXXX\"; then echo \"MBX sampler stderr capture could not be created\" >&2; if ! rm -f \"$stdout\"; then echo \"MBX sampler temp cleanup failed\" >&2; fi; return 0; fi; stderr=\"$temp_path\"; if bash \"$RUNNER_TEMP/mbx-cache-evidence/sampler.sh\" \"$RUNNER_TEMP/mbx-cache-evidence\" \"$RUNNER_TEMP\" \"$GITHUB_ENV\" \"${MBX_QUALIFICATION_SAMPLE_INTERVAL:-5}\" snapshot \"$label\" > \"$stdout\" 2> \"$stderr\"; then sample_status=0; else sample_status=$?; fi; if ! record \"snapshot-$label\" \"$stdout\" \"$stderr\" \"$sample_status\"; then echo \"MBX sampler receipt could not be recorded\" >&2; fi; if ! rm -f \"$stdout\" \"$stderr\"; then echo \"MBX sampler temp cleanup failed\" >&2; fi; return 0; }; ",
    "capture() { kind=\"$1\"; shift; stdout=\"$GITHUB_OUTPUT.mbx-$kind-stdout\"; stderr=\"$GITHUB_OUTPUT.mbx-$kind-stderr\"; log=\"$GITHUB_OUTPUT.mbx-$kind\"; for file in \"$stdout\" \"$stderr\"; do if [ -e \"$file\" ] || [ -L \"$file\" ]; then echo \"MBX export marker already exists\" >&2; return 1; fi; done; if command mbx \"$@\" > \"$stdout\" 2> \"$stderr\"; then status=0; else status=$?; fi; cat \"$stdout\" > \"$log\" || return 1; cat \"$stderr\" >> \"$log\" || return 1; cat \"$stdout\" || return 1; cat \"$stderr\" >&2 || return 1; record \"$kind\" \"$stdout\" \"$stderr\" \"$status\" || return 1; return \"$status\"; }; ",
    "mbx() { if [ \"$1\" = \"cache\" ] && [ \"$2\" = \"export\" ]; then phase export-start || return 1; if capture export \"$@\"; then command_status=0; else command_status=$?; fi; printf 'export_status=%s\\n' \"$command_status\" >> \"$GITHUB_OUTPUT\" || return 1; phase export-end || return 1; if [ \"$command_status\" -eq 0 ]; then snapshot export-complete; fi; return \"$command_status\"; elif [ \"$1\" = \"gc\" ]; then phase gc-start || return 1; if capture gc \"$@\"; then command_status=0; else command_status=$?; fi; printf 'gc_status=%s\\n' \"$command_status\" >> \"$GITHUB_OUTPUT\" || return 1; snapshot gc-complete; phase gc-end || return 1; return \"$command_status\"; else command mbx \"$@\"; fi; }; "
);

const IMPORT_RECEIPT_OBSERVER: &str = concat!(
    "set -eu; set -o pipefail; ",
    "stream() { count_file=\"$GITHUB_OUTPUT.mbx-import-byte-count\"; if [ -e \"$count_file\" ] || [ -L \"$count_file\" ]; then echo \"MBX import byte-count marker already exists\" >&2; return 1; fi; wc -c < \"$2\" > \"$count_file\" || return 1; read -r bytes < \"$count_file\" || return 1; rm -f \"$count_file\" || return 1; printf '%s_bytes=%s\\n' \"$1\" \"$bytes\"; cat \"$2\" || return 1; }; ",
    "mbx() { if [ \"$1\" = \"cache\" ] && [ \"$2\" = \"import\" ] && [ -n \"${MBX_QUALIFICATION_IMPORT_RECEIPT:-}\" ]; then receipt=\"$MBX_QUALIFICATION_IMPORT_RECEIPT\"; receipt_tmp=\"${receipt}.tmp\"; stdout_file=\"$GITHUB_OUTPUT.mbx-import-stdout\"; stderr_file=\"$GITHUB_OUTPUT.mbx-import-stderr\"; for file in \"$receipt\" \"$receipt_tmp\" \"$stdout_file\" \"$stderr_file\"; do if [ -e \"$file\" ] || [ -L \"$file\" ]; then echo \"MBX import receipt marker already exists\" >&2; return 1; fi; done; if command mbx \"$@\" > \"$stdout_file\" 2> \"$stderr_file\"; then status=0; else status=$?; fi; { stream stdout \"$stdout_file\" || return 1; stream stderr \"$stderr_file\" || return 1; printf '\\nexit_status=%s\\n' \"$status\" || return 1; } > \"$receipt_tmp\" || return 1; mv \"$receipt_tmp\" \"$receipt\" || return 1; cat \"$stdout_file\" || return 1; cat \"$stderr_file\" >&2 || return 1; return \"$status\"; else command mbx \"$@\"; fi; }; "
);

pub(super) fn export_script(qualification_writer: bool, base: &str) -> String {
    if qualification_writer {
        format!("{EXPORT_OBSERVER}{base}")
    } else {
        base.to_owned()
    }
}

pub(super) fn import_script(qualification: bool, guard: &str, base: &str) -> String {
    if qualification {
        format!("{guard}{IMPORT_RECEIPT_OBSERVER}{base}")
    } else {
        format!("{guard}{base}")
    }
}

#[cfg(test)]
#[path = "mbx_bundle_qualification_tests.rs"]
mod tests;
