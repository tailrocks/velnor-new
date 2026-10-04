//! Late native comparison export; a useful export alone authorizes artifact upload.

use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, MbxExportDescriptor, SourceBoundHelper,
    SourceBoundOperation, Step, StepId,
};
use velnor_actions_mise::{
    ToolCatalog,
    catalog::mbx_action_authority::QualifiedMbxAction,
    catalog::qualification::{DistributionRequirement, DistributionTool, QualifiedDistribution},
};

use crate::OrchestratorError;

pub(crate) const EXPORT_ID: &str = "mbx-export";
pub(crate) const RESTORE_ID: &str = "mbx-restore";

/// Reconstruct executable authority from its catalog owner before binding data.
pub(crate) fn from_descriptor(
    descriptor: &MbxExportDescriptor,
    catalog: &ToolCatalog,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    descriptor.validate()?;
    if version != env!("CARGO_PKG_VERSION") {
        return Err(invalid("generator_version"));
    }
    let action = QualifiedMbxAction::require_comparison_export()?;
    if descriptor.action_sha != action.source_commit() {
        return Err(invalid("action_identity_changed"));
    }
    let host = crate::workloads::host_for_runner(&descriptor.runs_on)?;
    let distribution = QualifiedDistribution::require_for_generator(
        DistributionTool::Mbx,
        host,
        DistributionRequirement::MbxTransport,
    )?;
    distribution.required_install_plan()?;
    if descriptor.owner.version != distribution.version()
        || descriptor.owner.binary_sha256 != distribution.binary_sha256()
        || descriptor.owner.qualification_identity != distribution.qualification_digest()
        || descriptor.owner.source_sha != distribution.source_commit()
    {
        return Err(invalid("owner_identity_changed"));
    }
    let source = velnor_actions_contract::generated_source(version, BODY)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::MbxExport;
    let owner = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let invocation = HelperInvocation::compiled(
        owner,
        vec![
            distribution.required_installed_binary_path()?.to_owned(),
            descriptor.owner.binary_sha256.clone(),
        ],
        Vec::new(),
    )?;
    let mut environment = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    environment.extend(BTreeMap::from([
        (
            "MISE_DATA_DIR".to_owned(),
            velnor_actions_mise::runtime_paths::MISE_DATA_DIR.to_owned(),
        ),
        (
            "VELNOR_MBX_COMPARISON".to_owned(),
            descriptor.comparison_path()?,
        ),
        ("VELNOR_MBX_BUNDLE".to_owned(), descriptor.bundle_root()?),
        ("VELNOR_MBX_GROUP".to_owned(), descriptor.export_group()?),
        (
            "VELNOR_MBX_COMPARISON_SHA256".to_owned(),
            format!("${{{{ steps.{RESTORE_ID}.outputs.comparison-state-sha256 }}}}"),
        ),
    ]));
    Ok(CompiledSourceHelper::compiled(invocation, source)?.with_environment(environment))
}

/// Append only after every selected validation obligation has completed.
pub(crate) fn export_step(record: &CompiledSourceHelper) -> Result<Step, OrchestratorError> {
    if record.invocation().descriptor().operation() != SourceBoundOperation::MbxExport {
        return Err(invalid("foreign_helper"));
    }
    let mut step = velnor_actions_workflow_renderer::source_helper::source_helper_step(
        "Export useful MBX state",
        record,
        record.environment().clone(),
    )?;
    step.id = Some(StepId::new(EXPORT_ID)?);
    step.condition = Some("success()".to_owned());
    Ok(step)
}

/// Exact same-run artifact carries only a successfully published native directory.
pub(crate) fn upload_step(descriptor: &MbxExportDescriptor) -> Result<Step, OrchestratorError> {
    let mut step = velnor_actions_workflow_renderer::steps::upload_artifact_step(
        &descriptor.artifact_name()?,
        &descriptor.bundle_root()?,
    )?;
    step.name = "Upload useful MBX state".to_owned();
    step.condition = Some(format!(
        "success() && steps.{EXPORT_ID}.outcome == 'success' && steps.{EXPORT_ID}.outputs.emitted_bundle_useful_delta == 'true'"
    ));
    Ok(step)
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("mbx_export_{reason}"),
    }
}

// Only supported native commands and public report fields are consumed here.
// The baseline digest comes from the action's runner-held output, outside task files.
const BODY: &str = concat!(
    r#"set -euo pipefail
export PATH=/usr/bin:/bin:/usr/sbin:/sbin
/usr/bin/python3 -I -S - "$@" <<'VELNOR_MBX_EXPORT'
import hashlib, json, os, pathlib, re, stat, subprocess, sys

relative, expected_binary = sys.argv[1:]
data = pathlib.Path(os.environ['MISE_DATA_DIR'])
binary = data / relative
comparison = pathlib.Path(os.environ['VELNOR_MBX_COMPARISON'])
bundle = pathlib.Path(os.environ['VELNOR_MBX_BUNDLE'])
temp = pathlib.Path(os.environ['RUNNER_TEMP'])
expected_comparison = os.environ['VELNOR_MBX_COMPARISON_SHA256']

def owned_path(path, root, regular):
    if not path.is_absolute() or not root.is_absolute() or not path.is_relative_to(root):
        raise ValueError('foreign native export path')
    if path.resolve(strict=True) != path:
        raise ValueError('noncanonical native export path')
    for parent in (path, *path.parents):
        info = parent.lstat()
        if stat.S_ISLNK(info.st_mode):
            raise ValueError('linked native export path')
        if parent == root:
            break
    if regular and not stat.S_ISREG(path.lstat().st_mode):
        raise ValueError('native export input is not regular')

owned_path(binary, data, True)
owned_path(comparison, temp, True)
owned_path(bundle.parent, temp, False)
if bundle.exists() or bundle.is_symlink():
    raise ValueError('native export destination already exists')
if not re.fullmatch('[0-9a-f]{64}', expected_comparison):
    raise ValueError('missing original owner comparison digest')
if hashlib.sha256(binary.read_bytes()).hexdigest() != expected_binary:
    raise ValueError('owner executable changed before export')
if hashlib.sha256(comparison.read_bytes()).hexdigest() != expected_comparison:
    raise ValueError('owner comparison baseline changed before export')

"#,
    include_str!("mbx_export_report.py"),
    r#"
def native(arguments):
    result = subprocess.run([str(binary), *arguments], check=True, capture_output=True, text=True)
    sys.stderr.write(result.stderr)
    return decode_native_report(result.stdout)

verification = native(['cache', 'comparison-state', str(comparison), '--verify', '--json'])
if type(verification) is not dict or set(verification) != {'version', 'valid', 'empty'} or type(verification['version']) is not int or verification['version'] != 1 or verification['valid'] is not True or verification['empty'] is not False:
    raise ValueError('native comparison verification refused')
report = native(['cache', 'export', '--compare', str(comparison), '--group',
    os.environ['VELNOR_MBX_GROUP'], '--format', 'directory', '--json', str(bundle)])
emitted, semantic_digest, workspace_reason = export_report(report)
if emitted:
    owned_path(bundle, temp, False)
    if not bundle.is_dir():
        raise ValueError('native export bundle missing')
with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
    output.write('emitted_bundle_useful_delta=' + ('true' if emitted else 'false') + '\n')
    output.write('semantic_digest=' + semantic_digest + '\n')
    output.write('workspace_usefulness=unavailable\n')
    output.write('workspace_usefulness_reason=' + workspace_reason + '\n')
VELNOR_MBX_EXPORT
"#
);

#[cfg(test)]
#[path = "mbx_export_report_tests.rs"]
mod tests;
