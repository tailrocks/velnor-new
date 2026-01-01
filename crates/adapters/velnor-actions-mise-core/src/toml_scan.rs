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
pub enum TomlValue {
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
pub struct TomlAssignment {
    /// Full key path (section segments plus dotted key segments).
    pub path: Vec<String>,
    /// Parsed value.
    pub value: TomlValue,
    /// One-based line where the key starts.
    pub line: u32,
}

/// Parsed document: section headers plus flattened assignments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TomlDoc {
    /// Section-header paths with their one-based header lines.
    pub sections: Vec<(Vec<String>, u32)>,
    /// Flattened assignments in file order.
    pub assignments: Vec<TomlAssignment>,
}

impl TomlDoc {
    /// Assignments whose path equals `path`, in file order.
    #[must_use]
    pub fn find(&self, path: &[&str]) -> Vec<&TomlAssignment> {
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
pub struct TomlDiagnostic {
    /// One-based line of the failure.
    pub line: u32,
    /// Stable problem code.
    pub problem: String,
}

/// Parse `content` into sections plus flattened assignments.
///
/// # Errors
///
/// Active table state during TOML document assembly.
#[derive(Debug)]
pub(crate) struct TomlScope<'a> {
    /// Fully qualified scalar paths defined in document order.
    pub(crate) defined: &'a mut BTreeSet<Vec<String>>,
    /// Normal table paths declared via `[table]`.
    pub(crate) tables: &'a mut BTreeSet<Vec<String>>,
    /// Array table paths declared via `[[table]]`.
    pub(crate) array_tables: &'a mut BTreeSet<Vec<String>>,
    /// Relative key paths defined in the current array-table element.
    pub(crate) current_array_keys: &'a mut BTreeSet<Vec<String>>,
    /// Whether the current section is an array table.
    pub(crate) in_array_table: bool,
}

impl TomlScope<'_> {
    /// Check whether `path` is already occupied by a scalar or table.
    fn check_path(&self, path: &[String], line: u32) -> Result<(), TomlDiagnostic> {
        if self.defined.contains(path)
            || self.tables.contains(path)
            || self.array_tables.contains(path)
        {
            return Err(TomlDiagnostic {
                line,
                problem: "duplicate_key".to_owned(),
            });
        }
        for width in 1..path.len() {
            if self.defined.contains(&path[..width]) {
                return Err(TomlDiagnostic {
                    line,
                    problem: "duplicate_key".to_owned(),
                });
            }
        }
        Ok(())
    }
}

/// Parse `content` into sections plus flattened assignments.
///
/// # Errors
///
/// Returns [`TomlDiagnostic`] on any malformed or unsupported input.
pub fn parse_toml(content: &str) -> Result<TomlDoc, TomlDiagnostic> {
    let mut parser = Parser::new(content);
    let mut doc = TomlDoc::default();
    let mut defined = BTreeSet::new();
    let mut tables = BTreeSet::new();
    let mut array_tables = BTreeSet::new();
    let mut current_array_keys = BTreeSet::new();
    let mut section = Vec::new();
    let mut in_array_table = false;

    loop {
        parser.skip_trivia();
        if parser.at_end() {
            return Ok(doc);
        }
        if parser.peek() == Some('[') {
            let mut scope = TomlScope {
                defined: &mut defined,
                tables: &mut tables,
                array_tables: &mut array_tables,
                current_array_keys: &mut current_array_keys,
                in_array_table,
            };
            let (path, is_array) = parser.parse_header(&mut doc, &mut scope)?;
            section = path;
            in_array_table = is_array;
            current_array_keys.clear();
        } else {
            let mut scope = TomlScope {
                defined: &mut defined,
                tables: &mut tables,
                array_tables: &mut array_tables,
                current_array_keys: &mut current_array_keys,
                in_array_table,
            };
            parser.parse_entry(&section, &mut doc, &mut scope)?;
        }
    }
}

/// Flatten one parsed value into document assignments.
pub(crate) fn flatten_entry(
    path: Vec<String>,
    relative_key: Vec<String>,
    value: InlineValue,
    line: u32,
    doc: &mut TomlDoc,
    scope: &mut TomlScope<'_>,
) -> Result<(), TomlDiagnostic> {
    match value {
        InlineValue::Scalar(scalar) => {
            if scope.in_array_table {
                if scope.current_array_keys.contains(&relative_key) {
                    return Err(TomlDiagnostic {
                        line,
                        problem: "duplicate_key".to_owned(),
                    });
                }
                for width in 1..relative_key.len() {
                    if scope.current_array_keys.contains(&relative_key[..width]) {
                        return Err(TomlDiagnostic {
                            line,
                            problem: "duplicate_key".to_owned(),
                        });
                    }
                }
                scope.current_array_keys.insert(relative_key);
            } else {
                scope.check_path(&path, line)?;
                scope.defined.insert(path.clone());
            }
            doc.assignments.push(TomlAssignment {
                path,
                value: scalar,
                line,
            });
            Ok(())
        }
        InlineValue::Table(entries) => {
            for (nested_key, nested, key_line) in entries {
                let mut full = path.clone();
                full.extend(nested_key.clone());
                let mut full_rel = relative_key.clone();
                full_rel.extend(nested_key);
                flatten_entry(full, full_rel, nested, key_line, doc, scope)?;
            }
            Ok(())
        }
    }
}
