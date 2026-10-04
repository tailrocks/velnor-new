//! Fixed native-YAML MBX evidence scripts at the schema2 qualification seam.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;

use super::mbx_qualification_helpers::{COMPILE_STEP_NAME, CORRUPT_JOB_ID};
use crate::render::RenderContext;
use crate::yaml::Yaml;
use crate::{RenderError, document, mbx_bundle};

const ARTIFACT_STEP: &str = "Upload MBX cache evidence";
const START_SCRIPT: &str = include_str!("schema2_mbx_resource_start.sh");
const PHASE_SCRIPT: &str = include_str!("schema2_mbx_resource_phase.sh");
const SNAPSHOT_SCRIPT: &str = include_str!("schema2_mbx_resource_snapshot.sh");
const RECEIPT_SCRIPT: &str = include_str!("schema2_mbx_resource_receipt.sh");
const STOP_SCRIPT: &str = include_str!("schema2_mbx_resource_stop.sh");
const SAMPLER: &str = include_str!("schema2_mbx_resource_sampler.sh");
const PATH_VALIDATION: &str = include_str!("schema2_mbx_resource_path.sh");
const SAMPLER_SHA256: &str = "12775978694a493f9b16ef79d707a353a1326c524d391bcb474426d8488e37d1";
const PATH_VALIDATION_SHA256: &str =
    "1a73b2bc858d52af8cceaaf7e1dec8aedb2dc01618d05454d0cb862428fe5356";
const PREUPLOAD_SCRIPT: &str = r#"set -euo pipefail
evidence="$RUNNER_TEMP/mbx-cache-evidence"
bash "$evidence/sampler.sh" "$evidence" "$RUNNER_TEMP" "$GITHUB_ENV" "$MBX_QUALIFICATION_SAMPLE_INTERVAL" validate
test -d "$evidence"
test ! -L "$evidence"
test "$(realpath -e -- "$evidence")" = "$evidence"
test "$(stat -c '%a:%u:%g' -- "$evidence")" = "700:$(id -u):$(id -g)"
"#;

/// Render fixed measurement scripts around the typed MBX lifecycle.
/// # Errors
pub(super) fn render_job_steps(
    job_id: &str,
    typed_steps: &[Step],
    request: &super::MbxQualificationPins,
    context: &RenderContext,
) -> Result<Vec<Yaml>, RenderError> {
    let writer = typed_steps
        .iter()
        .any(|step| step.name == mbx_bundle::MBX_BUNDLE_EXPORT_NAME);
    let corrupt = job_id == CORRUPT_JOB_ID;
    let role = role_for_job(job_id, writer, corrupt);
    let probe = ProbeRenderContext {
        job_id,
        request,
        context,
        role,
        writer,
        corrupt,
    };
    let start = START_SCRIPT
        .replace(
            "__SAMPLER_BASE64__",
            &super::mbx_qualification_helpers::base64_encode(SAMPLER.as_bytes()),
        )
        .replace("__SAMPLER_SHA256__", SAMPLER_SHA256)
        .replace(
            "__PATH_VALIDATION_BASE64__",
            &super::mbx_qualification_helpers::base64_encode(PATH_VALIDATION.as_bytes()),
        )
        .replace("__PATH_VALIDATION_SHA256__", PATH_VALIDATION_SHA256);
    let mut output = vec![native_step(
        "Start bounded MBX resource sampler",
        &start,
        &super::mbx_resource_probe_env::observer_env(request, job_id, role, writer, corrupt),
        None,
    )?];
    let mut has_artifact = false;
    for step in typed_steps {
        if step.name == ARTIFACT_STEP {
            if has_artifact {
                return Err(RenderError::InvalidWorkflow(
                    "duplicate_mbx_evidence_artifact_step".to_owned(),
                ));
            }
            has_artifact = true;
            append_finalizers(&mut output, job_id, role, writer, corrupt, request)?;
            output.push(native_step(
                "Validate private MBX evidence before upload",
                PREUPLOAD_SCRIPT,
                &super::mbx_resource_probe_env::observer_env(
                    request, job_id, role, writer, corrupt,
                ),
                Some("always()"),
            )?);
        } else {
            append_typed_step(&mut output, step, &probe)?;
        }
        if step.name == ARTIFACT_STEP {
            output.push(document::step_to_yaml(job_id, step, context, &[], false)?);
        }
    }
    if !has_artifact {
        return Err(RenderError::InvalidWorkflow(
            "missing_mbx_evidence_artifact_step".to_owned(),
        ));
    }
    Ok(output)
}

