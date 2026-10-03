//! Canonical source and tool claims transported with completed build receipts.
//! These claims are metadata supplied by the caller, not authentication.

use eyre::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_CONTEXT_BYTES: usize = 64 * 1024;

/// Source and tool metadata frozen before a build starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptContext {
    pub schema: u32,
    pub source: Value,
    pub tool: Value,
}

impl ReceiptContext {
    /// Decode a bounded context whose JSON bytes are already canonical.
    pub fn from_canonical_json(text: &str) -> Result<Self> {
        if text.len() > MAX_CONTEXT_BYTES {
            eyre::bail!("receipt context exceeds 64 KiB");
        }
        let context: Self = serde_json::from_str(text)?;
        if context.canonical_json()? != text {
            eyre::bail!("receipt context JSON is not canonical");
        }
        Ok(context)
    }

    /// Check schema, metadata object shape, and serialized size.
    pub fn validate(&self) -> Result<()> {
        if self.schema != 1 {
            eyre::bail!("unsupported receipt context schema {}", self.schema);
        }
        for (name, value) in [("source", &self.source), ("tool", &self.tool)] {
            if !value.as_object().is_some_and(|object| !object.is_empty()) {
                eyre::bail!("receipt context {name} must be a nonempty object");
            }
        }
        if self.encode()?.len() > MAX_CONTEXT_BYTES {
            eyre::bail!("receipt context exceeds 64 KiB");
        }
        Ok(())
    }

    /// Encode compact JSON with recursively ordered metadata object keys.
    pub fn canonical_json(&self) -> Result<String> {
        self.validate()?;
        self.encode()
    }

    fn encode(&self) -> Result<String> {
        let normalized = Self {
            schema: self.schema,
            source: canonical_value(&self.source),
            tool: canonical_value(&self.tool),
        };
        Ok(serde_json::to_string(&normalized)?)
    }
}

fn canonical_value(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key.clone(), canonical_value(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_value).collect()),
        value => value.clone(),
    }
}

#[cfg(test)]
#[path = "receipt_context_tests.rs"]
mod tests;
