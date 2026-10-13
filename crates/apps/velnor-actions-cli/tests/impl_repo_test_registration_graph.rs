//! Syntax scan for test attributes and compiler-visible Rust source includes.

#[path = "impl_repo_test_registration_cfg.rs"]
pub(super) mod cfg;
#[path = "impl_repo_test_registration_macros.rs"]
mod macros;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{ItemConst, ItemFn, ItemImpl, ItemStatic, ItemTrait};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(super) enum Possibility {
    Never,
    Always,
    Sometimes,
}
pub(super) use macros::{Call, Definition};

pub(super) struct IncludeSource {
    pub(super) path: IncludePath,
    pub(super) condition: Possibility,
}

pub(super) enum IncludePath {
    Relative(PathBuf),
    CargoOutDir(PathBuf),
}

#[derive(Default)]
pub(super) struct SourceFindings {
    pub(super) test_bearing: bool,
    pub(super) includes: Vec<IncludeSource>,
    pub(super) macro_definitions: Vec<Definition>,
    pub(super) macro_calls: Vec<Call>,
}

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(super) fn attrs_possible_in_test(attributes: &[syn::Attribute]) -> Outcome<bool> {
    cfg::attrs_possible_in_test(attributes)
}

pub(super) fn attrs_possibility_in_test(attributes: &[syn::Attribute]) -> Outcome<Possibility> {
    cfg::attrs_possibility_in_test(attributes)
}

pub(super) fn test_condition_state(meta: &syn::Meta) -> Outcome<Possibility> {
    cfg::test_condition_state(meta)
}

pub(super) fn macro_has_include(definition: &Definition) -> bool {
    macros::has_include_invocation(definition)
}

pub(super) fn macro_include_sources(
    definition: &Definition,
) -> Outcome<Vec<(PathBuf, Possibility)>> {
    macros::included_sources(definition, &definition.source)
}

pub(super) fn direct_include_path(
    tokens: &proc_macro2::TokenStream,
    source: &Path,
) -> Outcome<IncludePath> {
    macros::direct_include_path(tokens, source)
}

struct SourceVisitor<'a> {
    source: &'a Path,
    findings: SourceFindings,
    condition: Possibility,
    scope: Vec<String>,
    source_order: usize,
    error: Option<String>,
}

