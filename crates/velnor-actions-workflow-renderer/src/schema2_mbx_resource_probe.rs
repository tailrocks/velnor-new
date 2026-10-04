//! Bounded hosted resource sampling and retained MBX qualification evidence.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::steps;
use crate::{RenderError, mbx_bundle, schema2::MbxQualificationPins};

const EVIDENCE_DIR: &str = "${{ runner.temp }}/mbx-cache-evidence";
const INTERVAL_SECONDS: u8 = 5;
const SAMPLER: &str = include_str!("schema2_mbx_resource_sampler.sh");

const START_SCRIPT: &str = r#"set -eu
evidence="$RUNNER_TEMP/mbx-cache-evidence"
mkdir -m 700 -p "$evidence"
: > "$MBX_QUALIFICATION_PHASE_FILE"
cat > "$evidence/sampler.sh" <<'MBX_RESOURCE_SAMPLER'
__SAMPLER__
MBX_RESOURCE_SAMPLER
chmod 700 "$evidence/sampler.sh"
nohup bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" sample > "$evidence/sampler.log" 2>&1 </dev/null &
printf '%s\n' "$!" > "$evidence/sampler.pid"
printf 'sampler_started_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" > "$evidence/sampler-state.txt"
"#;

const PHASE_SCRIPT: &str = r#"set -eu
printf '%s\t%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" "$MBX_PHASE" >> "$MBX_QUALIFICATION_PHASE_FILE"
"#;

const RECEIPT_SCRIPT: &str = r#"set -eu
evidence="$RUNNER_TEMP/mbx-cache-evidence"
mkdir -m 700 -p "$evidence"
if [ -n "${MBX_CACHE_DIR-}" ]; then cache_root="$MBX_CACHE_DIR"; else cache_root=''; fi
if [ -s "$evidence/fallback-cache-root.txt" ]; then IFS= read -r import_root < "$evidence/fallback-cache-root.txt"; else import_root="$cache_root"; fi
jq -cn \
  --arg run_id "$GITHUB_RUN_ID" --arg run_attempt "$GITHUB_RUN_ATTEMPT" \
  --arg sha "$GITHUB_SHA" --arg ref "$GITHUB_REF" --arg workflow_ref "$GITHUB_WORKFLOW_REF" \
  --arg runner_os "$RUNNER_OS" --arg runner_arch "$RUNNER_ARCH" \
  --arg image_os "${ImageOS-}" --arg image_version "${ImageVersion-}" \
  --arg mbx_version "${MBX_VERSION-}" --arg rust_toolchain "${RUSTUP_TOOLCHAIN-}" \
  --arg action_ref "$MBX_QUALIFICATION_ACTION_REF" --arg cache_scope "$MBX_CACHE_SCOPE" \
  --arg cache_primary "$MBX_QUALIFICATION_CACHE_PRIMARY" \
  --arg cache_prefix "$MBX_QUALIFICATION_CACHE_PREFIX" \
  --arg cache_matched_key "$MBX_QUALIFICATION_CACHE_MATCHED_KEY" \
  --arg cache_hit "$MBX_QUALIFICATION_CACHE_HIT" \
  --arg export_ready "$MBX_QUALIFICATION_EXPORT_READY" \
  --arg export_status "$MBX_QUALIFICATION_EXPORT_STATUS" \
  --arg gc_status "$MBX_QUALIFICATION_GC_STATUS" \
  --arg save_outcome "$MBX_QUALIFICATION_SAVE_OUTCOME" \
  --arg job_id "$MBX_QUALIFICATION_JOB_ID" --arg role "$MBX_QUALIFICATION_ROLE" \
  --arg generation "$MBX_QUALIFICATION_CACHE_GENERATION" \
  --arg rustc_identity "$MBX_QUALIFICATION_RUSTC_IDENTITY" \
  --arg imported_objects "$(jq -r '.objects // empty' "$evidence/mbx-cache-stats-import-step-end.json" 2>/dev/null || true)" \
  --arg cached_compilations "$(jq -r '.savings.cached_compilations // empty' "$evidence/mbx-stats-build-end.json" 2>/dev/null || true)" \
  --arg import_status "$(sed -n 's/^exit_status=//p' "$MBX_QUALIFICATION_IMPORT_RECEIPT" 2>/dev/null | tail -n 1 || true)" \
  --arg import_receipt_path "$MBX_QUALIFICATION_IMPORT_RECEIPT" \
  --arg cache_root "$cache_root" --arg import_root "$import_root" \
  --arg bundle "$RUNNER_TEMP/mbx-single-bundle" \
  --arg cargo_home "$CARGO_HOME" \
  '{job_id:$job_id,role:$role,run_id:$run_id,run_attempt:$run_attempt,source_sha:$sha,source_ref:$ref,workflow_ref:$workflow_ref,runner_os:$runner_os,runner_arch:$runner_arch,image_os:$image_os,image_version:$image_version,mbx_action_ref:$action_ref,mbx_version:$mbx_version,rust_version:$rust_toolchain,generation:$generation,rustc_identity:$rustc_identity,scope:$cache_scope,primary_key:$cache_primary,cache_prefix:$cache_prefix,matched_key:$cache_matched_key,cache_hit:$cache_hit,export_ready:$export_ready,export_status:$export_status,gc_status:$gc_status,save_outcome:$save_outcome,imported_objects:(try ($imported_objects|tonumber) catch null),cached_compilations:(try ($cached_compilations|tonumber) catch null),import_status:$import_status,import_receipt_path:$import_receipt_path,selected_import_root:$import_root,mbx_cache_root:$cache_root,bundle:$bundle,cargo_home:$cargo_home}' \
  > "$evidence/cache-receipt.json"
