// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::layout::{
    MappingFieldState, StepLayout, StepTopFieldsState, anchor_name, indent, normalize_top_field,
    step_field_content, update_mapping_alias_leaf,
};
use super::scalars::StepRun;

pub(super) struct StepTopFields {
    pub(super) top_fields: Vec<(String, String)>,
    pub(super) env_fields: Vec<(String, String)>,
    pub(super) unexpected_nested: bool,
    pub(super) invalid_anchor: bool,
    pub(super) anchor_unsafe: bool,
}

struct StepTopFieldsBuilder<'a> {
    top_fields: Vec<(String, String)>,
    env_fields: Vec<(String, String)>,
    with_fields: Vec<(String, String)>,
    with_mapping: bool,
    anchor_unsafe: bool,
    state: StepTopFieldsState<'a>,
}

pub(super) fn step_top_fields(
    block: &[&str],
    layout: StepLayout,
    env_anchors: &mut BTreeMap<String, Option<Vec<(String, String)>>>,
    with_anchors: &mut BTreeMap<String, Option<Vec<(String, String)>>>,
    run: &StepRun,
    duplicate_anchors: &BTreeSet<String>,
) -> StepTopFields {
    let mut builder = StepTopFieldsBuilder {
        top_fields: Vec::new(),
        env_fields: Vec::new(),
        with_fields: Vec::new(),
        with_mapping: false,
        anchor_unsafe: layout.malformed_indent || run.malformed_body || run.invalid_anchor,
        state: StepTopFieldsState {
            run,
            env_anchor: None,
            env_alias: None,
            env_field: MappingFieldState::Other,
            with_anchor: None,
            with_alias: None,
            with_field: MappingFieldState::Other,
            unexpected_nested: layout.malformed_indent || run.malformed_body,
            invalid_anchor: run.invalid_anchor,
        },
    };
    for (index, line) in block.iter().enumerate() {
        collect_step_top_field(index, line, block, layout, &mut builder);
    }
    builder.anchor_unsafe |= builder.state.invalid_anchor;
    resolve_mapping_anchors(builder, env_anchors, with_anchors, duplicate_anchors)
}

fn collect_step_top_field(
    index: usize,
    line: &str,
    block: &[&str],
    layout: StepLayout,
    builder: &mut StepTopFieldsBuilder<'_>,
) {
    if builder.state.run.is_body_line(index) {
        return;
    }
    let trimmed = step_field_content(index, line, block[0]).trim();
    let line_indent = indent(line);
    if update_mapping_alias_leaf(
        trimmed,
        line_indent,
        layout.mapping_indent,
        &mut builder.state.env_field,
        &mut builder.state.unexpected_nested,
        &mut builder.state.invalid_anchor,
    ) || update_mapping_alias_leaf(
        trimmed,
        line_indent,
        layout.mapping_indent,
        &mut builder.state.with_field,
        &mut builder.state.unexpected_nested,
        &mut builder.state.invalid_anchor,
    ) {
        return;
    }
    let field = trimmed
        .split_once(": ")
        .or_else(|| trimmed.strip_suffix(':').map(|key| (key, "")));
    let Some((key, value)) = field else {
        if index > 0 && !line.trim().is_empty() {
            builder.state.unexpected_nested = true;
            if !(builder.with_mapping && trimmed.starts_with('#')) {
                builder.anchor_unsafe = true;
            }
        }
        return;
    };
    if (index == 0 && layout.head_field) || line_indent == layout.mapping_indent {
        builder.with_mapping =
            key == "with" && (value.is_empty() || anchor_name(value, '&').is_some());
        let normalized = normalize_top_field(key, value, &mut builder.state);
        builder.top_fields.push((key.to_owned(), normalized));
    } else if builder.state.env_field == MappingFieldState::Map
        && line_indent == layout.mapping_indent + layout.mapping_width
    {
        builder.env_fields.push((key.to_owned(), value.to_owned()));
    } else if builder.with_mapping
        && line_indent == layout.mapping_indent + layout.mapping_width
        && !key.is_empty()
    {
        builder.with_fields.push((key.to_owned(), value.to_owned()));
        builder.state.unexpected_nested = true;
    } else {
        builder.state.unexpected_nested = true;
        builder.anchor_unsafe = true;
    }
}

fn resolve_mapping_anchors(
    mut builder: StepTopFieldsBuilder<'_>,
    env_anchors: &mut BTreeMap<String, Option<Vec<(String, String)>>>,
    with_anchors: &mut BTreeMap<String, Option<Vec<(String, String)>>>,
    duplicate_anchors: &BTreeSet<String>,
) -> StepTopFields {
    if let Some(anchor) = builder.state.env_anchor.take() {
        if builder.anchor_unsafe
            || duplicate_anchors.contains(&anchor)
            || env_anchors.contains_key(&anchor)
        {
            builder.state.unexpected_nested = true;
            builder.state.invalid_anchor = true;
            env_anchors.insert(anchor, None);
        } else {
            env_anchors.insert(anchor, Some(builder.env_fields.clone()));
        }
    }
    if let Some(alias) = builder.state.env_alias.take() {
        if let Some(Some(aliased_fields)) = env_anchors
            .get(&alias)
            .filter(|_| !duplicate_anchors.contains(&alias))
        {
            builder.env_fields.clone_from(aliased_fields);
        } else {
            builder.state.unexpected_nested = true;
            builder.state.invalid_anchor = true;
        }
    }
    if let Some(anchor) = builder.state.with_anchor.take() {
        if builder.anchor_unsafe
            || builder.with_fields.is_empty()
            || duplicate_anchors.contains(&anchor)
            || with_anchors.contains_key(&anchor)
        {
            builder.state.unexpected_nested = true;
            builder.state.invalid_anchor = true;
            with_anchors.insert(anchor, None);
        } else {
            with_anchors.insert(anchor, Some(builder.with_fields.clone()));
        }
    }
    if let Some(alias) = builder.state.with_alias.take() {
        if let Some(Some(_aliased_fields)) = with_anchors
            .get(&alias)
            .filter(|_| !duplicate_anchors.contains(&alias))
        {
            builder.state.unexpected_nested = true;
        } else {
            builder.state.unexpected_nested = true;
            builder.state.invalid_anchor = true;
        }
    }
    StepTopFields {
        top_fields: builder.top_fields,
        env_fields: builder.env_fields,
        unexpected_nested: builder.state.unexpected_nested,
        invalid_anchor: builder.state.invalid_anchor,
        anchor_unsafe: builder.anchor_unsafe,
    }
}
