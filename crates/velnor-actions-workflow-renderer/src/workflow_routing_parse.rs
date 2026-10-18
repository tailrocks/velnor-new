use std::collections::BTreeMap;

use saphyr::{LoadableYamlNode, Yaml};
use saphyr_parser::{Event, Parser, ScanError, Span, Tag};

use crate::{RenderError, RenderedFile};

const MAX_WORKFLOW_BYTES: usize = 4 * 1024 * 1024;
const MAX_SYNTAX_EVENTS: usize = 200_000;
const MAX_EXPANDED_NODES: usize = 1_000_000;
const MAX_EXPANDED_SCALAR_BYTES: usize = 64 * 1024 * 1024;
const MAX_SCALAR_BYTES: usize = 1024 * 1024;
const MAX_NESTING_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Default)]
struct NodeCost {
    nodes: usize,
    scalar_bytes: usize,
    height: usize,
}

#[derive(Clone, Copy, Debug)]
struct NodeFrame {
    anchor_id: usize,
    mapping: bool,
    cost: NodeCost,
}

#[derive(Default)]
struct DocumentCost {
    root: Option<NodeCost>,
    frames: Vec<NodeFrame>,
    anchors: BTreeMap<usize, NodeCost>,
    anchor_storage: NodeCost,
}

/// Parse one final workflow after bounding parser work and alias expansion.
pub(super) fn parse_final_workflow(file: &RenderedFile) -> Result<Yaml<'_>, RenderError> {
    if file.bytes.len() > MAX_WORKFLOW_BYTES {
        return Err(resource_error(&file.path, 1, "workflow_bytes"));
    }
    check_expansion(&file.path, &file.bytes)?;
    let mut documents =
        Yaml::load_from_str(&file.bytes).map_err(|error| scan_error(&file.path, &error))?;
    if documents.len() != 1 {
        return Err(invalid(format!("workflow_document_count:{}", file.path)));
    }
    let document = documents
        .pop()
        .ok_or_else(|| invalid(format!("workflow_document_missing:{}", file.path)))?;
    reject_merge_keys(&file.path, &document)?;
    Ok(document)
}

fn check_expansion(path: &str, source: &str) -> Result<(), RenderError> {
    let mut parser = Parser::new_from_str(source);
    let mut document = DocumentCost::default();
    let mut documents = 0;
    let mut ended = false;
    let mut events: usize = 0;
    while let Some(result) = parser.next_event() {
        let (event, span) = result.map_err(|error| scan_error(path, &error))?;
        events = events
            .checked_add(1)
            .ok_or_else(|| resource_error(path, span.start.line(), "syntax_events"))?;
        if events > MAX_SYNTAX_EVENTS {
            return Err(resource_error(path, span.start.line(), "syntax_events"));
        }
        match event {
            Event::DocumentStart(_) => {
                documents += 1;
                if documents > 1 {
                    return Err(invalid(format!("workflow_document_count:{path}")));
                }
                document = DocumentCost::default();
                ended = false;
            }
            Event::Scalar(value, _, anchor_id, tag) => {
                check_tag(path, span, tag.as_deref())?;
                if value.len() > MAX_SCALAR_BYTES {
                    return Err(resource_error(path, span.start.line(), "scalar_bytes"));
                }
                check_depth(path, span, document.frames.len() + 1)?;
                let cost = NodeCost {
                    nodes: 1,
                    scalar_bytes: value
                        .len()
                        .checked_add(tag_bytes(path, span, tag.as_deref())?)
                        .ok_or_else(|| {
                            resource_error(path, span.start.line(), "expanded_scalar_bytes")
                        })?,
                    height: 1,
                };
                finish_node(path, span, &mut document, anchor_id, cost)?;
            }
            Event::SequenceStart(anchor_id, tag) => {
                check_tag(path, span, tag.as_deref())?;
                start_node(path, span, &mut document, anchor_id, false)?;
            }
            Event::MappingStart(anchor_id, tag) => {
                check_tag(path, span, tag.as_deref())?;
                start_node(path, span, &mut document, anchor_id, true)?;
            }
            Event::SequenceEnd => finish_container(path, span, &mut document, false)?,
            Event::MappingEnd => finish_container(path, span, &mut document, true)?,
            Event::Alias(anchor_id) => {
                let cost =
                    document.anchors.get(&anchor_id).copied().ok_or_else(|| {
                        resource_error(path, span.start.line(), "unresolved_alias")
                    })?;
                let depth = document
                    .frames
                    .len()
                    .checked_add(cost.height)
                    .ok_or_else(|| resource_error(path, span.start.line(), "expanded_depth"))?;
                check_depth(path, span, depth)?;
                finish_node(path, span, &mut document, 0, cost)?;
            }
            Event::DocumentEnd => {
                finish_document(path, span, &document)?;
                ended = true;
            }
            Event::StreamEnd => {
                if documents != 1 || !ended {
                    return Err(invalid(format!("workflow_document_count:{path}")));
                }
            }
            Event::Nothing | Event::StreamStart => {}
        }
    }
    if documents != 1 || !ended {
        return Err(invalid(format!("workflow_document_count:{path}")));
    }
    Ok(())
}

