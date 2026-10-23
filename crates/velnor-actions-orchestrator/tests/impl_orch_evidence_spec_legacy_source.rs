//! Exact identifier counting and byte-span masking for the legacy adapter.

use std::collections::BTreeMap;
use std::ops::Range;

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::{Expr, Pat};

pub(super) fn local_initializer(local: &syn::Local) -> Result<&Expr, syn::Error> {
    if !local.attrs.is_empty() {
        return Err(invalid("evidence binding attributes are unsupported"));
    }
    let Some(initializer) = &local.init else {
        return Err(invalid("evidence binding initializer is missing"));
    };
    if initializer.diverge.is_some() {
        return Err(invalid("evidence let-else is unsupported"));
    }
    Ok(&initializer.expr)
}

pub(super) fn plain_ident(pattern: &Pat, name: &str) -> bool {
    plain_ident_span(pattern, name).is_some()
}

pub(super) fn plain_ident_span(pattern: &Pat, name: &str) -> Option<Span> {
    let Pat::Ident(pattern) = pattern else {
        return None;
    };
    if pattern.attrs.is_empty()
        && pattern.by_ref.is_none()
        && pattern.mutability.is_none()
        && pattern.subpat.is_none()
        && pattern.ident == name
    {
        Some(pattern.ident.span())
    } else {
        None
    }
}

pub(super) fn immutable_ref_field_chain(expression: &Expr, root: &str, members: &[&str]) -> bool {
    match expression {
        Expr::Reference(reference)
            if reference.attrs.is_empty() && reference.mutability.is_none() =>
        {
            named_field_chain(&reference.expr, root, members)
        }
        _ => false,
    }
}

fn named_field_chain(expression: &Expr, root: &str, members: &[&str]) -> bool {
    if members.is_empty() {
        return false;
    }
    let mut current = expression;
    for expected in members.iter().rev() {
        let Expr::Field(field) = current else {
            return false;
        };
        let syn::Member::Named(member) = &field.member else {
            return false;
        };
        if !field.attrs.is_empty() || member != *expected {
            return false;
        }
        current = &field.base;
    }
    simple_expr_path(current, &[root])
}

pub(super) fn simple_expr_path(expression: &Expr, names: &[&str]) -> bool {
    matches!(
        expression,
        Expr::Path(path)
            if path.attrs.is_empty() && path.qself.is_none() && path_is(&path.path, names)
    )
}

pub(super) fn path_is(path: &syn::Path, names: &[&str]) -> bool {
    path.leading_colon.is_none()
        && path.segments.len() == names.len()
        && path.segments.iter().zip(names).all(|(segment, expected)| {
            segment.ident == *expected && matches!(&segment.arguments, syn::PathArguments::None)
        })
}

pub(super) fn identifier_counts(source: &str) -> Result<BTreeMap<String, usize>, syn::Error> {
    let tokens = source.parse::<TokenStream>()?;
    let mut counts = BTreeMap::new();
    count_token_identifiers(tokens, &mut counts);
    Ok(counts)
}

fn count_token_identifiers(tokens: TokenStream, counts: &mut BTreeMap<String, usize>) {
    for token in tokens {
        match token {
            TokenTree::Ident(ident) => *counts.entry(ident.unraw().to_string()).or_default() += 1,
            TokenTree::Group(group) => count_token_identifiers(group.stream(), counts),
            TokenTree::Literal(_) | TokenTree::Punct(_) => {}
        }
    }
}

pub(super) fn mask_exact_identifiers(
    source: &str,
    identifiers: Vec<(Span, &'static str)>,
) -> Result<String, syn::Error> {
    if identifiers.len() != 3 {
        return Err(invalid("expected exactly three passive evidence spans"));
    }
    let ranges = identifiers
        .into_iter()
        .map(|(span, expected)| identifier_range(source, span, expected))
        .collect::<Result<Vec<_>, _>>()?;
    validate_nonoverlap(&ranges)?;

    let mut bytes = source.as_bytes().to_vec();
    for (range, expected) in ranges {
        if range.end > bytes.len()
            || range.start >= range.end
            || source.get(range.clone()) != Some(expected)
        {
            return Err(invalid("verified source span is out of bounds"));
        }
        bytes[range].fill(b' ');
    }
    String::from_utf8(bytes).map_err(|_| invalid("masked source is not valid UTF-8"))
}

fn identifier_range(
    source: &str,
    span: Span,
    expected: &'static str,
) -> Result<(Range<usize>, &'static str), syn::Error> {
    let range = span.byte_range();
    let raw_identifier_prefix =
        range.start >= 2 && source.as_bytes().get(range.start - 2..range.start) == Some(&b"r#"[..]);
    if range.start >= range.end
        || range.end > source.len()
        || range.end - range.start != expected.len()
        || raw_identifier_prefix
        || source.get(range.clone()) != Some(expected)
    {
        return Err(syn::Error::new(span, "verified identifier span changed"));
    }
    Ok((range, expected))
}

fn validate_nonoverlap(ranges: &[(Range<usize>, &str)]) -> Result<(), syn::Error> {
    let mut ordered = ranges.iter().map(|(range, _)| range).collect::<Vec<_>>();
    ordered.sort_by_key(|range| range.start);
    if ordered.windows(2).any(|pair| pair[0].end > pair[1].start) {
        return Err(invalid("verified identifier spans overlap"));
    }
    Ok(())
}

pub(super) fn invalid(message: &str) -> syn::Error {
    syn::Error::new(Span::call_site(), message)
}
