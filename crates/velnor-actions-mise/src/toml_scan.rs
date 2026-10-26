// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::toml_parser::{InlineValue, Parser};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TomlValue {
    Str(String),
    Bool(bool),
    Num(String),
    Array(Vec<TomlValue>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TomlAssignment {
    pub(crate) path: Vec<String>,
    pub(crate) value: TomlValue,
    pub(crate) line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TomlDoc {
    pub(crate) sections: Vec<(Vec<String>, u32)>,
    pub(crate) array_sections: Vec<(Vec<String>, u32)>,
    pub(crate) assignments: Vec<TomlAssignment>,
}

impl TomlDoc {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TomlDiagnostic {
    pub(crate) line: u32,
    pub(crate) problem: String,
}

#[derive(Debug)]
pub(crate) struct TomlScope<'a> {
    pub(crate) defined: &'a mut BTreeSet<Vec<String>>,
    pub(crate) tables: &'a mut BTreeSet<Vec<String>>,
    pub(crate) array_tables: &'a mut BTreeSet<Vec<String>>,
    pub(crate) current_array_keys: &'a mut BTreeSet<Vec<String>>,
    pub(crate) in_array_table: bool,
}

impl TomlScope<'_> {
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

pub(crate) fn parse_toml(content: &str) -> Result<TomlDoc, TomlDiagnostic> {
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
