//! Bounded HCL/JSON structural facade over `hcl-rs` + `serde_json` (S8).
//!
//! `hcl-rs` offers no byte/alloc/count caps (`from_reader` reads
//! unbounded), so every bound here is facade-owned: callers cap bytes
//! before calling (never `from_reader` on an untrusted stream), and
//! the facade budgets walked nodes plus nesting depth. Malformed
//! input yields a typed [`ParseError`], never an empty model. No
//! expression evaluation happens: only block shapes, `required_version`
//! literals, and string-literal scans cross the facade.

use std::fmt;

use crate::modules::{ModuleDecl, source_from_native};

/// Maximum bytes of one parsed file.
pub const MAX_FILE_BYTES: u64 = 1_048_576;
/// Maximum files analyzed for one unit.
pub const MAX_FILES_PER_UNIT: usize = 1024;
/// Maximum structural nodes walked per file.
pub const MAX_NODES: usize = 50_000;
/// Maximum nesting depth walked per file.
pub const MAX_DEPTH: usize = 64;
/// Maximum `required_version` literals kept per file.
pub const MAX_VERSIONS_PER_FILE: usize = 64;
/// Maximum characters kept in one diagnostic.
pub const MAX_DIAGNOSTIC_CHARS: usize = 160;

/// Typed structural parse failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Input exceeds [`MAX_FILE_BYTES`].
    TooLarge {
        /// Observed byte length.
        bytes: u64,
    },
    /// Parser rejected the input (single-line, capped message).
    Syntax {
        /// First line of the parser message, capped.
        message: String,
    },
    /// Nesting exceeds [`MAX_DEPTH`].
    TooDeep,
    /// Walked nodes exceed [`MAX_NODES`].
    TooManyNodes,
    /// JSON root is not an object.
    NotObject,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(f, "too_large:{bytes}"),
            Self::Syntax { message } => write!(f, "syntax:{message}"),
            Self::TooDeep => write!(f, "too_deep"),
            Self::TooManyNodes => write!(f, "too_many_nodes"),
            Self::NotObject => write!(f, "json_root_must_be_object"),
        }
    }
}

impl std::error::Error for ParseError {}

/// One top-level block identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockModel {
    /// Block type (`variable`, `resource`, ...).
    pub kind: String,
    /// Block labels in order.
    pub labels: Vec<String>,
}

/// Structural model of one config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileModel {
    /// Top-level blocks in file order.
    pub blocks: Vec<BlockModel>,
    /// `required_version` string literals in encounter order.
    pub required_versions: Vec<String>,
    /// Whether any string literal carries a legacy `terraform` token.
    pub has_legacy_ref: bool,
    /// Top-level `module` declarations with parsed sources, in file order.
    pub modules: Vec<ModuleDecl>,
    /// Top-level `provider` hash counts, in file order (native only:
    /// lockfiles are HCL, so the JSON walk leaves this empty).
    pub provider_hash_counts: Vec<crate::lockfile::ProviderHashCount>,
}

/// Parse one native (`.tf`/`.tofu`) file structurally.
///
/// # Errors
///
/// Returns [`ParseError`] for oversize, syntactically invalid, or
/// over-budget input.
pub fn parse_native(text: &str) -> Result<FileModel, ParseError> {
    check_size(text)?;
    let body = hcl::parse(text).map_err(|err| ParseError::Syntax {
        message: single_line(&err.to_string()),
    })?;
    let mut walk = Walk::new();
    walk.body(&body, 0, true)?;
    Ok(walk.finish())
}

/// Parse one JSON (`.tf.json`/`.tofu.json`) file structurally.
///
/// # Errors
///
/// Returns [`ParseError`] for oversize, syntactically invalid,
/// non-object-root, or over-budget input.
pub fn parse_json(text: &str) -> Result<FileModel, ParseError> {
    check_size(text)?;
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|err| ParseError::Syntax {
            message: single_line(&err.to_string()),
        })?;
    let root = value.as_object().ok_or(ParseError::NotObject)?;
    let mut walk = Walk::new();
    walk.json_root(root)?;
    Ok(walk.finish())
}

