//! Literal include discovery within invoked macro-rule expansions.

use std::error::Error;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{ItemConst, ItemFn, ItemImpl, ItemMacro, ItemStatic, ItemTrait, LitStr, Meta};

use super::Possibility;

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(in crate::impl_repo_test_registration) struct Definition {
    pub(in crate::impl_repo_test_registration) name: String,
    pub(in crate::impl_repo_test_registration) scope: String,
    pub(in crate::impl_repo_test_registration) source_order: usize,
    pub(in crate::impl_repo_test_registration) source: PathBuf,
    pub(in crate::impl_repo_test_registration) condition: Possibility,
    pub(in crate::impl_repo_test_registration) empty_matcher_only: bool,
    pub(in crate::impl_repo_test_registration) expansions: Vec<TokenStream>,
    pub(in crate::impl_repo_test_registration) test_bearing: bool,
}

pub(in crate::impl_repo_test_registration) struct Call {
    pub(in crate::impl_repo_test_registration) name: String,
    pub(in crate::impl_repo_test_registration) scope: String,
    pub(in crate::impl_repo_test_registration) source_order: usize,
    pub(in crate::impl_repo_test_registration) condition: Possibility,
    pub(in crate::impl_repo_test_registration) empty_arguments: bool,
}

pub(super) fn definition(
    name: String,
    condition: Possibility,
    scope: String,
    source_order: usize,
    tokens: &TokenStream,
    source: &Path,
) -> Outcome<Definition> {
    let arms = macro_arms(tokens)?;
    let empty_matcher_only = arms.len() == 1 && arms[0].0.is_empty();
    let mut expansions = Vec::new();
    let mut test_bearing = false;
    for (_, expansion) in arms {
        expansions.push(expansion.clone());
        test_bearing |= tokens_mark_test(&expansion)?;
    }
    Ok(Definition {
        name,
        scope,
        source_order,
        source: source.to_path_buf(),
        condition,
        empty_matcher_only,
        expansions,
        test_bearing,
    })
}

pub(super) fn call(
    path: &syn::Path,
    condition: Possibility,
    scope: String,
    source_order: usize,
    tokens: &TokenStream,
) -> Outcome<Call> {
    if path.segments.last().is_none() {
        return Err("macro invocation has an empty path".into());
    }
    let name = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    Ok(Call {
        name,
        scope,
        source_order,
        condition,
        empty_arguments: tokens.is_empty(),
    })
}

fn macro_arms(tokens: &TokenStream) -> Outcome<Vec<(TokenStream, TokenStream)>> {
    let mut input = tokens.clone().into_iter().peekable();
    let mut arms = Vec::new();
    while let Some(token) = input.next() {
        let TokenTree::Group(matcher) = token else {
            return Err("macro rule matcher is not delimited".into());
        };
        let first = input.next().ok_or("macro rule is missing =>")?;
        let second = input.next().ok_or("macro rule is missing =>")?;
        if !punct_is(&first, '=') || !punct_is(&second, '>') {
            return Err("macro rule is missing =>".into());
        }
        let Some(TokenTree::Group(expansion)) = input.next() else {
            return Err("macro rule expansion is not delimited".into());
        };
        arms.push((matcher.stream(), expansion.stream()));
        if matches!(input.peek(), Some(TokenTree::Punct(punct)) if matches!(punct.as_char(), ';' | ','))
        {
            input.next();
        }
    }
    if input.next().is_some() {
        return Err("macro rules contain unsupported trailing tokens".into());
    }
    Ok(arms)
}

pub(super) fn included_sources(
    definition: &Definition,
    source: &Path,
) -> Outcome<Vec<(PathBuf, Possibility)>> {
    let mut includes = Vec::new();
    for expansion in &definition.expansions {
        let syntax = match syn::parse2::<syn::File>(expansion.clone()) {
            Ok(syntax) => syntax,
            Err(error) if contains_include_invocation(expansion) => {
                parse_terminated_expansion(expansion).map_err(|terminated_error| {
                    let message = format!(
                        "cannot parse include-bearing macro expansion `{expansion}`: {error}; {terminated_error}"
                    );
                    Box::<dyn Error>::from(message)
                })?
            }
            Err(_) => continue,
        };
        let file_condition = super::cfg::attrs_possibility_in_test(&syntax.attrs)?;
        if file_condition == Possibility::Never {
            continue;
        }
        let mut visitor = IncludeCollector {
            source,
            condition: file_condition,
            includes: Vec::new(),
            found_include: false,
            error: None,
        };
        visitor.visit_file(&syntax);
        if let Some(error) = visitor.error {
            return Err(error.into());
        }
        if contains_include_invocation(expansion) && !visitor.found_include {
            return Err("include! in macro expansion is not a direct item source edge".into());
        }
        includes.extend(visitor.includes);
    }
    includes.sort_by(|left, right| left.0.cmp(&right.0));
    includes.dedup();
    Ok(includes)
}