fn start_node(
    path: &str,
    span: Span,
    document: &mut DocumentCost,
    anchor_id: usize,
    mapping: bool,
) -> Result<(), RenderError> {
    check_depth(path, span, document.frames.len() + 1)?;
    document.frames.push(NodeFrame {
        anchor_id,
        mapping,
        cost: NodeCost {
            nodes: 1,
            scalar_bytes: 0,
            height: 1,
        },
    });
    Ok(())
}

fn finish_container(
    path: &str,
    span: Span,
    document: &mut DocumentCost,
    mapping: bool,
) -> Result<(), RenderError> {
    let frame = document.frames.pop().ok_or_else(|| {
        invalid(format!(
            "workflow_structure_invalid:{path}:{}",
            span.start.line()
        ))
    })?;
    if frame.mapping != mapping {
        return Err(invalid(format!(
            "workflow_structure_invalid:{path}:{}",
            span.start.line()
        )));
    }
    finish_node(path, span, document, frame.anchor_id, frame.cost)
}

fn finish_node(
    path: &str,
    span: Span,
    document: &mut DocumentCost,
    anchor_id: usize,
    cost: NodeCost,
) -> Result<(), RenderError> {
    if anchor_id != 0 {
        if document.anchors.insert(anchor_id, cost).is_some() {
            return Err(resource_error(path, span.start.line(), "duplicate_anchor"));
        }
        add_cost(path, span, &mut document.anchor_storage, cost)?;
    }
    if let Some(parent) = document.frames.last_mut() {
        add_cost(path, span, &mut parent.cost, cost)?;
        parent.cost.height = parent.cost.height.max(
            cost.height
                .checked_add(1)
                .ok_or_else(|| resource_error(path, span.start.line(), "expanded_depth"))?,
        );
        check_cost(path, span, parent.cost, document.anchor_storage)?;
    } else if document.root.replace(cost).is_some() {
        return Err(invalid(format!(
            "workflow_structure_invalid:{path}:{}",
            span.start.line()
        )));
    }
    Ok(())
}

fn add_cost(
    path: &str,
    span: Span,
    target: &mut NodeCost,
    added: NodeCost,
) -> Result<(), RenderError> {
    target.nodes = target
        .nodes
        .checked_add(added.nodes)
        .ok_or_else(|| resource_error(path, span.start.line(), "expanded_nodes"))?;
    target.scalar_bytes = target
        .scalar_bytes
        .checked_add(added.scalar_bytes)
        .ok_or_else(|| resource_error(path, span.start.line(), "expanded_scalar_bytes"))?;
    Ok(())
}

