//! Closed SDK cache-version and numbered quarantine mapping from admitted recipes.
use crate::OrchestratorError;
use serde_json::{Value, json};
use velnor_actions_contract::{StepKind, ToolCacheDomain};
use velnor_actions_workflow_renderer::cache_producer_workflow::CacheProducerRecipe;

const PREFIX: &str = "${{ runner.temp }}/velnor/";
const MAX_ROOTS: usize = 32;
const MAX_PATH_BYTES: usize = 4_096;
const MAX_TRANSPORT_BYTES: usize = 32_768;
const EVIDENCE_FILES: [&str; 3] = ["manifest.json", "predicate.json", "bundle.sigstore.json"];

/// Compiler-owned mapping; neither transport JSON nor an archive can construct it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheReceiptTransportLayout {
    sdk_paths: String,
    payload_roots: Vec<String>,
    optional_roots: Vec<String>,
    evidence_root: String,
}

impl CacheReceiptTransportLayout {
    /// Exact SDK path bytes used by both receipt save and receipt restore.
    #[must_use]
    pub fn transport_paths(&self) -> &str {
        &self.sdk_paths
    }

    /// Logical roots in original SDK order; evidence is deliberately excluded.
    #[must_use]
    pub fn payload_roots(&self) -> &[String] {
        &self.payload_roots
    }

    /// Missing payload roots admitted by the original producer's role.
    #[must_use]
    pub fn optional_roots(&self) -> &[String] {
        &self.optional_roots
    }

    /// Exact numbered payload roots; the final evidence index is separate.
    #[must_use]
    pub fn payload_indices(&self) -> Vec<usize> {
        (0..self.payload_roots.len()).collect()
    }

    /// Evidence always occupies the final SDK root.
    #[must_use]
    pub fn evidence_index(&self) -> usize {
        self.payload_roots.len()
    }

    /// Evidence namespace outside every signed payload traversal.
    #[must_use]
    pub fn evidence_root(&self) -> &str {
        &self.evidence_root
    }

    /// Closed regular-file inventory; no directories, links or extra files.
    #[must_use]
    pub fn evidence_files(&self) -> &[&'static str] {
        &EVIDENCE_FILES
    }

    /// Embed in the sealed compiled policy; this projection grants no authority.
    #[must_use]
    pub fn descriptor(&self) -> Value {
        json!({
            "schema": 1, "sdk_paths": self.sdk_paths.split('\n').collect::<Vec<_>>(),
            "payload_roots": self.payload_roots, "optional_roots": self.optional_roots,
            "payload_indices": self.payload_indices(), "evidence_index": self.evidence_index(),
            "evidence_root": self.evidence_root, "evidence_files": EVIDENCE_FILES,
        })
    }
}

/// Called only after independent source-owner admission by the receipt factory.
pub(crate) fn derive(
    recipe: &CacheProducerRecipe,
) -> Result<CacheReceiptTransportLayout, OrchestratorError> {
    let job = recipe.original();
    let save = match (&job.tool_producer, &job.source_producer, &job.mbx_producer) {
        (Some(meta), None, None) => &meta.save_step,
        (None, Some(meta), None) => &meta.save_step,
        (None, None, Some(meta)) => &meta.save_step,
        _ => return Err(invalid("requires_one_producer")),
    };
    let step = job
        .steps
        .iter()
        .find(|step| step.id.as_ref() == Some(save))
        .ok_or_else(|| invalid("missing_save"))?;
    let StepKind::Action { with, .. } = &step.kind else {
        return Err(invalid("save_not_action"));
    };
    let paths = with.get("path").ok_or_else(|| invalid("missing_paths"))?;
    let digest = velnor_actions_contract::cache_producer_recipe_digest(job)?;
    let optional = if job
        .tool_producer
        .as_ref()
        .is_some_and(|meta| meta.descriptor.domain == ToolCacheDomain::Full)
    {
        vec!["cargo/.crates.toml".into(), "cargo/.crates2.json".into()]
    } else {
        Vec::new()
    };
    build(paths, &digest, optional)
}

/// Reconstruct from admitted recipe, then compare the frozen source-holder policy.
/// JSON is comparison evidence only; it never supplies a layout or path mapping.
pub(crate) fn derive_for_descriptor(
    recipe: &CacheProducerRecipe,
    producer: &Value,
) -> Result<CacheReceiptTransportLayout, OrchestratorError> {
    let layout = derive(recipe)?;
    let digest = velnor_actions_contract::cache_producer_recipe_digest(recipe.original())?;
    validate_frozen_descriptor(&layout, &digest, producer)?;
    Ok(layout)
}

fn validate_frozen_descriptor(
    layout: &CacheReceiptTransportLayout,
    digest: &str,
    producer: &Value,
) -> Result<(), OrchestratorError> {
    if producer.get("transport_layout") != Some(&layout.descriptor())
        || producer.get("recipe_sha256").and_then(Value::as_str) != Some(digest)
        || producer.get("allowed_roots") != Some(&json!(layout.payload_roots()))
        || producer.get("optional_roots") != Some(&json!(layout.optional_roots()))
    {
        return Err(invalid("frozen_descriptor_mismatch"));
    }
    Ok(())
}

fn build(
    paths: &str,
    digest: &str,
    optional_roots: Vec<String>,
) -> Result<CacheReceiptTransportLayout, OrchestratorError> {
    if paths.is_empty()
        || paths.len() > MAX_TRANSPORT_BYTES
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid("bounds_or_digest"));
    }
    let payload_roots = paths
        .split('\n')
        .map(|path| {
            let root = path
                .strip_prefix(PREFIX)
                .ok_or_else(|| invalid("foreign_root"))?;
            validate_root(root)?;
            Ok(root.to_owned())
        })
        .collect::<Result<Vec<_>, OrchestratorError>>()?;
    if payload_roots.len() > MAX_ROOTS
        || payload_roots
            .iter()
            .any(|left| payload_roots.iter().filter(|right| *right == left).count() != 1)
        || payload_roots.iter().any(|left| {
            payload_roots
                .iter()
                .any(|right| right.starts_with(&format!("{left}/")))
        })
        || optional_roots
            .iter()
            .any(|root| !payload_roots.contains(root))
    {
        return Err(invalid("ambiguous_roots"));
    }
    let receipt_path = velnor_actions_contract::cache_receipt_root(digest);
    let sdk_paths = format!("{paths}\n{receipt_path}");
    if sdk_paths.len() > MAX_TRANSPORT_BYTES {
        return Err(invalid("transport_bound"));
    }
    Ok(CacheReceiptTransportLayout {
        sdk_paths,
        payload_roots,
        optional_roots,
        evidence_root: format!("cache-receipts/{digest}"),
    })
}

fn validate_root(root: &str) -> Result<(), OrchestratorError> {
    if root.is_empty()
        || root.len() > MAX_PATH_BYTES
        || root
            .bytes()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'/' | b'.' | b'_' | b'-'))
        || root.split('/').any(|part| matches!(part, "" | "." | ".."))
        || root == "cache-receipts"
        || root.starts_with("cache-receipts/")
    {
        return Err(invalid("unsafe_or_evidence_root"));
    }
    Ok(())
}

fn invalid(reason: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: format!("cache_receipt_transport_layout_{reason}"),
    }
}

#[cfg(test)]
#[path = "cache_receipt_transport_layout_tests.rs"]
mod tests;
