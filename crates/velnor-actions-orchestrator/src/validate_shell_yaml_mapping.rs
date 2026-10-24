//! Bounded normalization of the renderer's closed workflow alias grammar.

use std::collections::{BTreeMap, BTreeSet};

use crate::OrchestratorError;

use super::super::shellcheck_fail;
use super::{alias_scope, mapping_entry, run_anchor};

#[path = "validate_shell_yaml_mapping_compact.rs"]
mod compact;
#[path = "validate_shell_yaml_mapping_shape.rs"]
mod shape;

const MAX_INPUT: usize = 500_000;
#[cfg(all(test, feature = "test-render-capture"))]
const MAX_EQUIVALENCE_INPUT: usize = 2_000_000;
const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 262_144;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone)]
struct Line {
    indent: usize,
    content: String,
    raw_content: String,
}

enum BlockLine {
    Content(String),
    WhitespaceOnly { indentation: usize },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Mapping,
    Step,
}

#[derive(Default)]
struct Normalizer {
    names: BTreeSet<String>,
    mappings: BTreeMap<String, (Kind, Vec<Line>)>,
    runs: BTreeMap<String, String>,
    nodes: usize,
    bytes: usize,
}

pub(super) fn normalize(text: &str) -> Result<String, OrchestratorError> {
    normalize_with_input_limit(text, MAX_INPUT)
}

#[cfg(all(test, feature = "test-render-capture"))]
pub(super) fn normalize_for_equivalence(text: &str) -> Result<String, OrchestratorError> {
    normalize_with_input_limit(text, MAX_EQUIVALENCE_INPUT)
}

