//! Syntax scan for test attributes and literal `include!` sources.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, TokenTree};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, ItemFn, LitStr, Macro, Meta, Token};

type Outcome<T> = Result<T, Box<dyn Error>>;

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Possibility {
    Never,
    Always,
    Sometimes,
}

#[derive(Default)]
pub(super) struct SourceFindings {
    pub test_bearing: bool,
    /// Literal includes found as active macro invocations in parsed Rust syntax.
    pub includes: Vec<PathBuf>,
    /// Includes inside macro token bodies; Cargo dep-info determines expansion.
    pub macro_includes: Vec<PathBuf>,
}

struct SourceVisitor<'a> {
    source: &'a Path,
    findings: SourceFindings,
    error: Option<String>,
}

impl<'ast> Visit<'ast> for SourceVisitor<'_> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        match attrs_possible_in_test(&node.attrs) {
            Ok(true) => match attributes_mark_test(&node.attrs) {
                Ok(found) => self.findings.test_bearing |= found,
                Err(error) => self.error = Some(error.to_string()),
            },
            Ok(false) => return,
            Err(error) => self.error = Some(error.to_string()),
        }
        visit::visit_item_fn(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        match attrs_possible_in_test(&node.attrs) {
            Ok(true) => visit::visit_item_mod(self, node),
            Ok(false) => {}
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        match attrs_possible_in_test(&node.attrs) {
            Ok(true) => visit::visit_item_macro(self, node),
            Ok(false) => {}
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn visit_macro(&mut self, node: &'ast Macro) {
        match macro_findings(node, self.source) {
            Ok((test_bearing, includes, macro_includes)) => {
                self.findings.test_bearing |= test_bearing;
                self.findings.includes.extend(includes);
                self.findings.macro_includes.extend(macro_includes);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
        visit::visit_macro(self, node);
    }
}

pub(super) fn source_findings(path: &Path) -> Outcome<SourceFindings> {
    let source = fs::read_to_string(path)?;
    let syntax = syn::parse_file(&source)?;
    if !attrs_possible_in_test(&syntax.attrs)? {
        return Ok(SourceFindings::default());
    }
    let mut visitor = SourceVisitor {
        source: path,
        findings: SourceFindings::default(),
        error: None,
    };
    visitor.visit_file(&syntax);
    if let Some(error) = visitor.error {
        return Err(error.into());
    }
    Ok(visitor.findings)
}

fn attributes_mark_test(attributes: &[Attribute]) -> Outcome<bool> {
    for attribute in attributes {
        if attribute_marks_test(&attribute.meta)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn attribute_marks_test(meta: &Meta) -> Outcome<bool> {
    match meta {
        Meta::Path(path) => Ok(is_test_path(path)),
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
            let nested = parser.parse2(list.tokens.clone())?;
            let mut items = nested.iter();
            let condition = items.next().ok_or("cfg_attr has no condition")?;
            if test_condition_state(condition)? == Possibility::Never {
                return Ok(false);
            }
            for nested in items {
                if attribute_marks_test(nested)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

pub(super) fn attrs_possible_in_test(attributes: &[Attribute]) -> Outcome<bool> {
    for attribute in attributes {
        match &attribute.meta {
            Meta::List(list) if list.path.is_ident("cfg") => {
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                let conditions = parser.parse2(list.tokens.clone())?;
                let condition = conditions.first().ok_or("cfg has no predicate")?;
                if test_condition_state(condition)? == Possibility::Never {
                    return Ok(false);
                }
            }
            Meta::List(list) if list.path.is_ident("cfg_attr") => {
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                let nested = parser.parse2(list.tokens.clone())?;
                let mut items = nested.iter();
                let condition = items.next().ok_or("cfg_attr has no condition")?;
                if test_condition_state(condition)? != Possibility::Never {
                    let remaining = items.cloned().collect::<Vec<_>>();
                    if !attrs_possible_in_test(
                        &remaining.iter().map(meta_as_attr).collect::<Vec<_>>(),
                    )? {
                        return Ok(false);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(true)
}

fn meta_as_attr(meta: &Meta) -> Attribute {
    syn::parse_quote!(#meta)
}

pub(super) fn test_condition_state(meta: &Meta) -> Outcome<Possibility> {
    match meta {
        Meta::Path(path) if path.is_ident("test") => Ok(Possibility::Always),
        Meta::List(list) if list.path.is_ident("all") => {
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
            let mut result = Possibility::Always;
            for nested in parser.parse2(list.tokens.clone())? {
                match test_condition_state(&nested)? {
                    Possibility::Never => return Ok(Possibility::Never),
                    Possibility::Sometimes => result = Possibility::Sometimes,
                    Possibility::Always => {}
                }
            }
            Ok(result)
        }
        Meta::List(list) if list.path.is_ident("any") => {
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
            let mut result = Possibility::Never;
            for nested in parser.parse2(list.tokens.clone())? {
                match test_condition_state(&nested)? {
                    Possibility::Always => return Ok(Possibility::Always),
                    Possibility::Sometimes => result = Possibility::Sometimes,
                    Possibility::Never => {}
                }
            }
            Ok(result)
        }
        Meta::List(list) if list.path.is_ident("not") => {
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
            let nested = parser.parse2(list.tokens.clone())?;
            let condition = nested.first().ok_or("cfg not has no predicate")?;
            Ok(match test_condition_state(condition)? {
                Possibility::Never => Possibility::Always,
                Possibility::Always => Possibility::Never,
                Possibility::Sometimes => Possibility::Sometimes,
            })
        }
        Meta::NameValue(value) if value.path.is_ident("test") => Ok(Possibility::Never),
        _ => Ok(Possibility::Sometimes),
    }
}

fn is_test_path(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "test" | "rstest" | "test_case"
        )
    })
}

fn macro_findings(node: &Macro, source: &Path) -> Outcome<(bool, Vec<PathBuf>, Vec<PathBuf>)> {
    let mut includes = Vec::new();
    let mut macro_includes = Vec::new();
    if node.path.is_ident("include") {
        includes.push(include_source(&node.tokens, source)?);
    }
    collect_macro_includes(&node.tokens, source, &mut macro_includes)?;
    Ok((tokens_mark_test(&node.tokens)?, includes, macro_includes))
}

fn collect_macro_includes(
    tokens: &proc_macro2::TokenStream,
    source: &Path,
    includes: &mut Vec<PathBuf>,
) -> Outcome<()> {
    let mut iter = tokens.clone().into_iter().peekable();
    while let Some(token) = iter.next() {
        if let TokenTree::Ident(ident) = &token
            && ident == "include"
            && matches!(iter.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == '!')
        {
            iter.next();
            if let Some(TokenTree::Group(group)) = iter.next()
                && group.delimiter() == Delimiter::Parenthesis
            {
                includes.push(include_source(&group.stream(), source)?);
            }
        }
        if let TokenTree::Group(group) = token {
            collect_macro_includes(&group.stream(), source, includes)?;
        }
    }
    Ok(())
}

pub(super) fn include_source(tokens: &proc_macro2::TokenStream, source: &Path) -> Outcome<PathBuf> {
    let literal: LitStr = syn::parse2(tokens.clone())?;
    Ok(source
        .parent()
        .ok_or("include source has no parent")?
        .join(literal.value()))
}

fn tokens_mark_test(tokens: &proc_macro2::TokenStream) -> Outcome<bool> {
    let mut iter = tokens.clone().into_iter().peekable();
    while let Some(token) = iter.next() {
        if let TokenTree::Punct(punct) = &token
            && punct.as_char() == '#'
            && let Some(TokenTree::Group(group)) = iter.peek()
            && group.delimiter() == Delimiter::Bracket
        {
            let meta = syn::parse2::<Meta>(group.stream())?;
            if attribute_marks_test(&meta)? {
                return Ok(true);
            }
        }
        if let TokenTree::Group(group) = token
            && tokens_mark_test(&group.stream())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}
