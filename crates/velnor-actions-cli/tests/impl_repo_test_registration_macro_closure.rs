//! Restrict macro-expanded source admission to directly proved calls.

use std::collections::HashSet;
use std::error::Error;
use std::path::PathBuf;

use super::{ModuleFile, SourceRecord};
use crate::impl_repo_test_registration::graph::{self, Call, Definition, Possibility};

type Outcome<T> = Result<T, Box<dyn Error>>;

struct DefinitionAt<'a> {
    source: PathBuf,
    module_dir: PathBuf,
    source_base: PathBuf,
    file_condition: Possibility,
    definition: &'a Definition,
}

struct CallAt<'a> {
    source: PathBuf,
    call: &'a Call,
}

pub(super) fn collect(
    records: &[SourceRecord],
    expanded: &mut HashSet<(PathBuf, String, String)>,
) -> Outcome<Vec<(PathBuf, Possibility, ModuleFile)>> {
    let (definitions, calls) = index_sources(records);
    let mut includes = Vec::new();
    for definition in &definitions {
        includes.extend(active_includes(definition, &definitions, &calls, expanded)?);
    }
    Ok(includes)
}

fn index_sources(records: &[SourceRecord]) -> (Vec<DefinitionAt<'_>>, Vec<CallAt<'_>>) {
    let mut definitions = Vec::new();
    let mut calls = Vec::new();
    for record in records {
        definitions.extend(record.findings.macro_definitions.iter().map(|definition| {
            DefinitionAt {
                source: record.file.source.clone(),
                module_dir: record.file.module_dir.clone(),
                source_base: record.file.source_base.clone(),
                file_condition: record.file.condition,
                definition,
            }
        }));
        calls.extend(record.findings.macro_calls.iter().map(|call| CallAt {
            source: record.file.source.clone(),
            call,
        }));
    }
    (definitions, calls)
}

fn active_includes(
    source_definition: &DefinitionAt<'_>,
    definitions: &[DefinitionAt<'_>],
    calls: &[CallAt<'_>],
    expanded: &mut HashSet<(PathBuf, String, String)>,
) -> Outcome<Vec<(PathBuf, Possibility, ModuleFile)>> {
    let definition = source_definition.definition;
    if !graph::macro_has_include(definition) {
        return Ok(Vec::new());
    }
    let key = (
        source_definition.source.clone(),
        definition.scope.clone(),
        definition.name.clone(),
    );
    if expanded.contains(&key) {
        return Ok(Vec::new());
    }
    let matching_calls = calls
        .iter()
        .filter(|call| {
            call.call.scope == definition.scope
                && call.call.name == definition.name
                && (call.source != source_definition.source
                    || definition.source_order < call.call.source_order)
        })
        .collect::<Vec<_>>();
    if matching_calls.is_empty() {
        return Ok(Vec::new());
    }
    ensure_unique_definition(source_definition, definitions)?;
    ensure_local_calls(source_definition, &matching_calls)?;
    ensure_supported_calls(definition, &matching_calls)?;
    let calls_condition = matching_calls
        .iter()
        .fold(Possibility::Never, |state, call| {
            combine_or(state, call.call.condition)
        });
    let active_condition = combine(
        source_definition.file_condition,
        combine(definition.condition, calls_condition),
    );
    let includes = graph::macro_include_sources(definition)?
        .into_iter()
        .map(|(path, include_condition)| {
            (
                path,
                combine(active_condition, include_condition),
                ModuleFile {
                    source: source_definition.source.clone(),
                    module_dir: source_definition.module_dir.clone(),
                    source_base: source_definition.source_base.clone(),
                    condition: active_condition,
                },
            )
        })
        .collect();
    expanded.insert(key);
    Ok(includes)
}

fn ensure_unique_definition(
    source_definition: &DefinitionAt<'_>,
    definitions: &[DefinitionAt<'_>],
) -> Outcome<()> {
    let definition = source_definition.definition;
    let matches = definitions
        .iter()
        .filter(|candidate| {
            candidate.definition.scope == definition.scope
                && candidate.definition.name == definition.name
        })
        .count();
    if matches != 1 {
        return Err(format!(
            "include-bearing macro is ambiguous: {}! in {}",
            definition.name,
            source_definition.source.display()
        )
        .into());
    }
    Ok(())
}

fn ensure_local_calls(source_definition: &DefinitionAt<'_>, calls: &[&CallAt<'_>]) -> Outcome<()> {
    if calls
        .iter()
        .any(|call| call.source != source_definition.source)
    {
        return Err(format!(
            "include-bearing macro call crosses source files: {}! in {}",
            source_definition.definition.name,
            source_definition.source.display()
        )
        .into());
    }
    Ok(())
}

fn ensure_supported_calls(definition: &Definition, calls: &[&CallAt<'_>]) -> Outcome<()> {
    if !definition.empty_matcher_only || calls.iter().any(|call| !call.call.empty_arguments) {
        return Err(format!(
            "include-bearing macro must have one empty matcher and empty calls: {}!",
            definition.name
        )
        .into());
    }
    Ok(())
}

fn combine_or(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Always, _) | (_, Possibility::Always) => Possibility::Always,
        (Possibility::Never, Possibility::Never) => Possibility::Never,
        _ => Possibility::Sometimes,
    }
}

fn combine(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Never, _) | (_, Possibility::Never) => Possibility::Never,
        (Possibility::Always, Possibility::Always) => Possibility::Always,
        _ => Possibility::Sometimes,
    }
}