struct ProbeRenderContext<'a> {
    job_id: &'a str,
    request: &'a super::MbxQualificationPins,
    context: &'a RenderContext,
    role: &'a str,
    writer: bool,
    corrupt: bool,
}

fn append_typed_step(
    output: &mut Vec<Yaml>,
    step: &Step,
    probe: &ProbeRenderContext<'_>,
) -> Result<(), RenderError> {
    let phases = phases_for(&step.name);
    if let Some((start, end)) = phases {
        output.push(phase_step(
            start,
            false,
            probe.request,
            probe.job_id,
            probe.role,
            probe.writer,
            probe.corrupt,
        )?);
        output.push(document::step_to_yaml(
            probe.job_id,
            step,
            probe.context,
            &[],
            false,
        )?);
        output.push(phase_step(
            end,
            true,
            probe.request,
            probe.job_id,
            probe.role,
            probe.writer,
            probe.corrupt,
        )?);
    } else {
        output.push(document::step_to_yaml(
            probe.job_id,
            step,
            probe.context,
            &[],
            false,
        )?);
    }
    if probe.corrupt && step.name == mbx_bundle::MBX_BUNDLE_RESTORE_NAME {
        append_mutation(
            output,
            probe.request,
            probe.job_id,
            probe.role,
            probe.writer,
            probe.corrupt,
        )?;
    }
    if probe.corrupt && step.name == mbx_bundle::MBX_BUNDLE_IMPORT_NAME {
        append_import_verification(
            output,
            probe.request,
            probe.job_id,
            probe.role,
            probe.writer,
            probe.corrupt,
        )?;
    }
    Ok(())
}

