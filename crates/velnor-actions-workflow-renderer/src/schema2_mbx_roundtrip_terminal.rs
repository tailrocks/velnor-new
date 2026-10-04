//! Terminal stock-restore certification for the MBX roundtrip qualification.

use std::collections::BTreeMap;

use crate::yaml::Yaml;
use crate::{RenderError, document, steps};

use super::MbxQualificationPins;
use super::features::{base, finish, gated};
use super::mbx_qualification_helpers::{CORRUPT_JOB_ID, READER_JOB_ID, WRITER_JOB_ID};

const JOB_ID: &str = "mbx-cache-roundtrip-terminal";
const JOB_NAME: &str = "MBX objects cache / terminal certification";
const JOB_GATE: &str = "always() && (inputs.mode == 'mbx-cache-roundtrip' && github.event_name == 'workflow_dispatch' && github.ref == 'refs/heads/main' && github.ref_protected == true)";
const RESULT_DIR: &str = "${{ runner.temp }}/mbx-roundtrip-terminal";
const CLASSIFY_STEP: &str = "Classify MBX stock restore outcomes";
const UPLOAD_STEP: &str = "Upload MBX terminal certification";
const RESULT_NAME: &str = "mbx-cache-roundtrip-terminal-";
const PREPARE_SCRIPT: &str = r#"set -euo pipefail
umask 077
runner_temp="${RUNNER_TEMP:?}"
test -d "$runner_temp" && test ! -L "$runner_temp"
test "$(realpath -e -- "$runner_temp")" = "$runner_temp"
test "$(stat -c '%u' -- "$runner_temp")" = "$(id -u)"
root="$runner_temp/mbx-roundtrip-terminal"
stock="$runner_temp/mbx-stock-restore-evidence"
for path in "$root" "$stock"; do
  if [ -e "$path" ] || [ -L "$path" ]; then
    echo 'terminal evidence directory already exists' >&2
    exit 1
  fi
done
mkdir -m 700 "$root" "$stock"
mkdir -m 700 "$root/writer" "$root/reader" "$root/corrupt-reader"
for directory in "$root" "$root/writer" "$root/reader" "$root/corrupt-reader" "$stock"; do
  stock_restore_private_dir "$directory" "$runner_temp"
done
"#;
const CERTIFY_SCRIPT: &str = include_str!("schema2_mbx_roundtrip_terminal.sh");

/// Render the always-run roundtrip evidence gate.
/// # Errors
pub(super) fn job(
    request: &MbxQualificationPins,
    hosted: &Yaml,
) -> Result<(String, Yaml), RenderError> {
    let Yaml::Str(runs_on) = hosted else {
        return Err(RenderError::InvalidWorkflow(
            "mbx_roundtrip_terminal_requires_hosted_runner".to_owned(),
        ));
    };
    let context = super::mbx_qualification::step_context();
    let mut fields = base(JOB_NAME, Yaml::str(runs_on), 30);
    fields.push((
        "needs".to_owned(),
        Yaml::Seq(
            [WRITER_JOB_ID, READER_JOB_ID, CORRUPT_JOB_ID]
                .into_iter()
                .map(Yaml::str)
                .collect(),
        ),
    ));
    fields.push((
        "permissions".to_owned(),
        Yaml::Map(vec![("actions".to_owned(), Yaml::str("read"))]),
    ));
    let mut rendered = vec![prepare_step()?];
    rendered.extend(download_steps(&context)?);
    rendered.push(classify_step(request)?);
    rendered.push(upload_step(&context)?);
    Ok(gated(finish(JOB_ID, fields, rendered), JOB_GATE))
}

