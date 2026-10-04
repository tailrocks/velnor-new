//! Fixed source owner for isolated, public, locked Cargo acquisition.

use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation,
};

use super::descriptor::RustSourceDescriptor;
use crate::OrchestratorError;

const PYTHON_BODY: &str = include_str!("source_producer_body.py");

/// Descriptor bytes are one hexadecimal literal, never interpolated shell code.
pub(crate) fn compiled_helper(
    descriptor: &RustSourceDescriptor,
    version: &str,
) -> Result<CompiledSourceHelper, OrchestratorError> {
    if version != env!("CARGO_PKG_VERSION") {
        return Err(OrchestratorError::Contract {
            problem: "rust_source_runtime_version_mismatch".to_owned(),
        });
    }
    let body = format!(
        "set -eu\nexport PATH=/usr/bin:/bin:/usr/sbin:/sbin\n\
         /usr/bin/python3 -I -S -B - \"$1\" <<'VELNOR_PY'\n{PYTHON_BODY}\nVELNOR_PY\n"
    );
    let source = velnor_actions_contract::generated_source(version, &body)?;
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let operation = SourceBoundOperation::RustSourceProducer;
    let owner = SourceBoundHelper::compiled(operation, operation.path(), &digest)?;
    let invocation = HelperInvocation::compiled(owner, vec![descriptor.hex_json()?], Vec::new())?;
    Ok(CompiledSourceHelper::compiled(invocation, source)?)
}