fn normalize_with_input_limit(text: &str, input_limit: usize) -> Result<String, OrchestratorError> {
    if text.len() > input_limit {
        return Err(fail("input_limit"));
    }
    let lines = text
        .lines()
        .map(|raw| {
            let (indent, content) = indent_content(raw)?;
            if indent > MAX_DEPTH * 2 {
                return Err(fail("depth_limit"));
            }
            let raw_content = raw[indent..].to_owned();
            Ok(Line {
                indent,
                content: content.to_owned(),
                raw_content,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let lines = compact::normalize(&lines)?;
    let mut normalizer = Normalizer::default();
    let expanded = normalizer.lines(&lines, Vec::new(), 0)?;
    let mut output = String::new();
    for line in expanded {
        output.push_str(&" ".repeat(line.indent));
        output.push_str(&line.content);
        output.push('\n');
    }
    Ok(output)
}

impl Normalizer {
    fn lines(
        &mut self,
        lines: &[Line],
        mut path: Vec<(usize, String)>,
        depth: usize,
    ) -> Result<Vec<Line>, OrchestratorError> {
        if depth > MAX_DEPTH {
            return Err(fail("depth_limit"));
        }
        let mut output = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            let line = &lines[index];
            if line.content.is_empty() || line.content.starts_with('#') {
                self.push(&mut output, line.clone())?;
                index += 1;
                continue;
            }
            path.retain(|(indent, _)| *indent < line.indent);
            if let Some((kind, value)) = shape::alias_site(line, &path)? {
                let end = block_end(lines, index);
                let expanded = self.alias(lines, index, end, (kind, &value), &path, depth)?;
                for expanded_line in expanded {
                    self.push(&mut output, expanded_line)?;
                }
                index = end;
                continue;
            }
            let mut line = line.clone();
            let mut next_index = index + 1;
            if shape::is_run(&line, &path)
                && let Some((_, value)) = mapping_entry(&line.content)
            {
                let mut value = value.to_owned();
                if value == "|" {
                    let (body, end) = literal_run_block(lines, index);
                    if body.is_empty() {
                        return Err(super::super::shellcheck_fail("run_block_scalar_unlintable"));
                    }
                    value = quote_run_body(&body)?;
                    line.content = format!("run: {value}");
                    line.raw_content.clone_from(&line.content);
                    next_index = end;
                }
                if let Some(definition) = value.strip_prefix('&')
                    && let Some((name, _)) = definition.split_once(' ')
                    && !self.names.insert(name.to_owned())
                {
                    return Err(super::super::shellcheck_fail("run_scalar_anchor_duplicate"));
                }
                let resolved = run_anchor::resolve_run_scalar(&value, &mut self.runs)?;
                line.content = format!("run: {resolved}");
            }
            // Keep all existing scope/tag/flow checks on ordinary source lines.
            if let Some((key, value)) = mapping_entry(&line.content) {
                alias_scope::validate_mapping_value(key, value, shape::is_run(&line, &path))?;
            } else {
                alias_scope::reject_non_mapping_content(&line.content)?;
            }
            shape::update_path(&line, &mut path);
            self.push(&mut output, line)?;
            index = next_index;
        }
        Ok(output)
    }

    fn alias(
        &mut self,
        lines: &[Line],
        index: usize,
        end: usize,
        site: (Kind, &str),
        path: &[(usize, String)],
        depth: usize,
    ) -> Result<Vec<Line>, OrchestratorError> {
        let header = &lines[index];
        let (kind, value) = site;
        let name = &value[1..];
        if !shape::valid_name(name) {
            return Err(fail("name_invalid"));
        }
        let body = if value.starts_with('&') {
            if !self.names.insert(name.to_owned()) {
                return Err(fail("duplicate_anchor"));
            }
            let mut child_path = path.to_vec();
            child_path.push((
                header.indent,
                if kind == Kind::Step {
                    "[]".to_owned()
                } else {
                    mapping_entry(&header.content)
                        .ok_or_else(|| fail("mapping_entry_unsupported"))?
                        .0
                        .to_owned()
                },
            ));
            let body = self.lines(&lines[index + 1..end], child_path, depth + 1)?;
            shape::validate_body(kind, &body, header.indent + 2)?;
            let relative = body
                .iter()
                .map(|line| Line {
                    indent: line.indent.saturating_sub(header.indent + 2),
                    content: line.content.clone(),
                    raw_content: line.raw_content.clone(),
                })
                .collect::<Vec<_>>();
            self.mappings
                .insert(name.to_owned(), (kind, relative.clone()));
            relative
        } else {
            if end != index + 1 {
                return Err(fail("alias_has_children"));
            }
            let (found_kind, body) = self
                .mappings
                .get(name)
                .ok_or_else(|| fail("alias_unresolved"))?;
            if *found_kind != kind {
                return Err(fail("alias_kind_mismatch"));
            }
            body.clone()
        };
        shape::materialize(header, kind, &body)
    }

    fn push(&mut self, output: &mut Vec<Line>, line: Line) -> Result<(), OrchestratorError> {
        self.nodes = self.nodes.saturating_add(1);
        self.bytes = self
            .bytes
            .saturating_add(line.indent + line.content.len() + 1);
        if self.nodes > MAX_NODES || self.bytes > MAX_BYTES {
            return Err(fail("expansion_limit"));
        }
        output.push(line);
        Ok(())
    }
}

pub(super) fn indent_content(line: &str) -> Result<(usize, &str), OrchestratorError> {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    if line.as_bytes().get(indent) == Some(&b'\t') {
        return Err(shellcheck_fail("workflow_yaml_tabs_unsupported"));
    }
    Ok((indent, line[indent..].trim_end()))
}

fn block_end(lines: &[Line], index: usize) -> usize {
    let indent = lines[index].indent;
    let mut end = index + 1;
    while end < lines.len() && lines[end].indent > indent {
        end += 1;
    }
    end
}

/// Read a step's `run: |` text and apply the YAML rule for final line breaks.
fn literal_run_block(lines: &[Line], index: usize) -> (String, usize) {
    let header_indent = lines[index].indent;
    let mut content_indent = None;
    let mut body_lines = Vec::new();
    let mut end = index + 1;

    while let Some(line) = lines.get(end) {
        if line.raw_content.is_empty() {
            body_lines.push(BlockLine::WhitespaceOnly {
                indentation: line.indent,
            });
            end += 1;
            continue;
        }
        let Some(indent) = content_indent else {
            if line.indent <= header_indent {
                break;
            }
            content_indent = Some(line.indent);
            body_lines.push(BlockLine::Content(line.raw_content.clone()));
            end += 1;
            continue;
        };
        if line.indent < indent {
            break;
        }
        let extra_indent = line.indent - indent;
        body_lines.push(BlockLine::Content(format!(
            "{}{}",
            " ".repeat(extra_indent),
            line.raw_content
        )));
        end += 1;
    }

    let Some(content_indent) = content_indent else {
        return (String::new(), end);
    };
    let mut body_lines = body_lines
        .into_iter()
        .map(|line| match line {
            BlockLine::Content(line) => line,
            BlockLine::WhitespaceOnly { indentation } => {
                " ".repeat(indentation.saturating_sub(content_indent))
            }
        })
        .collect::<Vec<_>>();
    while body_lines.last().is_some_and(String::is_empty) {
        body_lines.pop();
    }
    if body_lines.is_empty() {
        return (String::new(), end);
    }
    let mut body = body_lines.join("\n");
    body.push('\n');
    (body, end)
}

fn quote_run_body(body: &str) -> Result<String, OrchestratorError> {
    let mut quoted = String::with_capacity(body.len() + 2);
    quoted.push('"');
    for character in body.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            value if value.is_control() => {
                return Err(fail("block_scalar_character_unsupported"));
            }
            value => quoted.push(value),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

fn fail(problem: &str) -> OrchestratorError {
    shellcheck_fail(&format!("workflow_mapping_{problem}"))
}

#[cfg(test)]
#[path = "validate_shell_yaml_mapping_tests.rs"]
mod tests;