"#;

const STOP_SCRIPT: &str = r#"set -eu
evidence="$RUNNER_TEMP/mbx-cache-evidence"
mkdir -m 700 -p "$evidence"
if [ -n "${MBX_CACHE_DIR-}" ] && [ -d "$MBX_CACHE_DIR" ]; then printf '%s\n' "$MBX_CACHE_DIR" > "$evidence/final-cache-root.txt"; fi
if command -v mbx >/dev/null 2>&1; then
  mbx cache stats --json > "$evidence/final-cache-stats.json" 2> "$evidence/final-cache-stats.stderr" || printf 'exit_status=%s\n' "$?" >> "$evidence/final-cache-stats.stderr"
  mbx stats --json > "$evidence/final-mbx-stats.json" 2> "$evidence/final-mbx-stats.stderr" || printf 'exit_status=%s\n' "$?" >> "$evidence/final-mbx-stats.stderr"
fi
if [ -f "$evidence/sampler.pid" ]; then
  : > "$evidence/sampler.stop"
  IFS= read -r pid < "$evidence/sampler.pid"
  count=0
  while kill -0 "$pid" 2>/dev/null && [ "$count" -lt "$MBX_QUALIFICATION_FINALIZER_WAIT" ]; do sleep 1; count=$((count + 1)); done
  if kill -0 "$pid" 2>/dev/null; then kill "$pid" 2>/dev/null || :; fi
fi
if [ -f "$evidence/sampler.sh" ]; then
  if ! bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" snapshot final; then
    echo 'final_inventory_status=failed' >> "$evidence/snapshot-errors.txt"
  fi
fi
printf 'sampler_stopped_utc=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)" >> "$evidence/sampler-state.txt"
status=0
for label in $MBX_QUALIFICATION_EXPECTED_INVENTORIES; do
  if [ ! -s "$evidence/inventory-$label.tsv" ]; then
    echo "missing_inventory=$label" >> "$evidence/qualification-errors.txt"
    status=1
  fi
done
for file in runner-metadata.json samples.jsonl sampling-summary.json inventory-final.tsv inventory-meta-final.tsv cache-receipt.json mbx-cache-stats-import-step-end.json mbx-stats-build-end.json; do
  if [ ! -s "$evidence/$file" ]; then
    echo "missing_required_file=$file" >> "$evidence/qualification-errors.txt"
    status=1
  fi
done
if [ -s "$evidence/snapshot-errors.txt" ] || [ -s "$evidence/inventory-errors.txt" ]; then status=1; fi
if grep -q 'df_status=failed' "$evidence"/df-*-bytes.txt "$evidence"/df-*-inodes.txt 2>/dev/null; then status=1; fi
for file in "$evidence/mbx-cache-stats-import-step-end.json" "$evidence/mbx-stats-build-end.json"; do
  if [ -s "$file" ] && ! jq -e . "$file" >/dev/null; then status=1; fi
done
if [ "$status" -eq 0 ]; then printf 'qualification_status\tcomplete\n' > "$evidence/qualification-status.tsv"; else printf 'qualification_status\tincomplete\n' > "$evidence/qualification-status.tsv"; exit 1; fi
"#;

