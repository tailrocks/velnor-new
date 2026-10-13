//! Opaque same-size payload mutation and cold-store fallback scripts.

const CORRUPTOR: &str = include_str!("schema2_mbx_corrupt_bundle.sh");
const CORRUPTOR_SHA256: &str = "dc043b77adb5f541ddff3a0d8c7a31288864709caf333b78734545e684906a1d";

pub(super) const MUTATE_NAME: &str = "Corrupt restored MBX bundle payload";
pub(super) const VERIFY_IMPORT_NAME: &str = "Verify corrupt import selected a cold MBX store";

pub(super) const VERIFY_IMPORT_SCRIPT: &str = r#"set -eu
evidence="$RUNNER_TEMP/mbx-cache-evidence"
bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate
receipt="$MBX_QUALIFICATION_IMPORT_RECEIPT"
test -s "$receipt"
test "$(head -n 1 "$receipt")" = 'command=import'
grep -Eq '^stdout_bytes=[0-9]+$' "$receipt"
grep -Eq '^stderr_bytes=[0-9]+$' "$receipt"
last_line="$(tail -n 1 "$receipt")"
case "$last_line" in exit_status=*) status="${last_line#exit_status=}" ;; *) echo 'raw MBX import status missing' >&2; exit 1 ;; esac
case "$status" in ''|*[!0-9]*) echo 'raw MBX import status malformed' >&2; exit 1 ;; esac
test "$status" -ne 0
IFS= read -r original_root < "$evidence/original-cache-root.txt"
selected_root="$MBX_SELECTED_CACHE_ROOT"
test -n "$original_root"
test -n "$selected_root"
test "$selected_root" != "$original_root"
test "$MBX_CACHE_DIR" = "$selected_root"
runner_temp="$(realpath -e -- "$RUNNER_TEMP")"
selected_real="$(realpath -e -- "$selected_root")"
test "$selected_real" = "$selected_root"
case "$selected_real" in "$runner_temp"/*) ;; *) echo 'selected fallback root escaped runner.temp' >&2; exit 1 ;; esac
test -d "$selected_root"
test ! -e "$evidence/fallback-cache-root.txt"
printf '%s\n' "$selected_root" > "$evidence/fallback-cache-root.txt"
mbx cache stats --json > "$evidence/corrupt-cache-stats-before-build.json"
mbx stats --json > "$evidence/corrupt-mbx-stats-before-build.json"
jq -e '.objects == 0 and .action_results == 0' "$evidence/corrupt-cache-stats-before-build.json" >/dev/null
jq -e '.savings.cached_compilations == 0' "$evidence/corrupt-mbx-stats-before-build.json" >/dev/null
printf '%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" corrupt-import-cold-store-verified >> "$MBX_QUALIFICATION_PHASE_FILE"
"#;

const MUTATE_SCRIPT: &str = r#"set -eu
evidence="$RUNNER_TEMP/mbx-cache-evidence"
bundle="$RUNNER_TEMP/mbx-single-bundle"
bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate
test "$MATCHED" = "$PRIMARY"
test -n "$PRIMARY"
test "$CACHE_HIT" = true
test -d "$bundle"
test ! -L "$bundle"
runner_temp="$(realpath -e -- "$RUNNER_TEMP")"
bundle_real="$(realpath -e -- "$bundle")"
test "$bundle_real" = "$bundle"
case "$bundle_real" in "$runner_temp"/mbx-single-bundle) ;; *) echo 'bundle escaped runner.temp' >&2; exit 1 ;; esac
test -n "$MBX_CACHE_DIR"
cache_real="$(realpath -e -- "$MBX_CACHE_DIR")"
test "$cache_real" = "$MBX_CACHE_DIR"
case "$cache_real" in "$runner_temp"/*) ;; *) echo 'MBX root escaped runner.temp' >&2; exit 1 ;; esac
test ! -e "$evidence/original-cache-root.txt"
test ! -L "$evidence/original-cache-root.txt"
printf '%s\n' "$cache_real" > "$evidence/original-cache-root.txt"
test ! -e "$evidence/corrupt-bundle.sh"
test ! -L "$evidence/corrupt-bundle.sh"
(
  set -o noclobber
  printf '%s' '__CORRUPTOR_BASE64__' | base64 --decode > "$evidence/corrupt-bundle.sh"
)
printf '%s  %s\n' '__CORRUPTOR_SHA256__' "$evidence/corrupt-bundle.sh" | sha256sum --check --status
chmod 700 "$evidence/corrupt-bundle.sh"
bash "$evidence/corrupt-bundle.sh" "$bundle" "$evidence" "$runner_temp"
"#;

pub(super) fn mutation_script() -> String {
    MUTATE_SCRIPT
        .replace(
            "__CORRUPTOR_BASE64__",
            &super::mbx_qualification_helpers::base64_encode(CORRUPTOR.as_bytes()),
        )
        .replace("__CORRUPTOR_SHA256__", CORRUPTOR_SHA256)
}

pub(super) fn verify_import_script() -> &'static str {
    VERIFY_IMPORT_SCRIPT
}

#[cfg(test)]
mod tests {
    use super::super::mbx_qualification_helpers::{base64_encode, verify_embedded_payload};
    use super::{
        CORRUPTOR, CORRUPTOR_SHA256, MUTATE_SCRIPT, VERIFY_IMPORT_SCRIPT, mutation_script,
    };

    #[test]
    fn corruptor_uses_a_bounded_nofollow_atomic_same_size_replacement() {
        assert!(CORRUPTOR.contains("find -P \"$bundle\" -type f"));
        assert!(CORRUPTOR.contains("capture_ancestors \"$parent\""));
        assert!(CORRUPTOR.contains("validate_ancestors || return 1"));
        assert!(CORRUPTOR.contains("mktemp --tmpdir=\"/proc/$$/fd/$parent_fd\""));
        assert!(CORRUPTOR.contains("mv -T -- \"./$temp_basename\" \"./$eligible_basename\""));
        assert!(CORRUPTOR.contains("\"$size\" == \"$(stat -c '%s' -- \"$eligible_file\")\""));
        assert!(CORRUPTOR.contains("bundle changed outside one same-size payload replacement"));
        assert!(!CORRUPTOR.contains("dd of=\"$eligible_file\""));
    }

    #[test]
    fn mutation_and_fallback_require_exact_hit_raw_failure_and_cold_root() {
        assert!(MUTATE_SCRIPT.contains("test \"$MATCHED\" = \"$PRIMARY\""));
        assert!(MUTATE_SCRIPT.contains("test \"$CACHE_HIT\" = true"));
        assert!(MUTATE_SCRIPT.contains("sha256sum --check --status"));
        assert!(VERIFY_IMPORT_SCRIPT.contains("command=import"));
        assert!(VERIFY_IMPORT_SCRIPT.contains("test \"$status\" -ne 0"));
        assert!(VERIFY_IMPORT_SCRIPT.contains("test \"$selected_root\" != \"$original_root\""));
        assert!(VERIFY_IMPORT_SCRIPT.contains("test \"$MBX_CACHE_DIR\" = \"$selected_root\""));
        assert!(VERIFY_IMPORT_SCRIPT.contains(".objects == 0 and .action_results == 0"));
        assert!(VERIFY_IMPORT_SCRIPT.contains(".savings.cached_compilations == 0"));
    }

    #[test]
    fn embedded_corruptor_decodes_and_matches_its_sha256_pin() {
        let encoded = base64_encode(CORRUPTOR.as_bytes());
        let rendered = mutation_script();
        assert!(rendered.contains(&encoded));
        assert!(rendered.contains(CORRUPTOR_SHA256));
        assert!(!rendered.contains("__CORRUPTOR_"));
        verify_embedded_payload(CORRUPTOR, &encoded, CORRUPTOR_SHA256);
    }
}