fn parse_terminated_expansion(expansion: &TokenStream) -> Outcome<syn::File> {
    let terminated = format!("{expansion};")
        .parse::<TokenStream>()
        .map_err(|error| format!("cannot terminate macro expansion: {error}"))?;
    syn::parse2(terminated)
        .map_err(|error| format!("terminated expansion is invalid: {error}").into())
}

pub(super) fn has_include_invocation(definition: &Definition) -> bool {
    definition
        .expansions
        .iter()
        .any(contains_include_invocation)
}

struct IncludeCollector<'a> {
    source: &'a Path,
    condition: Possibility,
    includes: Vec<(PathBuf, Possibility)>,
    found_include: bool,
    error: Option<String>,
}

impl<'ast> Visit<'ast> for IncludeCollector<'_> {
    fn visit_item_fn(&mut self, _item: &'ast ItemFn) {}

    fn visit_item_const(&mut self, _item: &'ast ItemConst) {}

    fn visit_item_static(&mut self, _item: &'ast ItemStatic) {}

    fn visit_item_impl(&mut self, _item: &'ast ItemImpl) {}

    fn visit_item_trait(&mut self, _item: &'ast ItemTrait) {}

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        match super::cfg::attrs_possibility_in_test(&item.attrs) {
            Ok(condition) => {
                let previous = self.condition;
                self.condition = combine(previous, condition);
                if self.condition != Possibility::Never {
                    visit::visit_item_mod(self, item);
                }
                self.condition = previous;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn visit_item_macro(&mut self, item: &'ast ItemMacro) {
        if !item.mac.path.is_ident("include") {
            return;
        }
        self.found_include = true;
        let condition = match super::cfg::attrs_possibility_in_test(&item.attrs) {
            Ok(condition) => combine(self.condition, condition),
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        if condition == Possibility::Never {
            return;
        }
        match include_source(&item.mac.tokens, self.source) {
            Ok(path) => self.includes.push((path, condition)),
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

fn contains_include_invocation(tokens: &TokenStream) -> bool {
    let mut input = tokens.clone().into_iter().peekable();
    while let Some(token) = input.next() {
        if matches!(&token, TokenTree::Ident(ident) if ident == "include")
            && matches!(input.peek(), Some(TokenTree::Punct(punct)) if punct.as_char() == '!')
        {
            return true;
        }
        if let TokenTree::Group(group) = token
            && contains_include_invocation(&group.stream())
        {
            return true;
        }
    }
    false
}

pub(super) fn tokens_mark_test(tokens: &TokenStream) -> Outcome<bool> {
    let mut input = tokens.clone().into_iter().peekable();
    while let Some(token) = input.next() {
        if punct_is(&token, '#')
            && let Some(TokenTree::Group(group)) = input.peek()
            && group.delimiter() == proc_macro2::Delimiter::Bracket
        {
            let meta = syn::parse2::<Meta>(group.stream())?;
            if super::cfg::meta_marks_test(&meta)? {
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

pub(super) fn include_source(tokens: &TokenStream, source: &Path) -> Outcome<PathBuf> {
    let literal: LitStr = syn::parse2(tokens.clone())?;
    Ok(source
        .parent()
        .ok_or("include source has no parent")?
        .join(literal.value()))
}

fn punct_is(token: &TokenTree, expected: char) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == expected)
}

fn combine(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Never, _) | (_, Possibility::Never) => Possibility::Never,
        (Possibility::Always, Possibility::Always) => Possibility::Always,
        _ => Possibility::Sometimes,
    }
}
