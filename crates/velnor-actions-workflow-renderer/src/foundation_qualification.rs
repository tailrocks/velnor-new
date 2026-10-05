//! Closed SOURCE qualification document serialization; no runtime authority.

use sha2::{Digest, Sha256};

use crate::{RenderError, RenderedFile, marker};

/// Fixed generated path; repository policy remains the orchestrator's decision.
pub const WORKFLOW_PATH: &str = ".github/workflows/foundation-qualification.yml";
const TEMPLATE: &str = include_str!("foundation_qualification_workflow.yml.in");
const TEMPLATE_SHA256: &str = "01acba73d0dd12facb25f0985e9141ab16f6fb8941fdc72f54aa10f31914b6a7";
const ACTION_MARKER: &str = "__FOUNDATION_ACTION_REF__";

/// Immutable syntax-validated action reference, carrying no publication trust.
#[derive(Debug)]
pub struct SourceActionReference(String);

impl SourceActionReference {
    /// Validate a neutral action reference pinned to a full lowercase commit SHA.
    /// # Errors
    /// Rejects malformed names, unsafe path segments, and mutable references.
    pub fn new(reference: &str) -> Result<Self, RenderError> {
        crate::steps::validate_uses(reference)?;
        let name = reference.split_once('@').map_or("", |(name, _)| name);
        if name.split('/').any(|part| matches!(part, "" | "." | "..")) {
            return Err(RenderError::BadActionRef("unsafe_action_path".to_owned()));
        }
        Ok(Self(reference.to_owned()))
    }
}

/// Serialize only the compiled Foundation SOURCE qualification document.
///
/// This accepts no caller template, path, YAML, body, policy, or runtime trust.
/// The orchestrator authenticates publication and supplies its explicit version.
/// # Errors
/// Rejects changed compiled template identity or invalid generator version.
pub fn render(action: &SourceActionReference, version: &str) -> Result<RenderedFile, RenderError> {
    render_compiled(action, version, TEMPLATE)
}

fn render_compiled(
    action: &SourceActionReference,
    version: &str,
    template: &str,
) -> Result<RenderedFile, RenderError> {
    if sha256_hex(template.as_bytes()) != TEMPLATE_SHA256
        || template.matches(ACTION_MARKER).count() != 1
    {
        return Err(RenderError::InvalidWorkflow(
            "foundation_qualification_source_template_identity".to_owned(),
        ));
    }
    let body = template.replace(ACTION_MARKER, &action.0);
    Ok(RenderedFile {
        path: WORKFLOW_PATH.to_owned(),
        bytes: marker::with_marker(version, &body)?,
    })
}

/// Private byte digest for this renderer's closed document identity checks.
fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
#[path = "foundation_qualification_tests.rs"]
mod tests;
