//! Structural TOML-subset scanner with line diagnostics.
//!
//! Private engine behind the Mise adapter's structural inspections (cargo
//! wrappers, Nextest config). Tokenization lives in [`crate::toml_parser`];
//! this module owns the document types plus assembly into flattened
//! assignments. Anything outside the subset is a [`TomlDiagnostic`]
//! naming the line, never a guess. Consumers match exact paths, so a
//! comment or string merely mentioning a key is never evidence.

use std::collections::BTreeSet;

use crate::toml_parser::{InlineValue, Parser};

/// One parsed scalar or array value (inline tables flatten on parse).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TomlValue {
    /// Quoted string value.
    Str(String),
    /// Boolean value.
    Bool(bool),
    /// Opaque bare value (number, datetime, or other scalar token).
    Num(String),
    /// Array value, kept composite (never flattened to indexed paths).
    Array(Vec<TomlValue>),
}

/// One flattened `section.dotted.key = value` assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TomlAssignment {
    /// Full key path (section segments plus dotted key segments).
    pub(crate) path: Vec<String>,
    /// Parsed value.
    pub(crate) value: TomlValue,
    /// One-based line where the key starts.
    pub(crate) line: u32,
}

/// Parsed document: section headers plus flattened assignments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TomlDoc {
    /// Section-header paths with their one-based header lines.
    pub(crate) sections: Vec<(Vec<String>, u32)>,
    /// Flattened assignments in file order.
    pub(crate) assignments: Vec<TomlAssignment>,
}

impl TomlDoc {
    /// Assignments whose path equals `path`, in file order.
    pub(crate) fn find(&self, path: &[&str]) -> Vec<&TomlAssignment> {
        self.assignments
            .iter()
            .filter(|item| {
                item.path
                    .iter()
                    .map(String::as_str)
                    .eq(path.iter().copied())
            })
            .collect()
    }
}

/// Strict parse failure naming the offending line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TomlDiagnostic {
    /// One-based line of the failure.
    pub(crate) line: u32,
    /// Stable problem code.
    pub(crate) problem: String,
}

/// Parse `content` into sections plus flattened assignments.
///
/// # Errors
///
/// Returns [`TomlDiagnostic`] on any malformed or unsupported input.
pub(crate) fn parse_toml(content: &str) -> Result<TomlDoc, TomlDiagnostic> {
    let mut parser = Parser::new(content);
    let mut doc = TomlDoc::default();
    let mut defined: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut tables: BTreeSet<Vec<String>> = BTreeSet::new();
    let mut section: Vec<String> = Vec::new();
    loop {
        parser.skip_trivia();
        if parser.at_end() {
            return Ok(doc);
        }
        if parser.peek() == Some('[') {
            section = parser.parse_header(&mut doc, &defined)?;
            tables.insert(section.clone());
        } else {
            parser.parse_entry(&section, &mut doc, &mut defined, &tables)?;
        }
    }
}

/// Flatten one parsed value into document assignments.
pub(crate) fn flatten_value(
    path: Vec<String>,
    value: InlineValue,
    line: u32,
    doc: &mut TomlDoc,
    defined: &mut BTreeSet<Vec<String>>,
    tables: &BTreeSet<Vec<String>>,
) -> Result<(), TomlDiagnostic> {
    match value {
        InlineValue::Scalar(scalar) => {
            check_path(&path, line, defined, tables)?;
            defined.insert(path.clone());
            doc.assignments.push(TomlAssignment {
                path,
                value: scalar,
                line,
            });
            Ok(())
        }
        InlineValue::Table(entries) => {
            for (key, nested, key_line) in entries {
                let mut full = path.clone();
                full.extend(key);
                flatten_value(full, nested, key_line, doc, defined, tables)?;
            }
            Ok(())
        }
    }
}

/// Reject redefined or scalar-shadowed assignment paths.
fn check_path(
    path: &[String],
    line: u32,
    defined: &BTreeSet<Vec<String>>,
    tables: &BTreeSet<Vec<String>>,
) -> Result<(), TomlDiagnostic> {
    if defined.contains(path) || tables.contains(path) {
        return Err(TomlDiagnostic {
            line,
            problem: "duplicate_key".to_owned(),
        });
    }
    for width in 1..path.len() {
        if defined.contains(&path[..width]) {
            return Err(TomlDiagnostic {
                line,
                problem: "duplicate_key".to_owned(),
            });
        }
    }
    Ok(())
}