/// Add bounded sampling, phase markers, a final receipt, and an always-upload artifact.
/// # Errors
pub(super) fn attach(
    jobs: &mut BTreeMap<String, Job>,
    request: &super::MbxQualificationPins,
) -> Result<(), RenderError> {
    for (job_id, job) in jobs.iter_mut() {
        if !job
            .steps
            .iter()
            .any(|step| step.name == super::mbx_qualification::MBX_CACHE_ACTION_STEP)
        {
            continue;
        }
        let sampler = START_SCRIPT.replace("__SAMPLER__", SAMPLER);
        let writer = job
            .steps
            .iter()
            .any(|step| step.name == mbx_bundle::MBX_BUNDLE_EXPORT_NAME);
        let corrupt = job_id.contains("corrupt");
        let role = role_for_job(job_id, writer, corrupt);
        let start = steps::shell_step(
            "Start bounded MBX resource sampler",
            vec!["bash".to_owned(), "-c".to_owned(), sampler],
            observer_env(request, job_id, role, writer, corrupt),
        )?;
        let mut measured = Vec::with_capacity(job.steps.len() + 14);
        measured.push(start);
        for step in job.steps.drain(..) {
            append_step_markers(&mut measured, step, request, job_id, role, writer, corrupt)?;
        }
        let mut receipt = steps::shell_step(
            "Record MBX cache identity and save outcome",
            vec!["bash".to_owned(), "-c".to_owned(), RECEIPT_SCRIPT.to_owned()],
            receipt_env(request, job_id, role),
        )?;
        receipt.condition = Some("always()".to_owned());
        let mut stop = steps::shell_step(
            "Stop sampler and capture final MBX state",
            vec!["bash".to_owned(), "-c".to_owned(), STOP_SCRIPT.to_owned()],
            observer_env(request, job_id, role, writer, corrupt),
        )?;
        stop.condition = Some("always()".to_owned());
        measured.push(receipt);
        measured.push(stop);
        let artifact_name = format!("mbx-cache-evidence-{job_id}");
        let mut upload =
            steps::upload_artifact_step(&artifact_name, "$RUNNER_TEMP/mbx-cache-evidence")?;
        upload.name = "Upload MBX cache evidence".to_owned();
        upload.condition = Some("always()".to_owned());
        if let StepKind::Action { with, .. } = &mut upload.kind {
            with.insert("if-no-files-found".to_owned(), "error".to_owned());
        }
        measured.push(upload);
        job.steps = measured;
    }
    Ok(())
}