impl<'ast> Visit<'ast> for SourceVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        match cfg::attrs_possibility_in_test(&node.attrs) {
            Ok(condition) => {
                let previous = self.condition;
                self.condition = combine(previous, condition);
                if self.condition != Possibility::Never {
                    match node.attrs.iter().try_fold(false, |found, attribute| {
                        Ok::<_, Box<dyn Error>>(found || cfg::meta_marks_test(&attribute.meta)?)
                    }) {
                        Ok(found) => self.findings.test_bearing |= found,
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                self.condition = previous;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        match cfg::attrs_possibility_in_test(&node.attrs) {
            Ok(condition) => {
                let previous = self.condition;
                self.condition = combine(previous, condition);
                if self.condition != Possibility::Never {
                    self.scope.push(node.ident.to_string());
                    visit::visit_item_mod(self, node);
                    self.scope.pop();
                }
                self.condition = previous;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        let source_order = self.source_order;
        self.source_order += 1;
        let item_condition = match cfg::attrs_possibility_in_test(&node.attrs) {
            Ok(condition) => combine(self.condition, condition),
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        if item_condition == Possibility::Never {
            return;
        }
        let path = &node.mac.path;
        if path.is_ident("macro_rules") {
            let Some(name) = &node.ident else {
                self.error = Some("macro_rules definition has no name".to_owned());
                return;
            };
            match macros::definition(
                name.to_string(),
                item_condition,
                scope_name(&self.scope),
                source_order,
                &node.mac.tokens,
                self.source,
            ) {
                Ok(definition) => {
                    self.findings.macro_definitions.push(definition);
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        } else if path.is_ident("include") {
            match direct_include_path(&node.mac.tokens, self.source) {
                Ok(path) => self.findings.includes.push(IncludeSource {
                    path,
                    condition: item_condition,
                }),
                Err(error) => self.error = Some(error.to_string()),
            }
        } else {
            match macros::call(
                path,
                item_condition,
                scope_name(&self.scope),
                source_order,
                &node.mac.tokens,
            ) {
                Ok(call) => self.findings.macro_calls.push(call),
                Err(error) => self.error = Some(error.to_string()),
            }
            match macros::tokens_mark_test(&node.mac.tokens) {
                Ok(found) => self.findings.test_bearing |= found,
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }

    fn visit_item_const(&mut self, _node: &'ast ItemConst) {}

    fn visit_item_static(&mut self, _node: &'ast ItemStatic) {}

    fn visit_item_impl(&mut self, _node: &'ast ItemImpl) {}

    fn visit_item_trait(&mut self, _node: &'ast ItemTrait) {}
}

pub(super) fn source_findings(path: &Path) -> Outcome<SourceFindings> {
    let source = fs::read_to_string(path)?;
    let syntax = syn::parse_file(&source)?;
    if cfg::attrs_possibility_in_test(&syntax.attrs)? == Possibility::Never {
        return Ok(SourceFindings::default());
    }
    let condition = cfg::attrs_possibility_in_test(&syntax.attrs)?;
    let mut visitor = SourceVisitor {
        source: path,
        findings: SourceFindings::default(),
        condition,
        scope: Vec::new(),
        source_order: 0,
        error: None,
    };
    visitor.visit_file(&syntax);
    if let Some(error) = visitor.error {
        return Err(error.into());
    }
    mark_emitted_tests(&mut visitor.findings);
    Ok(visitor.findings)
}

fn mark_emitted_tests(findings: &mut SourceFindings) {
    for call in &findings.macro_calls {
        for definition in &findings.macro_definitions {
            if definition.name == call.name
                && definition.scope == call.scope
                && definition.source_order < call.source_order
                && definition.test_bearing
                && definition.condition != Possibility::Never
                && call.condition != Possibility::Never
            {
                findings.test_bearing = true;
            }
        }
    }
}

pub(super) fn active_macro_includes(
    findings: &SourceFindings,
) -> Outcome<Vec<(PathBuf, Possibility)>> {
    let mut includes = Vec::new();
    for definition in &findings.macro_definitions {
        if !macro_has_include(definition) {
            continue;
        }
        let calls = findings
            .macro_calls
            .iter()
            .filter(|call| {
                call.name == definition.name
                    && call.scope == definition.scope
                    && definition.source_order < call.source_order
            })
            .collect::<Vec<_>>();
        if calls.is_empty() {
            continue;
        }
        let definitions = findings
            .macro_definitions
            .iter()
            .filter(|candidate| {
                candidate.name == definition.name && candidate.scope == definition.scope
            })
            .count();
        if definitions != 1
            || !definition.empty_matcher_only
            || calls.iter().any(|call| !call.empty_arguments)
        {
            return Err(format!(
                "include-bearing macro must have one empty matcher and empty calls: {}!",
                definition.name
            )
            .into());
        }
        let mut calls_condition = Possibility::Never;
        for call in calls {
            calls_condition = combine_or(calls_condition, call.condition);
        }
        let condition = combine(definition.condition, calls_condition);
        includes.extend(
            macro_include_sources(definition)?
                .into_iter()
                .map(|(path, include_condition)| (path, combine(condition, include_condition))),
        );
    }
    includes.sort_by(|left, right| left.0.cmp(&right.0));
    includes.dedup();
    Ok(includes)
}

fn scope_name(scope: &[String]) -> String {
    scope.join("::")
}

fn combine(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Never, _) | (_, Possibility::Never) => Possibility::Never,
        (Possibility::Always, Possibility::Always) => Possibility::Always,
        _ => Possibility::Sometimes,
    }
}

fn combine_or(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Always, _) | (_, Possibility::Always) => Possibility::Always,
        (Possibility::Never, Possibility::Never) => Possibility::Never,
        _ => Possibility::Sometimes,
    }
}
