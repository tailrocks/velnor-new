//! Ordered mapping and anchor handling for the workflow step reader.

use std::collections::{BTreeMap, BTreeSet};

use super::anchor_names;
use super::indent;

#[derive(Default)]
pub(super) struct ParsedStepFields {
    pub(super) fields: Vec<(String, String)>,
    pub(super) env_fields: Vec<(String, String)>,
    current_key: String,
    env_anchor: Option<String>,
    env_alias: Option<String>,
    run_anchor: Option<String>,
    pub(super) run_scalar: Option<String>,
    run_block_scalar: bool,
    run_block: Vec<String>,
    pub(super) unexpected_nested: bool,
    pub(super) invalid_anchor: bool,
}

impl ParsedStepFields {
    pub(super) fn new(unexpected_nested: bool) -> Self {
        Self {
            unexpected_nested,
            ..Self::default()
        }
    }

    pub(super) fn read_line(
        &mut self,
        index: usize,
        line: &str,
        head: &str,
        layout: MappingLayout,
        scalar_anchors: &mut BTreeMap<String, String>,
        duplicate_anchors: &BTreeSet<String>,
    ) {
        let Some(content) =
            self.line_content(index, line, head, layout, scalar_anchors, duplicate_anchors)
        else {
            return;
        };
        self.record_field(content, scalar_anchors, duplicate_anchors);
    }