fn append_mutation(
    output: &mut Vec<Yaml>,
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> Result<(), RenderError> {
    output.push(phase_step(
        "corruption-start",
        false,
        request,
        job_id,
        role,
        writer,
        corrupt,
    )?);
    output.push(native_step(
        super::mbx_corrupt_probe::MUTATE_NAME,
        &super::mbx_corrupt_probe::mutation_script(),
        &super::mbx_resource_probe_env::mutation_env(request, job_id, role, writer, corrupt),
        None,
    )?);
    output.push(phase_step(
        "corruption-end",
        true,
        request,
        job_id,
        role,
        writer,
        corrupt,
    )?);
    Ok(())
}

fn append_import_verification(
    output: &mut Vec<Yaml>,
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> Result<(), RenderError> {
    output.push(phase_step(
        "corrupt-import-verify-start",
        false,
        request,
        job_id,
        role,
        writer,
        corrupt,
    )?);
    output.push(native_step(
        super::mbx_corrupt_probe::VERIFY_IMPORT_NAME,
        super::mbx_corrupt_probe::verify_import_script(),
        &super::mbx_resource_probe_env::observer_env(request, job_id, role, writer, corrupt),
        None,
    )?);
    output.push(phase_step(
        "corrupt-import-verified",
        true,
        request,
        job_id,
        role,
        writer,
        corrupt,
    )?);
    Ok(())
}

fn append_finalizers(
    output: &mut Vec<Yaml>,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
    request: &super::MbxQualificationPins,
) -> Result<(), RenderError> {
    output.push(native_step(
        "Record MBX cache identity and save outcome",
        RECEIPT_SCRIPT,
        &super::mbx_resource_probe_env::receipt_env(request, job_id, role, writer, corrupt),
        Some("always()"),
    )?);
    output.push(native_step(
        "Stop sampler and capture final MBX state",
        STOP_SCRIPT,
        &super::mbx_resource_probe_env::receipt_env(request, job_id, role, writer, corrupt),
        Some("always()"),
    )?);
    Ok(())
}

fn phase_step(
    phase: &str,
    snapshot: bool,
    request: &super::MbxQualificationPins,
    job_id: &str,
    role: &str,
    writer: bool,
    corrupt: bool,
) -> Result<Yaml, RenderError> {
    let mut env =
        super::mbx_resource_probe_env::observer_env(request, job_id, role, writer, corrupt);
    env.insert("MBX_PHASE".to_owned(), phase.to_owned());
    let script = if snapshot {
        SNAPSHOT_SCRIPT
    } else {
        PHASE_SCRIPT
    };
    native_step(
        &format!("Record MBX phase {phase}"),
        script,
        &env,
        snapshot.then_some("always()"),
    )
}

fn native_step(
    name: &str,
    script: &str,
    env: &BTreeMap<String, String>,
    condition: Option<&str>,
) -> Result<Yaml, RenderError> {
    crate::expressions::check_name_content(name)?;
    crate::steps::scan_for_private_subcommands(script)?;
    let env = crate::toolchain_env::with_credential_scrub(env);
    super::mbx_resource_probe_env::validate_native_env(&env)?;
    let mut fields = vec![("name".to_owned(), Yaml::str(name))];
    if let Some(condition) = condition {
        crate::steps::scan_for_private_subcommands(condition)?;
        fields.push(("if".to_owned(), Yaml::str(condition)));
    }
    fields.push((
        "env".to_owned(),
        Yaml::Map(
            env.iter()
                .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
                .collect(),
        ),
    ));
    fields.push(("shell".to_owned(), Yaml::str("bash")));
    fields.push((
        "run".to_owned(),
        Yaml::str(format!(
            "{}\n{}",
            crate::toolchain_env::credential_unset_prelude(),
            script
        )),
    ));
    Ok(Yaml::Map(fields))
}

fn phases_for(step: &str) -> Option<(&'static str, &'static str)> {
    match step {
        mbx_bundle::MBX_BUNDLE_RESTORE_NAME => Some(("restore-step-start", "restore-step-end")),
        mbx_bundle::MBX_BUNDLE_IMPORT_NAME => Some(("import-step-start", "import-step-end")),
        COMPILE_STEP_NAME => Some(("build-start", "build-end")),
        mbx_bundle::MBX_BUNDLE_EXPORT_NAME => Some(("export-step-start", "export-step-end")),
        mbx_bundle::MBX_BUNDLE_SAVE_NAME => Some(("save-step-start", "save-step-end")),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::super::mbx_qualification_helpers::{base64_encode, verify_embedded_payload};
    use super::{PATH_VALIDATION, PATH_VALIDATION_SHA256, SAMPLER, SAMPLER_SHA256, START_SCRIPT};

    #[test]
    fn sampler_source_keeps_measurements_bounded_and_labeled_as_lower_bounds() {
        assert!(SAMPLER.contains("observed_max_filesystem_used_bytes_lower_bound"));
        assert!(SAMPLER.contains("observed_max_filesystem_used_inodes_lower_bound"));
        assert!(SAMPLER.contains("inode_allocated_bytes_sum_dedup_within_tree"));
        assert!(SAMPLER.contains("st_blocks*512 once per dev/inode"));
        assert!(SAMPLER.contains("reflink/COW extent sharing is unknown"));
        assert!(SAMPLER.contains("GITHUB_ENV file not read"));
        assert!(SAMPLER.contains("max_files=20000"));
        assert!(SAMPLER.contains("max_hash_bytes=$((1024 * 1024 * 1024))"));
    }

    #[test]
    fn embedded_measurement_payloads_decode_and_match_their_sha256_pins() {
        let sampler = base64_encode(SAMPLER.as_bytes());
        let path_validation = base64_encode(PATH_VALIDATION.as_bytes());
        let start = START_SCRIPT
            .replace("__SAMPLER_BASE64__", &sampler)
            .replace("__SAMPLER_SHA256__", SAMPLER_SHA256)
            .replace("__PATH_VALIDATION_BASE64__", &path_validation)
            .replace("__PATH_VALIDATION_SHA256__", PATH_VALIDATION_SHA256);
        assert!(!start.contains("__SAMPLER_"));
        assert!(!start.contains("__PATH_VALIDATION_"));
        assert!(start.contains(&sampler));
        assert!(start.contains(&path_validation));
        verify_embedded_payload(SAMPLER, &sampler, SAMPLER_SHA256);
        verify_embedded_payload(PATH_VALIDATION, &path_validation, PATH_VALIDATION_SHA256);
    }
}
