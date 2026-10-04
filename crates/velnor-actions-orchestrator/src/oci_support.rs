//! Fixed generated OCI script inventory and source emission.

use super::OCI_WORKFLOW_PATH;
use std::collections::BTreeMap;
use velnor_actions_native::oci::ENTRY_PATH;
use velnor_actions_workflow_renderer::{RenderError, RenderedFile, marker};

/// Complete OCI-owned output family, including the shared admission dependency.
pub(crate) const OCI_DELIVERY_TREE_PATHS: &[&str] = &[
    ".github/velnor/oci_delivery.sh",
    ENTRY_PATH,
    ".github/velnor/oci_digest.py",
    ".github/velnor/oci_digest_parts.py",
    ".github/velnor/oci_index_receipt.py",
    ".github/velnor/oci_archive.py",
    ".github/velnor/oci_registry.py",
    ".github/velnor/oci_platform_publish.py",
    ".github/velnor/release_admission.py",
    OCI_WORKFLOW_PATH,
];

/// Emit fixed source scripts from the generator, with deterministic markers.
/// # Errors
/// Rejects an invalid generator version marker.
pub(in crate::delivery_emit) fn render_oci_support_files(
    version: &str,
) -> Result<Vec<RenderedFile>, RenderError> {
    let sources =
        velnor_actions_native::oci::support_sources(version).map_err(RenderError::Contract)?;
    let mut files = sources
        .files()
        .iter()
        .map(|file| RenderedFile {
            path: file.path().to_owned(),
            bytes: file.source().to_owned(),
        })
        .collect::<Vec<_>>();
    files.push(RenderedFile {
        path: crate::release_emit::release_admission::ADMISSION_PATH.to_owned(),
        bytes: marker::with_marker(version, crate::release_emit::release_admission::source())?,
    });
    embed_entry_closure(&mut files, version)?;
    let entry = files
        .iter()
        .find(|file| file.path == ENTRY_PATH)
        .ok_or_else(|| RenderError::InvalidWorkflow("oci_compiled_entry_missing".to_owned()))?;
    let wrapper = format!(
        "set -euo pipefail\npython3 -I -S - \"$@\" <<'VELNOR_OCI_COMPILED_BODY'\n{}\nVELNOR_OCI_COMPILED_BODY\n",
        entry.bytes
    );
    files.push(RenderedFile {
        path: ".github/velnor/oci_delivery.sh".to_owned(),
        bytes: marker::with_marker(version, &wrapper)?,
    });
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn embed_entry_closure(files: &mut [RenderedFile], version: &str) -> Result<(), RenderError> {
    let sources = files
        .iter()
        .filter(|file| file.path != ENTRY_PATH)
        .map(|file| {
            let name = file
                .path
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_suffix(".py"));
            name.map(|name| (name.to_owned(), file.bytes.clone()))
                .ok_or_else(|| RenderError::InvalidWorkflow("oci_compiled_source_path".to_owned()))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let encoded = serde_json::to_string(&sources)
        .map_err(|_| RenderError::InvalidWorkflow("oci_compiled_source_encoding".to_owned()))?;
    let entry = files
        .iter_mut()
        .find(|file| file.path == ENTRY_PATH)
        .ok_or_else(|| RenderError::InvalidWorkflow("oci_compiled_entry_missing".to_owned()))?;
    let (_, template) = entry
        .bytes
        .split_once('\n')
        .ok_or_else(|| RenderError::InvalidWorkflow("oci_compiled_entry_marker".to_owned()))?;
    entry.bytes = marker::with_marker(
        version,
        &format!("OCI_COMPILED_SOURCES = {encoded}\n{template}"),
    )?;
    Ok(())
}