    fn line_content<'a>(
        &mut self,
        index: usize,
        line: &'a str,
        head: &'a str,
        layout: MappingLayout,
        scalar_anchors: &mut BTreeMap<String, String>,
        duplicate_anchors: &BTreeSet<String>,
    ) -> Option<&'a str> {
        let line_indent = indent(line);
        if index == 0 && layout.head_field {
            Some(head)
        } else if line_indent == layout.mapping_indent {
            Some(line.trim())
        } else if self.current_key == "env"
            && self.env_alias.is_some()
            && line_indent > layout.mapping_indent
        {
            let content = line.trim_start();
            if !content.is_empty() && !content.starts_with('#') {
                self.invalid_anchor = true;
                self.unexpected_nested = true;
            }
            None
        } else if self.current_key == "env"
            && line_indent == layout.mapping_indent + layout.mapping_width
        {
            let Some((key, value)) = line.trim().split_once(':') else {
                self.unexpected_nested = true;
                return None;
            };
            if self.env_alias.is_some() {
                self.unexpected_nested = true;
            } else {
                let value =
                    self.scalar_value(value.trim_start(), scalar_anchors, duplicate_anchors);
                self.env_fields.push((key.to_owned(), value));
            }
            None
        } else if line_indent > layout.mapping_indent
            && self.current_key == "run"
            && self.run_block_scalar
        {
            self.run_block.push(line.trim_start().to_owned());
            None
        } else {
            if !line.trim().is_empty() && line_indent > layout.base {
                self.unexpected_nested = true;
            }
            None
        }
    }

    fn record_field(
        &mut self,
        content: &str,
        scalar_anchors: &mut BTreeMap<String, String>,
        duplicate_anchors: &BTreeSet<String>,
    ) {
        let Some((key, value)) = content.split_once(':') else {
            return;
        };
        let key = key.trim().to_owned();
        let mut value = value.trim_start().to_owned();
        if key == "env" {
            if let Some(raw) = value.strip_prefix('&') {
                match anchor_names::anchor_name_after_ampersand(raw) {
                    Some(anchor) if !duplicate_anchors.contains(anchor) => {
                        self.env_anchor = Some(anchor.to_owned());
                    }
                    _ => self.invalid_anchor = true,
                }
            } else if value.starts_with('*') {
                match anchor_names::alias_name_after_asterisk(&value) {
                    Some(alias) if !duplicate_anchors.contains(alias) => {
                        self.env_alias = Some(alias.to_owned());
                    }
                    _ => self.invalid_anchor = true,
                }
            }
        } else if key == "run" {
            self.record_run(&mut value, scalar_anchors, duplicate_anchors);
        } else if value.starts_with('*') || value.starts_with('&') {
            value = self.scalar_value(&value, scalar_anchors, duplicate_anchors);
        }
        self.current_key.clone_from(&key);
        self.fields.push((key, value));
    }

    fn record_run(
        &mut self,
        value: &mut String,
        scalar_anchors: &mut BTreeMap<String, String>,
        duplicate_anchors: &BTreeSet<String>,
    ) {
        self.run_block_scalar = false;
        if value.starts_with('*') {
            if let Some(alias) = anchor_names::alias_name_after_asterisk(value)
                && !duplicate_anchors.contains(alias)
                && let Some(scalar) = scalar_anchors.get(alias)
            {
                self.run_scalar = Some(scalar.clone());
                value.clone_from(scalar);
            } else {
                self.invalid_anchor = true;
            }
        } else if let Some(declaration) = value.strip_prefix('&') {
            if let Some((anchor, scalar)) = anchor_names::anchor_value_after_ampersand(declaration)
                && !duplicate_anchors.contains(anchor)
            {
                self.run_anchor = Some(anchor.to_owned());
                let scalar = scalar.to_owned();
                self.record_run_scalar(&scalar, value);
            } else {
                self.invalid_anchor = true;
            }
        } else if is_block_scalar(value) {
            self.run_block_scalar = true;
            self.run_scalar = Some(String::new());
        } else {
            self.run_scalar = Some(value.clone());
        }
    }

    fn record_run_scalar(&mut self, scalar: &str, value: &mut String) {
        if is_block_scalar(scalar) {
            self.run_block_scalar = true;
            self.run_scalar = Some(String::new());
        } else {
            let scalar = scalar.to_owned();
            self.run_scalar = Some(scalar.clone());
            *value = scalar;
        }
    }

    fn scalar_value(
        &mut self,
        value: &str,
        scalar_anchors: &mut BTreeMap<String, String>,
        duplicate_anchors: &BTreeSet<String>,
    ) -> String {
        if value.starts_with('*') {
            if let Some(alias) = anchor_names::alias_name_after_asterisk(value)
                && !duplicate_anchors.contains(alias)
                && let Some(scalar) = scalar_anchors.get(alias)
            {
                return scalar.clone();
            }
            self.invalid_anchor = true;
        } else if let Some(declaration) = value.strip_prefix('&') {
            if let Some((anchor, scalar)) = anchor_names::anchor_value_after_ampersand(declaration)
                && !duplicate_anchors.contains(anchor)
                && !is_block_scalar(scalar)
            {
                scalar_anchors.insert(anchor.to_owned(), scalar.to_owned());
                return scalar.to_owned();
            }
            self.invalid_anchor = true;
        }
        value.to_owned()
    }

    pub(super) fn finish(
        &mut self,
        map_anchors: &mut BTreeMap<String, Vec<(String, String)>>,
        scalar_anchors: &mut BTreeMap<String, String>,
        duplicate_anchors: &BTreeSet<String>,
    ) {
        if self.run_scalar.as_deref().is_some_and(str::is_empty) && !self.run_block.is_empty() {
            let body = self.run_block.join("\n");
            self.run_scalar = Some(body);
        }
        if let (Some(anchor), Some(scalar)) = (&self.run_anchor, &self.run_scalar) {
            if duplicate_anchors.contains(anchor) {
                self.invalid_anchor = true;
            } else {
                scalar_anchors.insert(anchor.clone(), scalar.clone());
            }
        }
        if let Some(alias) = &self.env_alias {
            if duplicate_anchors.contains(alias) {
                self.invalid_anchor = true;
            } else if let Some(expanded) = map_anchors.get(alias) {
                self.env_fields.clone_from(expanded);
            } else {
                self.invalid_anchor = true;
            }
        }
        if let Some(anchor) = &self.env_anchor {
            if duplicate_anchors.contains(anchor) {
                self.invalid_anchor = true;
            } else {
                map_anchors.insert(anchor.clone(), self.env_fields.clone());
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct MappingLayout {
    base: usize,
    head_field: bool,
    mapping_indent: usize,
    mapping_width: usize,
    pub(super) malformed_indent: bool,
}

/// Return the emitted map indentation for standard, compact, and anchored
/// sequence entries, rejecting widths outside the two supported renderers.
pub(super) fn mapping_layout(block: &[&str], head: &str, base: usize) -> MappingLayout {
    let head = head.strip_prefix('&').map_or(head, |anchor| {
        anchor
            .split_once(char::is_whitespace)
            .map_or("", |(_, tail)| tail)
    });
    let head_field = head.contains(':');
    if head_field {
        return MappingLayout {
            base,
            head_field: true,
            mapping_indent: base + 2,
            mapping_width: 2,
            malformed_indent: false,
        };
    }

    let first_field_indent = block
        .iter()
        .skip(1)
        .find(|line| !line.trim().is_empty() && indent(line) > base && line.trim().contains(':'))
        .map(|line| indent(line));
    match first_field_indent.map(|field_indent| field_indent - base) {
        Some(width @ (1 | 2)) => MappingLayout {
            base,
            head_field: false,
            mapping_indent: base + width,
            mapping_width: width,
            malformed_indent: false,
        },
        Some(_) => MappingLayout {
            base,
            head_field: false,
            mapping_indent: base + 2,
            mapping_width: 2,
            malformed_indent: true,
        },
        None => MappingLayout {
            base,
            head_field: false,
            mapping_indent: base + 2,
            mapping_width: 2,
            malformed_indent: false,
        },
    }
}

fn is_block_scalar(value: &str) -> bool {
    matches!(value.trim().chars().next(), Some('|' | '>'))
}
