//! Production-only Rust scanning for the offline-fetch structural gate.

use std::error::Error;
use std::fs;
use std::path::Path;

use proc_macro2::{TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

#[path = "impl_orch_f2a_offline_execution.rs"]
mod execution;
#[path = "impl_orch_f2a_offline_sources.rs"]
mod sources;

pub(crate) use execution::source_prep_executes_text;
pub(crate) use sources::production_src_files;

pub(crate) fn is_source_prep_source(path: &Path, source_prep: &Path) -> bool {
    path == source_prep
}

pub(crate) fn production_fetch_hits(path: &Path) -> Result<Vec<(usize, String)>, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    fetch_hits_of_text(&production_source_of_text(&source)?).map_err(Into::into)
}

pub(crate) fn fetch_hits_of_text(source: &str) -> Result<Vec<(usize, String)>, syn::Error> {
    let tokens = source.parse::<TokenStream>()?;
    let mut hits = Vec::new();
    collect_fetch_hits(tokens, &mut hits);
    execution::collect_process_fetch_hits(source, &mut hits);
    Ok(hits)
}

fn collect_fetch_hits(tokens: TokenStream, hits: &mut Vec<(usize, String)>) {
    for token in tokens {
        match token {
            TokenTree::Group(group) => collect_fetch_hits(group.stream(), hits),
            TokenTree::Ident(identifier) => {
                let text = identifier.unraw().to_string();
                if contains_forbidden_fetch(&text) {
                    hits.push((identifier.span().start().line, text));
                }
            }
            // String/doc literals describe fetch behavior but do not invoke it.
            TokenTree::Literal(_) | TokenTree::Punct(_) => {}
        }
    }
}

fn contains_forbidden_fetch(token: &str) -> bool {
    // These exact names only carry workflow step/input data through constructors.
    !matches!(
        token,
        "fetch_inventory"
            | "FetchFailure"
            | "fetch_add"
            | "fetch_steps"
            | "fetch_roots"
            | "fetch_steps_for_crate"
            | "fetch_steps_for_plan"
            | "unsafe_fetch_root"
    ) && token.contains("fetch")
}

pub(crate) fn source_prep_executes(path: &Path) -> Result<bool, Box<dyn Error>> {
    let source = fs::read_to_string(path)?;
    source_prep_executes_text(&source).map_err(Into::into)
}

pub(crate) fn source_prep_tree_executes(path: &Path) -> Result<bool, Box<dyn Error>> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("source_prep source has no parent"))?;
    let module_dir = if path.file_name().is_some_and(|name| name == "mod.rs") {
        parent.to_path_buf()
    } else {
        let stem = path
            .file_stem()
            .ok_or_else(|| std::io::Error::other("source_prep source has no file stem"))?;
        parent.join(stem)
    };
    for source in sources::production_src_files_from(path, &module_dir, parent)? {
        if source_prep_executes(&source)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn production_source_of_text(source: &str) -> Result<String, syn::Error> {
    let syntax = match syn::parse_file(source) {
        Ok(syntax) => syntax,
        Err(file_error) => {
            if let Ok(expression) = syn::parse_str::<syn::Expr>(source) {
                return mask_expression_source(source, &expression);
            }
            return Err(file_error);
        }
    };
    if has_cfg_test(&syntax.attrs) {
        return Ok(mask_entire_source(source));
    }
    let mut visitor = CfgTestRanges::default();
    visitor.visit_file(&syntax);
    mask_spans(source, &visitor.ranges).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("could not preserve source while masking test code: {error}"),
        )
    })
}

fn mask_expression_source(source: &str, expression: &syn::Expr) -> Result<String, syn::Error> {
    let mut visitor = CfgTestRanges::default();
    visitor.visit_expr(expression);
    mask_spans(source, &visitor.ranges).map_err(|error| {
        syn::Error::new(
            proc_macro2::Span::call_site(),
            format!("could not preserve source while masking test code: {error}"),
        )
    })
}

fn mask_entire_source(source: &str) -> String {
    source
        .bytes()
        .map(|byte| match byte {
            b'\n' | b'\r' => char::from(byte),
            _ => ' ',
        })
        .collect()
}

fn mask_spans(source: &str, spans: &[SourceSpan]) -> Result<String, std::string::FromUtf8Error> {
    let mut bytes = source.as_bytes().to_vec();
    for span in spans {
        let start = line_column_offset(source, span.start_line, span.start_column);
        let end = line_column_offset(source, span.end_line, span.end_column);
        let length = bytes.len();
        let start = start.min(length);
        let end = end.min(length);
        if start < end {
            for byte in &mut bytes[start..end] {
                if !matches!(*byte, b'\n' | b'\r') {
                    *byte = b' ';
                }
            }
        }
    }
    String::from_utf8(bytes)
}

fn line_column_offset(source: &str, line: usize, column: usize) -> usize {
    let mut line_start = 0;
    for _ in 1..line {
        let Some(relative) = source
            .as_bytes()
            .get(line_start..)
            .and_then(|rest| rest.iter().position(|byte| *byte == b'\n'))
        else {
            return source.len();
        };
        line_start += relative + 1;
    }
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |relative| line_start + relative);
    let line_text = &source[line_start..line_end];
    let byte_column = line_text
        .char_indices()
        .nth(column)
        .map_or(line_text.len(), |(offset, _)| offset);
    line_start + byte_column
}