fn check_cost(
    path: &str,
    span: Span,
    tree: NodeCost,
    anchor_storage: NodeCost,
) -> Result<(), RenderError> {
    let nodes = tree
        .nodes
        .checked_add(anchor_storage.nodes)
        .ok_or_else(|| resource_error(path, span.start.line(), "expanded_nodes"))?;
    let bytes = tree
        .scalar_bytes
        .checked_add(anchor_storage.scalar_bytes)
        .ok_or_else(|| resource_error(path, span.start.line(), "expanded_scalar_bytes"))?;
    if nodes > MAX_EXPANDED_NODES {
        return Err(resource_error(path, span.start.line(), "expanded_nodes"));
    }
    if bytes > MAX_EXPANDED_SCALAR_BYTES {
        return Err(resource_error(
            path,
            span.start.line(),
            "expanded_scalar_bytes",
        ));
    }
    Ok(())
}

fn finish_document(path: &str, span: Span, document: &DocumentCost) -> Result<(), RenderError> {
    if !document.frames.is_empty() {
        return Err(invalid(format!(
            "workflow_structure_invalid:{path}:{}",
            span.start.line()
        )));
    }
    let root = document
        .root
        .ok_or_else(|| invalid(format!("workflow_document_empty:{path}")))?;
    check_cost(path, span, root, document.anchor_storage)?;
    if root.height > MAX_NESTING_DEPTH {
        return Err(resource_error(path, span.start.line(), "expanded_depth"));
    }
    Ok(())
}

fn check_depth(path: &str, span: Span, depth: usize) -> Result<(), RenderError> {
    if depth > MAX_NESTING_DEPTH {
        Err(resource_error(path, span.start.line(), "expanded_depth"))
    } else {
        Ok(())
    }
}

fn check_tag(path: &str, span: Span, tag: Option<&Tag>) -> Result<(), RenderError> {
    let Some(tag) = tag else {
        return Ok(());
    };
    let known_core = tag.is_yaml_core_schema()
        && matches!(
            tag.suffix.as_str(),
            "str" | "int" | "float" | "bool" | "null" | "map" | "seq" | "set" | "omap" | "pairs"
        );
    if known_core {
        Ok(())
    } else {
        Err(invalid(format!(
            "workflow_tag_unsupported:{path}:{}:{}",
            span.start.line(),
            span.start.col()
        )))
    }
}

fn tag_bytes(path: &str, span: Span, tag: Option<&Tag>) -> Result<usize, RenderError> {
    let Some(tag) = tag else {
        return Ok(0);
    };
    tag.handle
        .len()
        .checked_add(tag.suffix.len())
        .ok_or_else(|| resource_error(path, span.start.line(), "expanded_scalar_bytes"))
}

fn reject_merge_keys(path: &str, yaml: &Yaml<'_>) -> Result<(), RenderError> {
    match yaml {
        Yaml::Mapping(mapping) => {
            for (key, value) in mapping {
                if scalar_text(key) == Some("<<") {
                    return Err(invalid(format!("workflow_merge_key_unsupported:{path}")));
                }
                reject_merge_keys(path, key)?;
                reject_merge_keys(path, value)?;
            }
        }
        Yaml::Sequence(sequence) => {
            for value in sequence {
                reject_merge_keys(path, value)?;
            }
        }
        Yaml::Tagged(_, value) => reject_merge_keys(path, value)?,
        Yaml::Alias(_) | Yaml::Representation(..) | Yaml::Value(_) | Yaml::BadValue => {}
    }
    Ok(())
}

fn scalar_text<'a>(value: &'a Yaml<'_>) -> Option<&'a str> {
    match value {
        Yaml::Tagged(_, inner) => scalar_text(inner),
        _ => value.as_str(),
    }
}

fn scan_error(path: &str, error: &ScanError) -> RenderError {
    invalid(format!(
        "workflow_yaml_invalid:{path}:{}:{}",
        error.marker().line(),
        error.marker().col()
    ))
}

fn resource_error(path: &str, line: usize, limit: &str) -> RenderError {
    invalid(format!("workflow_resource_limit:{path}:{line}:{limit}"))
}

fn invalid(problem: String) -> RenderError {
    RenderError::InvalidWorkflow(problem)
}

#[cfg(test)]
#[path = "workflow_routing_parse_tests.rs"]
mod tests;
