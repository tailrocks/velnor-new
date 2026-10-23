//! Narrow data-only adapter for the legacy fetch-word scan.

use std::path::Path;

#[path = "impl_orch_evidence_spec_legacy_projection.rs"]
mod projection;
#[path = "impl_orch_evidence_spec_legacy_source.rs"]
mod source;
#[path = "impl_orch_evidence_spec_legacy_syntax.rs"]
mod syntax;
#[path = "impl_orch_evidence_spec_legacy_tests.rs"]
mod tests;

const SOURCE_FILE: &str = "mise_task_suites_emit.rs";

pub(crate) fn mask_resolved_evidence_spec_for_legacy_scan(
    path: &Path,
    orchestrator_src: &Path,
    text: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let source_root = orchestrator_src.canonicalize()?;
    let source_path = path.canonicalize()?;
    if source_path != source_root.join(SOURCE_FILE) {
        return Ok(text.to_owned());
    }
    let file = syn::parse_file(text)?;
    let identifiers = syntax::passive_spec_identifiers(&file, text)?;
    source::mask_exact_identifiers(text, identifiers).map_err(Into::into)
}