#[derive(Clone, Copy)]
struct SourceSpan {
    start_line: usize,
    start_column: usize,
    end_line: usize,
    end_column: usize,
}

#[derive(Default)]
struct CfgTestRanges {
    ranges: Vec<SourceSpan>,
}

impl<'ast> Visit<'ast> for CfgTestRanges {
    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        if expression_attrs(expression).is_some_and(has_cfg_test) {
            self.push_span(expression.span());
        } else {
            visit::visit_expr(self, expression);
        }
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        if item_attrs(item).is_some_and(has_cfg_test) {
            self.push_span(item.span());
        } else {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if impl_item_attrs(item).is_some_and(has_cfg_test) {
            self.push_span(item.span());
        } else {
            visit::visit_impl_item(self, item);
        }
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if trait_item_attrs(item).is_some_and(has_cfg_test) {
            self.push_span(item.span());
        } else {
            visit::visit_trait_item(self, item);
        }
    }

    fn visit_stmt_macro(&mut self, statement: &'ast syn::StmtMacro) {
        if has_cfg_test(&statement.attrs) {
            self.push_span(statement.span());
        } else {
            visit::visit_stmt_macro(self, statement);
        }
    }
}

fn expression_attrs(expression: &syn::Expr) -> Option<&[syn::Attribute]> {
    macro_rules! attributes {
        ($($variant:ident),+ $(,)?) => {
            match expression {
                $(syn::Expr::$variant(expression) => Some(expression.attrs.as_slice()),)+
                _ => None,
            }
        };
    }
    attributes!(
        Array, Assign, Async, Await, Binary, Block, Break, Call, Cast, Closure, Const, Continue,
        Field, ForLoop, Group, If, Index, Infer, Let, Lit, Loop, Macro, Match, MethodCall, Paren,
        Path, Range, RawAddr, Reference, Repeat, Return, Struct, Try, TryBlock, Tuple, Unary,
        Unsafe, While, Yield,
    )
}

pub(super) fn expression_is_cfg_test_only(expression: &syn::Expr) -> bool {
    expression_attrs(expression).is_some_and(has_cfg_test)
}

impl CfgTestRanges {
    fn push_span(&mut self, span: proc_macro2::Span) {
        let start = span.start();
        let end = span.end();
        self.ranges.push(SourceSpan {
            start_line: start.line,
            start_column: start.column,
            end_line: end.line,
            end_column: end.column,
        });
    }
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("cfg") && cfg_expr_is_test_only(&attribute.meta))
}

fn cfg_expr_is_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("cfg") => list
            .parse_args::<syn::Meta>()
            .is_ok_and(|expression| cfg_expr_is_test_only(&expression)),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|items| items.iter().any(cfg_expr_is_test_only)),
        syn::Meta::List(list) if list.path.is_ident("any") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|items| !items.is_empty() && items.iter().all(cfg_expr_is_test_only)),
        _ => false,
    }
}

fn item_attrs(item: &syn::Item) -> Option<&[syn::Attribute]> {
    match item {
        syn::Item::Const(item) => Some(&item.attrs),
        syn::Item::Enum(item) => Some(&item.attrs),
        syn::Item::ExternCrate(item) => Some(&item.attrs),
        syn::Item::Fn(item) => Some(&item.attrs),
        syn::Item::ForeignMod(item) => Some(&item.attrs),
        syn::Item::Impl(item) => Some(&item.attrs),
        syn::Item::Macro(item) => Some(&item.attrs),
        syn::Item::Mod(item) => Some(&item.attrs),
        syn::Item::Static(item) => Some(&item.attrs),
        syn::Item::Struct(item) => Some(&item.attrs),
        syn::Item::Trait(item) => Some(&item.attrs),
        syn::Item::TraitAlias(item) => Some(&item.attrs),
        syn::Item::Type(item) => Some(&item.attrs),
        syn::Item::Union(item) => Some(&item.attrs),
        syn::Item::Use(item) => Some(&item.attrs),
        _ => None,
    }
}

pub(super) fn impl_item_attrs(item: &syn::ImplItem) -> Option<&[syn::Attribute]> {
    match item {
        syn::ImplItem::Const(item) => Some(&item.attrs),
        syn::ImplItem::Fn(item) => Some(&item.attrs),
        syn::ImplItem::Type(item) => Some(&item.attrs),
        syn::ImplItem::Macro(item) => Some(&item.attrs),
        _ => None,
    }
}

pub(super) fn trait_item_attrs(item: &syn::TraitItem) -> Option<&[syn::Attribute]> {
    match item {
        syn::TraitItem::Const(item) => Some(&item.attrs),
        syn::TraitItem::Fn(item) => Some(&item.attrs),
        syn::TraitItem::Type(item) => Some(&item.attrs),
        syn::TraitItem::Macro(item) => Some(&item.attrs),
        _ => None,
    }
}
