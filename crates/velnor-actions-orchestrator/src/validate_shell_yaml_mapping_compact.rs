use std::collections::BTreeMap;

use crate::OrchestratorError;

use super::super::mapping_entry;
use super::{Line, MAX_DEPTH, fail};

#[cfg(test)]
#[path = "validate_shell_yaml_mapping_compact_tests.rs"]
mod tests;

pub(super) fn normalize(lines: &[Line]) -> Result<Vec<Line>, OrchestratorError> {
    let Some(width) = emitted_width(lines)? else {
        return Ok(lines.to_vec());
    };
    validate_layout(lines, width)?;
    if width == 2 {
        return Ok(lines.to_vec());
    }
    let step_heads = compact_step_heads(lines, width)?;
    Ok(canonicalize_compact(lines, &step_heads))
}

fn emitted_width(lines: &[Line]) -> Result<Option<usize>, OrchestratorError> {
    let Some(jobs) = lines.iter().position(|line| {
        line.indent == 0
            && mapping_entry(&line.content)
                .is_some_and(|(key, value)| key == "jobs" && value.is_empty())
    }) else {
        return Ok(None);
    };
    let Some(first_job) = lines[jobs + 1..]
        .iter()
        .find(|line| is_significant(&line.content))
    else {
        return Ok(None);
    };
    match first_job.indent {
        1 | 2 if mapping_entry(&first_job.content).is_some() => Ok(Some(first_job.indent)),
        _ => Err(fail("indentation_width_unsupported")),
    }
}

fn validate_layout(lines: &[Line], width: usize) -> Result<(), OrchestratorError> {
    let mut parents = vec![0];
    let mut block_header = None;
    for line in lines {
        if let Some(header_indent) = block_header {
            if line.raw_content.is_empty() || line.indent > header_indent {
                continue;
            }
            block_header = None;
        }
        if !is_significant(&line.content) {
            continue;
        }
        if line.indent % width != 0 || line.indent / width > MAX_DEPTH {
            return Err(fail("indentation_mixed_or_too_deep"));
        }
        while parents.len() > 1 && parents.last().is_some_and(|indent| *indent > line.indent) {
            parents.pop();
        }
        if parents.last().copied() != Some(line.indent) {
            return Err(fail("indentation_mixed_or_malformed"));
        }
        if let Some((_, value)) = mapping_entry(&line.content)
            && is_block_header(value)
        {
            block_header = Some(line.indent);
        } else if opens_container(&line.content) {
            parents.push(line.indent + width);
        }
    }
    Ok(())
}

fn compact_step_heads(
    lines: &[Line],
    width: usize,
) -> Result<BTreeMap<usize, (usize, Line)>, OrchestratorError> {
    let mut heads = BTreeMap::new();
    let mut in_jobs = false;
    let mut in_job = false;
    let mut in_steps = false;
    let mut block_header = None;
    for (index, line) in lines.iter().enumerate() {
        if let Some(header_indent) = block_header {
            if line.raw_content.is_empty() || line.indent > header_indent {
                continue;
            }
            block_header = None;
        }
        if !is_significant(&line.content) {
            continue;
        }
        if let Some((_, value)) = mapping_entry(&line.content)
            && is_block_header(value)
        {
            block_header = Some(line.indent);
        }
        if line.indent == 0 {
            in_jobs = mapping_entry(&line.content)
                .is_some_and(|(key, value)| key == "jobs" && value.is_empty());
            in_job = false;
            in_steps = false;
            continue;
        }
        if !in_jobs {
            continue;
        }
        if line.indent == width {
            in_job = mapping_entry(&line.content).is_some();
            in_steps = false;
            continue;
        }
        if in_job && line.indent == width * 2 {
            in_steps = mapping_entry(&line.content)
                .is_some_and(|(key, value)| key == "steps" && value.is_empty());
            continue;
        }
        if !in_steps || line.indent != width * 3 || line.content != "-" {
            continue;
        }
        let Some(first) = lines.get(index + 1) else {
            return Err(fail("compact_step_mapping_unsupported"));
        };
        if first.indent != width * 4
            || mapping_entry(&first.content)
                .is_none_or(|(key, value)| key != "name" || value.is_empty())
        {
            return Err(fail("compact_step_mapping_unsupported"));
        }
        let combined = Line {
            indent: line.indent,
            content: format!("- {}", first.content),
            raw_content: format!("- {}", first.raw_content),
        };
        heads.insert(index, (index + 1, combined));
    }
    Ok(heads)
}

fn canonicalize_compact(lines: &[Line], step_heads: &BTreeMap<usize, (usize, Line)>) -> Vec<Line> {
    let mut output = Vec::with_capacity(lines.len());
    let mut block_header = None;
    let mut index = 0;
    while index < lines.len() {
        let mut line = lines[index].clone();
        if let Some(header_indent) = block_header {
            if line.raw_content.is_empty() || line.indent > header_indent {
                line.indent = compact_body_indent(line.indent, header_indent);
                output.push(line);
                index += 1;
                continue;
            }
            block_header = None;
        }
        if let Some((first_index, combined)) = step_heads.get(&index) {
            let mut combined = combined.clone();
            combined.indent *= 2;
            output.push(combined);
            index = first_index + 1;
            continue;
        }
        let old_indent = line.indent;
        line.indent *= 2;
        if mapping_entry(&line.content).is_some_and(|(_, value)| is_block_header(value)) {
            block_header = Some(old_indent);
        }
        output.push(line);
        index += 1;
    }
    output
}

fn compact_body_indent(indent: usize, header_indent: usize) -> usize {
    if indent > header_indent {
        header_indent * 2 + (indent - header_indent)
    } else {
        indent
    }
}

fn opens_container(content: &str) -> bool {
    if let Some(item) = content.strip_prefix("- ")
        && map_entry_has_separator(item)
    {
        return true;
    }
    if let Some((_, value)) = mapping_entry(content) {
        return value.is_empty() || is_anchor_only(value);
    }
    let Some(item) = content.strip_prefix("-") else {
        return false;
    };
    if item.is_empty() {
        return true;
    }
    let Some(item) = item.strip_prefix(' ') else {
        return false;
    };
    is_anchor_only(item) || map_entry_has_separator(item)
}

fn map_entry_has_separator(content: &str) -> bool {
    mapping_entry(content).is_some()
        && content
            .find(':')
            .and_then(|index| content.as_bytes().get(index + 1))
            .is_none_or(|next| *next == b' ')
}

fn is_anchor_only(value: &str) -> bool {
    let Some(name) = value.strip_prefix('&') else {
        return false;
    };
    !name.is_empty() && !name.bytes().any(|byte| byte.is_ascii_whitespace())
}

fn is_block_header(value: &str) -> bool {
    value.starts_with(['|', '>'])
}

fn is_significant(content: &str) -> bool {
    !content.is_empty() && !content.starts_with('#')
}
