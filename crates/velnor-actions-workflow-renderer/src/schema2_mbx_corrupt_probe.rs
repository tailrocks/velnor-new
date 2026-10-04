//! Job-local corruption and cold-fallback proof for the restored MBX bundle.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step};

use crate::steps;
use crate::{RenderError, mbx_bundle};

pub(super) const MUTATE_NAME: &str = "Corrupt restored MBX bundle payload";
pub(super) const VERIFY_IMPORT_NAME: &str = "Verify corrupt import selected a cold MBX store";
const CORRUPTOR: &str = include_str!("schema2_mbx_corrupt_bundle.sh");

const MUTATE_SCRIPT: &str = r#"set -eu
evidence="$RUNNER_TEMP/mbx-cache-evidence"
bundle="$RUNNER_TEMP/mbx-single-bundle"
mkdir -m 700 -p "$evidence"
test "$MATCHED" = "$PRIMARY"
test -n "$PRIMARY"
test "$CACHE_HIT" = true
test -d "$bundle"
test ! -L "$bundle"
test -n "$MBX_CACHE_DIR"
printf '%s\n' "$MBX_CACHE_DIR" > "$evidence/original-cache-root.txt"
cat > "$evidence/corrupt-bundle.sh" <<'MBX_CORRUPTOR'
__CORRUPTOR__
MBX_CORRUPTOR
chmod 700 "$evidence/corrupt-bundle.sh"
bash "$evidence/corrupt-bundle.sh" "$bundle" "$evidence"
"#;

const VERIFY_IMPORT_SCRIPT: &str = r#"set -eu
evidence="$RUNNER_TEMP/mbx-cache-evidence"
receipt="$MBX_QUALIFICATION_IMPORT_RECEIPT"
test -s "$receipt"
last_line="$(tail -n 1 "$receipt")"
case "$last_line" in exit_status=*) status="${last_line#exit_status=}" ;; *) echo 'raw MBX import status missing' >&2; exit 1 ;; esac
case "$status" in ''|*[!0-9]*) echo 'raw MBX import status malformed' >&2; exit 1 ;; esac
test "$status" -ne 0
IFS= read -r original_root < "$evidence/original-cache-root.txt"
test -n "$original_root"
test -n "$MBX_CACHE_DIR"
test "$MBX_CACHE_DIR" != "$original_root"
test -d "$MBX_CACHE_DIR"
printf '%s\n' "$MBX_CACHE_DIR" > "$evidence/fallback-cache-root.txt"
mbx cache stats --json > "$evidence/corrupt-cache-stats-before-build.json"
mbx stats --json > "$evidence/corrupt-mbx-stats-before-build.json"
jq -e '.objects == 0' "$evidence/corrupt-cache-stats-before-build.json" >/dev/null
printf '%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" corrupt-import-cold-store-verified >> "$MBX_QUALIFICATION_PHASE_FILE"
"#;

/// Add the isolated reader's mutation and import-fallback assertions.
/// # Errors
pub(super) fn attach(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    let Some(job) = jobs.get_mut(super::mbx_qualification_helpers::CORRUPT_JOB_ID) else {
        return Err(RenderError::InvalidWorkflow(
            "missing_mbx_corrupt_reader_job".to_owned(),
        ));
    };
    let Some(restore) = job
        .steps
        .iter()
        .position(|step| step.name == mbx_bundle::MBX_BUNDLE_RESTORE_NAME)
    else {
        return Err(RenderError::InvalidWorkflow(
            "missing_mbx_corrupt_reader_restore".to_owned(),
        ));
    };
    let mutation = mutation_step()?;
    job.steps.insert(restore + 1, mutation);
    let Some(import) = job
        .steps
        .iter()
        .position(|step| step.name == mbx_bundle::MBX_BUNDLE_IMPORT_NAME)
    else {
        return Err(RenderError::InvalidWorkflow(
            "missing_mbx_corrupt_reader_import".to_owned(),
        ));
    };
    let verify = verify_import_step()?;
    job.steps.insert(import + 1, verify);
    Ok(())
}

fn mutation_step() -> Result<Step, RenderError> {
    let command = MUTATE_SCRIPT.replace("__CORRUPTOR__", CORRUPTOR);
    steps::shell_step(
        MUTATE_NAME,
        vec!["bash".to_owned(), "-c".to_owned(), command],
        BTreeMap::from([
            (
                "MATCHED".to_owned(),
                "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned(),
            ),
            (
                "PRIMARY".to_owned(),
                "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned(),
            ),
            (
                "CACHE_HIT".to_owned(),
                "${{ steps.mbx-bundle.outputs.cache-hit }}".to_owned(),
            ),
        ]),
    )
}

fn verify_import_step() -> Result<Step, RenderError> {
    steps::shell_step(
        VERIFY_IMPORT_NAME,
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            VERIFY_IMPORT_SCRIPT.to_owned(),
        ],
        BTreeMap::new(),
    )
}
