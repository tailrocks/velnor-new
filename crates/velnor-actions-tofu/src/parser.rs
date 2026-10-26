// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use crate::modules::{ModuleDecl, source_from_native};

pub const MAX_FILE_BYTES: u64 = 1_048_576;
pub const MAX_FILES_PER_UNIT: usize = 1024;
pub const MAX_NODES: usize = 50_000;
pub const MAX_DEPTH: usize = 64;
pub const MAX_VERSIONS_PER_FILE: usize = 64;
pub const MAX_DIAGNOSTIC_CHARS: usize = 160;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    TooLarge { bytes: u64 },
    Syntax { message: String },
    TooDeep,
    TooManyNodes,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockModel {
    pub kind: String,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileModel {
    pub blocks: Vec<BlockModel>,
    pub required_versions: Vec<String>,
    pub has_legacy_ref: bool,
    pub modules: Vec<ModuleDecl>,
    pub provider_hash_counts: Vec<crate::lockfile::ProviderHashCount>,
}

pub fn parse_native(text: &str) -> Result<FileModel, ParseError> {
    check_size(text)?;
    let body = hcl::parse(text).map_err(|err| ParseError::Syntax {
        message: single_line(&err.to_string()),
    })?;
    let mut walk = Walk::new();
    walk.body(&body, 0, true)?;
    Ok(walk.finish())
}

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

fn check_size(text: &str) -> Result<(), ParseError> {
    let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if bytes > MAX_FILE_BYTES {
        return Err(ParseError::TooLarge { bytes });
    }
    Ok(())
}

fn single_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(MAX_DIAGNOSTIC_CHARS)
        .collect()
}

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

fn is_token_char(byte: Option<&u8>) -> bool {
    byte.is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, b'_' | b'.' | b'-'))
}

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

pub(crate) struct Walk {
    nodes: usize,
    pub(crate) blocks: Vec<BlockModel>,
    pub(crate) modules: Vec<ModuleDecl>,
    versions: Vec<String>,
    legacy: bool,
    provider_hashes: Vec<crate::lockfile::ProviderHashCount>,
}

impl Walk {
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

    pub(crate) fn literal(&mut self, text: &str) {
        if !self.legacy && has_legacy_ref_text(&strip_template_spans(text)) {
            self.legacy = true;
        }
    }

    pub(crate) fn version(&mut self, text: &str) {
        if self.versions.len() < MAX_VERSIONS_PER_FILE {
            self.versions.push(text.to_owned());
        }
    }

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

    fn terraform_block(&mut self, block: &hcl::Block) {
        for attribute in block.body().attributes() {
            if attribute.key.as_str() == "required_version"
                && let hcl::Expression::String(literal) = &attribute.expr
            {
                self.version(literal);
            }
        }
    }

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