fn download_steps(context: &crate::render::RenderContext) -> Result<Vec<Yaml>, RenderError> {
    let mut rendered = Vec::new();
    for (role, job_id, directory) in [
        ("writer", WRITER_JOB_ID, "writer"),
        ("reader", READER_JOB_ID, "reader"),
        ("corrupt reader", CORRUPT_JOB_ID, "corrupt-reader"),
    ] {
        let mut download = steps::download_artifact_step(
            &format!("mbx-cache-evidence-{job_id}-{}", steps::RUN_KEY_EXPR),
            &format!("{RESULT_DIR}/{directory}"),
        )?;
        download.name = format!("Download MBX {role} evidence");
        download.condition =
            Some("always() && steps.prepare_terminal_evidence.outcome == 'success'".to_owned());
        rendered.push(document::step_to_yaml(
            JOB_ID,
            &download,
            context,
            &[],
            false,
        )?);
    }
    Ok(rendered)
}

fn classify_step(request: &MbxQualificationPins) -> Result<Yaml, RenderError> {
    let script = stock_restore_script(CERTIFY_SCRIPT);
    crate::steps::scan_for_private_subcommands(&script)?;
    crate::expressions::check_name_content(CLASSIFY_STEP)?;
    Ok(token_shell_step(
        CLASSIFY_STEP,
        &script,
        &BTreeMap::from([
            (
                "MBX_EXPECTED_ACTION_REF".to_owned(),
                request.mbx_action_uses.clone(),
            ),
            (
                "MBX_EXPECTED_MODE".to_owned(),
                "mbx-cache-roundtrip".to_owned(),
            ),
            (
                "MBX_EXPECTED_RUST_VERSION".to_owned(),
                request.rust_version.clone(),
            ),
            (
                "MBX_EXPECTED_VERSION".to_owned(),
                request.mbx_version.clone(),
            ),
        ]),
        Some("always() && steps.prepare_terminal_evidence.outcome == 'success'"),
    ))
}

fn prepare_step() -> Result<Yaml, RenderError> {
    let script = stock_restore_script(PREPARE_SCRIPT);
    raw_step(
        "Prepare private MBX terminal evidence",
        "prepare_terminal_evidence",
        &script,
        Some("always()"),
    )
}

fn stock_restore_script(body: &str) -> String {
    let prelude = super::mbx_stock_restore::STOCK_RESTORE_CLASSIFIER_SCRIPT;
    let mut script = String::with_capacity(prelude.len() + body.len() + 1);
    script.push_str(prelude);
    script.push('\n');
    script.push_str(body);
    script
}

fn upload_step(context: &crate::render::RenderContext) -> Result<Yaml, RenderError> {
    let mut upload = steps::upload_artifact_step(
        &format!("{RESULT_NAME}{}", steps::RUN_KEY_EXPR),
        "${{ runner.temp }}/mbx-stock-restore-evidence",
    )?;
    upload.name = UPLOAD_STEP.to_owned();
    upload.condition =
        Some("always() && steps.prepare_terminal_evidence.outcome == 'success'".to_owned());
    document::step_to_yaml(JOB_ID, &upload, context, &[], false)
}

fn raw_step(
    name: &str,
    id: &str,
    script: &str,
    condition: Option<&str>,
) -> Result<Yaml, RenderError> {
    crate::expressions::check_name_content(name)?;
    crate::expressions::check_name_content(id)?;
    crate::steps::scan_for_private_subcommands(script)?;
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("id".to_owned(), Yaml::str(id)),
    ];
    fields.push(("shell".to_owned(), Yaml::str("bash")));
    if let Some(condition) = condition {
        crate::steps::scan_for_private_subcommands(condition)?;
        fields.push(("if".to_owned(), Yaml::str(condition)));
    }
    fields.push(("run".to_owned(), Yaml::str(script)));
    Ok(Yaml::Map(fields))
}

fn token_shell_step(
    name: &str,
    script: &str,
    env: &BTreeMap<String, String>,
    condition: Option<&str>,
) -> Yaml {
    let mut values: Vec<(String, Yaml)> = env
        .iter()
        .map(|(key, value)| (key.clone(), Yaml::str(value.clone())))
        .collect();
    values.push(("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")));
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("env".to_owned(), Yaml::Map(values)),
    ];
    if let Some(condition) = condition {
        fields.push(("if".to_owned(), Yaml::str(condition)));
    }
    fields.push(("run".to_owned(), Yaml::str(script)));
    Yaml::Map(fields)
}