/// Reject input over the byte cap.
fn check_size(text: &str) -> Result<(), ParseError> {
    let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if bytes > MAX_FILE_BYTES {
        return Err(ParseError::TooLarge { bytes });
    }
    Ok(())
}

/// First line of `text`, capped to [`MAX_DIAGNOSTIC_CHARS`].
fn single_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect()
}

/// True when `text` carries a legacy `terraform` token.
///
/// The token must stand alone: neither side may be ASCII
/// alphanumeric, `_`, `.`, or `-`. That admits CLI invocations
/// (`terraform plan`) while excluding shared-registry hosts
/// (`registry.terraform.io`), native identifiers
/// (`terraform.workspace` never reaches here: only string literals
/// are scanned), and suffixed names (`terraform_data`).
#[must_use]
pub fn has_legacy_ref_text(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 9 <= bytes.len() {
        if &bytes[index..index + 9] == b"terraform"
            && !is_token_char(index.checked_sub(1).and_then(|pos| bytes.get(pos)))
            && !is_token_char(bytes.get(index + 9))
        {
            return true;
        }
        index += 1;
    }
    false
}

/// True when the optional neighbor byte continues a token.
fn is_token_char(byte: Option<&u8>) -> bool {
    byte.is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, b'_' | b'.' | b'-'))
}

/// Strip `${...}` interpolations and `%{...}` directives from template text.
///
/// Brace nesting is honored; an unbalanced opener strips to the end.
/// Only literal spans survive, so identifier references inside
/// interpolations never scan as legacy refs.
#[must_use]
pub fn strip_template_spans(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut kept = String::with_capacity(text.len());
    let mut literal_start = 0;
    let mut index = 0;
    while index < bytes.len() {
        let opens = index + 1 < bytes.len()
            && bytes[index + 1] == b'{'
            && (bytes[index] == b'$' || bytes[index] == b'%');
        if opens {
            kept.push_str(&text[literal_start..index]);
            index = skip_span(bytes, index + 2);
            literal_start = index;
        } else {
            index += 1;
        }
    }
    kept.push_str(&text[literal_start..]);
    kept
}

/// Index just past the span opened before `index` (nesting-aware).
fn skip_span(bytes: &[u8], mut index: usize) -> usize {
    let mut depth = 1;
    while index < bytes.len() && depth > 0 {
        if bytes[index] == b'{' {
            depth += 1;
        } else if bytes[index] == b'}' {
            depth -= 1;
        }
        index += 1;
    }
    index
}

/// Budgeted structural walk shared by both dialects.
pub(crate) struct Walk {
    /// Nodes charged so far.
    nodes: usize,
    /// Blocks collected.
    pub(crate) blocks: Vec<BlockModel>,
    /// Module declarations collected.
    pub(crate) modules: Vec<ModuleDecl>,
    /// Version literals collected.
    versions: Vec<String>,
    /// Legacy token observed.
    legacy: bool,
    /// Provider hash counts collected (native walk only).
    provider_hashes: Vec<crate::lockfile::ProviderHashCount>,
}

impl Walk {
    /// Empty walk.
    fn new() -> Self {
        Self {
            nodes: 0,
            blocks: Vec::new(),
            modules: Vec::new(),
            versions: Vec::new(),
            legacy: false,
            provider_hashes: Vec::new(),
        }
    }

    /// Finished model.
    fn finish(self) -> FileModel {
        FileModel {
            blocks: self.blocks,
            required_versions: self.versions,
            has_legacy_ref: self.legacy,
            modules: self.modules,
            provider_hash_counts: self.provider_hashes,
        }
    }