fn append_step_markers(
    output: &mut Vec<Step>,
    step: Step,
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> Result<(), RenderError> {
    let phases = match step.name.as_str() {
        mbx_bundle::MBX_BUNDLE_RESTORE_NAME => Some(("restore-step-start", "restore-step-end")),
        mbx_bundle::MBX_BUNDLE_IMPORT_NAME => Some(("import-step-start", "import-step-end")),
        super::mbx_qualification_helpers::COMPILE_STEP_NAME => Some(("build-start", "build-end")),
        mbx_bundle::MBX_BUNDLE_EXPORT_NAME => Some(("export-step-start", "export-step-end")),
        mbx_bundle::MBX_BUNDLE_SAVE_NAME => Some(("save-step-start", "save-step-end")),
        super::mbx_corrupt_probe::MUTATE_NAME => Some(("corruption-start", "corruption-end")),
        super::mbx_corrupt_probe::VERIFY_IMPORT_NAME => {
            Some(("corrupt-import-verify-start", "corrupt-import-verified"))
        }
        _ => None,
    };
    if let Some((before, after)) = phases {
        output.push(phase_step(before, false, false, request, job_id, role, writer, corrupt)?);
        output.push(step);
        output.push(phase_step(after, true, true, request, job_id, role, writer, corrupt)?);
    } else {
        output.push(step);
    }
    Ok(())
}

fn phase_step(
    phase: &str,
    always: bool,
    snapshot: bool,
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> Result<Step, RenderError> {
    let command = if snapshot {
        format!(
            "{}\n{}",
            PHASE_SCRIPT,
            snapshot_shell("$MBX_PHASE")
        )
    } else {
        PHASE_SCRIPT.to_owned()
    };
    let mut step = steps::shell_step(
        &format!("Record MBX phase {phase}"),
        vec!["bash".to_owned(), "-c".to_owned(), command],
        phase_env(phase, request, job_id, role, writer, corrupt),
    )?;
    if always {
        step.condition = Some("always()".to_owned());
    }
    Ok(step)
}

fn snapshot_shell(phase: &str) -> String {
    format!(
        "set -eu\nevidence=\"$RUNNER_TEMP/mbx-cache-evidence\"\nif ! bash \"$evidence/sampler.sh\" \"$evidence\" \"$RUNNER_TEMP\" \"$GITHUB_ENV\" \"$MBX_QUALIFICATION_SAMPLE_INTERVAL\" snapshot {phase}; then echo 'phase={phase} status=failed' >> \"$evidence/snapshot-errors.txt\"; fi\nif command -v mbx >/dev/null 2>&1; then\n  capture_stats() {{ base=\"$1\"; shift; if \"$@\" > \"$base.json\" 2> \"$base.stderr\"; then :; else status=$?; printf 'exit_status=%s\\n' \"$status\" >> \"$base.stderr\"; fi; }}\n  capture_stats \"$evidence/mbx-cache-stats-{phase}\" mbx cache stats --json\n  capture_stats \"$evidence/mbx-stats-{phase}\" mbx stats --json\nfi\nprintf '%s\\t%s\\n' \"$(date -u +%Y-%m-%dT%H:%M:%S.%NZ)\" \"{phase}-inventory-end\" >> \"$MBX_QUALIFICATION_PHASE_FILE\""
    )
}

fn receipt_env(
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
) -> BTreeMap<String, String> {
    let mut env = observer_env(request, job_id, role, false, false);
    env.extend(BTreeMap::from([
        ("MBX_QUALIFICATION_CACHE_PRIMARY".to_owned(), "${{ steps.mbx-bundle-key.outputs.primary }}".to_owned()),
        ("MBX_QUALIFICATION_CACHE_PREFIX".to_owned(), "${{ steps.mbx-bundle-key.outputs.prefix }}".to_owned()),
        ("MBX_QUALIFICATION_CACHE_MATCHED_KEY".to_owned(), "${{ steps.mbx-bundle.outputs.cache-matched-key }}".to_owned()),
        ("MBX_QUALIFICATION_CACHE_HIT".to_owned(), "${{ steps.mbx-bundle.outputs.cache-hit }}".to_owned()),
        ("MBX_QUALIFICATION_EXPORT_READY".to_owned(), "${{ steps.mbx-export.outputs.ready }}".to_owned()),
        ("MBX_QUALIFICATION_EXPORT_STATUS".to_owned(), "${{ steps.mbx-export.outputs.export_status }}".to_owned()),
        ("MBX_QUALIFICATION_GC_STATUS".to_owned(), "${{ steps.mbx-export.outputs.gc_status }}".to_owned()),
        ("MBX_QUALIFICATION_SAVE_OUTCOME".to_owned(), "${{ steps.mbx-bundle-save.outcome }}".to_owned()),
        ("MBX_QUALIFICATION_CACHE_GENERATION".to_owned(), "${{ steps.mbx-bundle-key.outputs.generation }}".to_owned()),
        ("MBX_QUALIFICATION_RUSTC_IDENTITY".to_owned(), "${{ steps.mbx-bundle-key.outputs.rustc_identity }}".to_owned()),
    ]));
    env
}

fn observer_env(
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> BTreeMap<String, String> {
    let mut env = super::mbx_qualification::qualification_shell_env(request);
    let mut expected = vec!["restore-step-end", "import-step-end", "build-end", "final"];
    if writer {
        expected.extend([
            "export-complete",
            "gc-complete",
            "export-step-end",
            "save-step-end",
        ]);
    }
    if corrupt {
        expected.extend(["corruption-end", "corrupt-import-verified"]);
    }
    let expected = expected.join(" ");
    env.extend(BTreeMap::from([
        (
            "MBX_QUALIFICATION_PHASE_FILE".to_owned(),
            format!("{EVIDENCE_DIR}/phases.tsv"),
        ),
        (
            "MBX_QUALIFICATION_SAMPLE_INTERVAL".to_owned(),
            INTERVAL_SECONDS.to_string(),
        ),
        (
            "MBX_QUALIFICATION_FINALIZER_WAIT".to_owned(),
            "6".to_owned(),
        ),
        ("MBX_QUALIFICATION_EXPECTED_INVENTORIES".to_owned(), expected),
        ("MBX_QUALIFICATION_JOB_ID".to_owned(), job_id.to_owned()),
        ("MBX_QUALIFICATION_ROLE".to_owned(), role.to_owned()),
    ]));
    env
}

fn phase_env(
    phase: &str,
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> BTreeMap<String, String> {
    let mut env = observer_env(request, job_id, role, writer, corrupt);
    env.insert("MBX_PHASE".to_owned(), phase.to_owned());
    env
}

fn role_for_job(job_id: &str, writer: bool, corrupt: bool) -> &'static str {
    match job_id {
        "mbx-parallel-seed" => "seed",
        "mbx-parallel-reader-a" => "reader-a",
        "mbx-parallel-reader-b" => "reader-b",
        "mbx-parallel-new-key-writer" => "new-key-writer",
        "mbx-parallel-observer-shared" => "observer-shared",
        "mbx-parallel-observer-new" => "observer-new",
        _ if corrupt => "corrupt-reader",
        _ if writer => "writer",
        _ => "reader",
    }
}