    pub(crate) fn tick(&mut self) -> Result<(), ParseError> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(ParseError::TooManyNodes);
        }
        Ok(())
    }

    /// Record one string literal for the legacy scan.
    pub(crate) fn literal(&mut self, text: &str) {
        if !self.legacy && has_legacy_ref_text(&strip_template_spans(text)) {
            self.legacy = true;
        }
    }

    /// Record one `required_version` literal, capped.
    pub(crate) fn version(&mut self, text: &str) {
        if self.versions.len() < MAX_VERSIONS_PER_FILE {
            self.versions.push(text.to_owned());
        }
    }

    /// Walk one HCL body; `top` marks the file top level.
    fn body(&mut self, body: &hcl::Body, depth: usize, top: bool) -> Result<(), ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::TooDeep);
        }
        for attribute in body.attributes() {
            self.tick()?;
            self.expression(&attribute.expr, depth)?;
        }
        for block in body.blocks() {
            self.tick()?;
            if top {
                self.blocks.push(BlockModel {
                    kind: block.identifier().to_owned(),
                    labels: block
                        .labels()
                        .iter()
                        .map(|label| label.as_str().to_owned())
                        .collect(),
                });
                if block.identifier() == "terraform" {
                    self.terraform_block(block);
                }
                if block.identifier() == "provider" {
                    self.provider_hashes
                        .push(crate::lockfile::provider_hash_count(block));
                }
                if block.identifier() == "module" {
                    self.modules.push(ModuleDecl {
                        name: block
                            .labels()
                            .first()
                            .map_or_else(String::new, |label| label.as_str().to_owned()),
                        source: source_from_native(block),
                    });
                }
            }
            self.body(block.body(), depth + 1, false)?;
        }
        Ok(())
    }

    /// Collect `required_version` string literals from one `terraform` block.
    fn terraform_block(&mut self, block: &hcl::Block) {
        for attribute in block.body().attributes() {
            if attribute.key.as_str() == "required_version"
                && let hcl::Expression::String(literal) = &attribute.expr
            {
                self.version(literal);
            }
        }
    }

    /// Walk one HCL expression for string literals.
    fn expression(&mut self, expr: &hcl::Expression, depth: usize) -> Result<(), ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::TooDeep);
        }
        self.tick()?;
        match expr {
            hcl::Expression::String(literal) => self.literal(literal),
            hcl::Expression::Array(items) => {
                for item in items {
                    self.expression(item, depth + 1)?;
                }
            }
            hcl::Expression::Object(object) => {
                for (_, value) in object {
                    self.expression(value, depth + 1)?;
                }
            }
            hcl::Expression::TemplateExpr(template) => self.literal(&template.to_string()),
            hcl::Expression::Parenthesis(inner) => self.expression(inner, depth + 1)?,
            hcl::Expression::Conditional(conditional) => {
                self.expression(&conditional.cond_expr, depth + 1)?;
                self.expression(&conditional.true_expr, depth + 1)?;
                self.expression(&conditional.false_expr, depth + 1)?;
            }
            hcl::Expression::FuncCall(call) => {
                for argument in &call.args {
                    self.expression(argument, depth + 1)?;
                }
            }
            hcl::Expression::Operation(operation) => match operation.as_ref() {
                hcl::expr::Operation::Unary(unary) => self.expression(&unary.expr, depth + 1)?,
                hcl::expr::Operation::Binary(binary) => {
                    self.expression(&binary.lhs_expr, depth + 1)?;
                    self.expression(&binary.rhs_expr, depth + 1)?;
                }
            },
            hcl::Expression::ForExpr(for_expr) => {
                self.expression(&for_expr.collection_expr, depth + 1)?;
                self.expression(&for_expr.value_expr, depth + 1)?;
                if let Some(key) = &for_expr.key_expr {
                    self.expression(key, depth + 1)?;
                }
                if let Some(condition) = &for_expr.cond_expr {
                    self.expression(condition, depth + 1)?;
                }
            }
            hcl::Expression::Traversal(traversal) => self.expression(&traversal.expr, depth + 1)?,
            _ => {}
        }
        Ok(())
    }

    /// Walk one JSON object root (implemented in `parser_json`).
    fn json_root(
        &mut self,
        root: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<(), ParseError> {
        for (kind, value) in root {
            self.tick()?;
            self.json_block(kind, value, 0)?;
        }
        Ok(())
    }
}
