//! Cargo fetch detection in macro token bodies.

use proc_macro2::{Group, Ident, Punct, Spacing, TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::visit::Visit;

pub(super) fn macro_tokens_include_cargo_fetch(tokens: &TokenStream) -> bool {
    let normalized = normalize_metavariables(tokens.clone());
    if expression_stream_has_fetch(normalized.clone()) {
        return true;
    }
    let expression_list =
        syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    if syn::parse::Parser::parse2(expression_list, normalized.clone())
        .is_ok_and(|expressions| expressions.iter().any(expression_has_fetch))
    {
        return true;
    }
    let block = format!("{{{normalized}}}");
    if syn::parse_str::<syn::Expr>(&block).is_ok_and(|expression| expression_has_fetch(&expression))
        || semicolon_segments_have_fetch(normalized)
        || command_builder_has_fetch(tokens.clone())
    {
        return true;
    }
    tokens.clone().into_iter().any(|token| match token {
        TokenTree::Group(group) => macro_tokens_include_cargo_fetch(&group.stream()),
        TokenTree::Ident(_) | TokenTree::Literal(_) | TokenTree::Punct(_) => false,
    })
}

fn command_builder_has_fetch(tokens: TokenStream) -> bool {
    let tokens = tokens.into_iter().collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(index, token)| {
        if let Some(end) = cargo_constructor_end(&tokens, index) {
            return builder_chain_has_fetch(&tokens, end);
        }
        let TokenTree::Group(group) = token else {
            return false;
        };
        (transparent_cargo_receiver(group.stream()) && builder_chain_has_fetch(&tokens, index + 1))
            || command_builder_has_fetch(group.stream())
    })
}

fn transparent_cargo_receiver(tokens: TokenStream) -> bool {
    let tokens = tokens.into_iter().collect::<Vec<_>>();
    if tokens.len() == 1
        && let Some(TokenTree::Group(group)) = tokens.first()
        && matches!(
            group.delimiter(),
            proc_macro2::Delimiter::Parenthesis | proc_macro2::Delimiter::None
        )
    {
        return transparent_cargo_receiver(group.stream());
    }
    let Some(mut index) = cargo_constructor_end(&tokens, 0) else {
        return false;
    };
    while index < tokens.len() {
        if !is_punctuation(tokens.get(index), '.')
            || !matches!(tokens.get(index + 1), Some(TokenTree::Ident(_)))
            || !matches!(tokens.get(index + 2), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Parenthesis)
        {
            return false;
        }
        index += 3;
    }
    true
}

fn cargo_constructor_end(tokens: &[TokenTree], index: usize) -> Option<usize> {
    let mut segment_index = index;
    if is_path_separator(tokens, segment_index) {
        segment_index += 2;
    }
    if !matches!(tokens.get(segment_index), Some(TokenTree::Ident(_))) {
        return None;
    }
    let mut cursor = segment_index;
    loop {
        let TokenTree::Ident(segment) = tokens.get(cursor)? else {
            return None;
        };
        cursor += 1;
        if segment.unraw() == "Command"
            && is_path_separator(tokens, cursor)
            && is_identifier(tokens.get(cursor + 2), "new")
        {
            let arguments_index = cursor + 3;
            let Some(TokenTree::Group(arguments)) = tokens.get(arguments_index) else {
                return None;
            };
            if arguments.delimiter() != proc_macro2::Delimiter::Parenthesis {
                return None;
            }
            let expression_list =
                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
            let expressions =
                syn::parse::Parser::parse2(expression_list, arguments.stream()).ok()?;
            return (expressions.len() == 1
                && expressions
                    .first()
                    .and_then(super::literal_string)
                    .is_some_and(|program| program == "cargo"))
            .then_some(arguments_index + 1);
        }
        if !is_path_separator(tokens, cursor) {
            return None;
        }
        cursor += 2;
    }
}

fn builder_chain_has_fetch(tokens: &[TokenTree], mut index: usize) -> bool {
    while is_punctuation(tokens.get(index), '.') {
        let Some(TokenTree::Ident(method)) = tokens.get(index + 1) else {
            return false;
        };
        let Some(TokenTree::Group(arguments)) = tokens.get(index + 2) else {
            return false;
        };
        if method.unraw() == "arg" && argument_is_fetch(arguments)
            || method.unraw() == "args" && argument_array_has_fetch(arguments)
        {
            return true;
        }
        index += 3;
    }
    false
}

fn argument_is_fetch(arguments: &Group) -> bool {
    if arguments.delimiter() != proc_macro2::Delimiter::Parenthesis {
        return false;
    }
    let expression_list =
        syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    syn::parse::Parser::parse2(expression_list, arguments.stream()).is_ok_and(|expressions| {
        expressions.len() == 1
            && expressions
                .first()
                .and_then(super::literal_string)
                .is_some_and(|argument| argument == "fetch")
    })
}

fn argument_array_has_fetch(arguments: &Group) -> bool {
    if arguments.delimiter() != proc_macro2::Delimiter::Parenthesis {
        return false;
    }
    let tokens = arguments.stream().into_iter().collect::<Vec<_>>();
    let expression_list =
        syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    if let Ok(expressions) = syn::parse::Parser::parse2(expression_list, arguments.stream()) {
        return expressions.len() == 1
            && expressions.first().is_some_and(expression_array_has_fetch);
    }
    let Some((array, end)) = first_array_argument(&tokens, 0) else {
        return false;
    };
    let complete =
        end == tokens.len() || (end + 1 == tokens.len() && is_punctuation(tokens.get(end), ','));
    complete && array_has_fetch(&array)
}

fn expression_array_has_fetch(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Array(array) => array
            .elems
            .iter()
            .any(|element| super::literal_string(element).is_some_and(|value| value == "fetch")),
        syn::Expr::Repeat(repeat) => array_repeat_contains_fetch(&repeat.expr, &repeat.len),
        syn::Expr::Paren(expression) => expression_array_has_fetch(&expression.expr),
        syn::Expr::Group(expression) => expression_array_has_fetch(&expression.expr),
        syn::Expr::Reference(expression) => expression_array_has_fetch(&expression.expr),
        _ => false,
    }
}

pub(super) fn array_repeat_contains_fetch(value: &syn::Expr, length: &syn::Expr) -> bool {
    super::literal_string(value).is_some_and(|argument| argument == "fetch")
        && !statically_zero_array_length(length)
}

fn statically_zero_array_length(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Lit(expression) => match &expression.lit {
            syn::Lit::Int(length) => length
                .base10_parse::<u128>()
                .is_ok_and(|length| length == 0),
            _ => false,
        },
        syn::Expr::Paren(expression) => statically_zero_array_length(&expression.expr),
        syn::Expr::Group(expression) => statically_zero_array_length(&expression.expr),
        _ => false,
    }
}

fn first_array_argument(tokens: &[TokenTree], index: usize) -> Option<(Group, usize)> {
    match tokens.get(index)? {
        TokenTree::Punct(punctuation) if punctuation.as_char() == '&' => {
            let next = if is_identifier(tokens.get(index + 1), "mut") {
                index + 2
            } else {
                index + 1
            };
            first_array_argument(tokens, next)
        }
        TokenTree::Group(group) if group.delimiter() == proc_macro2::Delimiter::Bracket => {
            Some((group.clone(), index + 1))
        }
        TokenTree::Group(group) if group.delimiter() == proc_macro2::Delimiter::Parenthesis => {
            let nested = group.stream().into_iter().collect::<Vec<_>>();
            let (array, end) = first_array_argument(&nested, 0)?;
            (end == nested.len()).then_some((array, index + 1))
        }
        _ => None,
    }
}

fn array_has_fetch(array: &Group) -> bool {
    let expression_tokens = TokenStream::from(TokenTree::Group(array.clone()));
    if let Ok(expression) = syn::parse2::<syn::Expr>(expression_tokens) {
        return expression_array_has_fetch(&expression);
    }
    let mut element = TokenStream::new();
    for token in array.stream() {
        if is_punctuation(Some(&token), ',') {
            if array_element_is_fetch(&element) {
                return true;
            }
            element = TokenStream::new();
        } else {
            element.extend([token]);
        }
    }
    array_element_is_fetch(&element)
}

fn array_element_is_fetch(element: &TokenStream) -> bool {
    syn::parse2::<syn::Expr>(element.clone())
        .ok()
        .and_then(|expression| super::literal_string(&expression))
        .is_some_and(|literal| literal == "fetch")
}

fn is_identifier(token: Option<&TokenTree>, expected: &str) -> bool {
    matches!(token, Some(TokenTree::Ident(identifier)) if identifier.unraw() == expected)
}

fn is_punctuation(token: Option<&TokenTree>, expected: char) -> bool {
    matches!(token, Some(TokenTree::Punct(punctuation)) if punctuation.as_char() == expected)
}

fn is_path_separator(tokens: &[TokenTree], index: usize) -> bool {
    is_punctuation(tokens.get(index), ':') && is_punctuation(tokens.get(index + 1), ':')
}

fn normalize_metavariables(tokens: TokenStream) -> TokenStream {
    let mut source = tokens.into_iter().peekable();
    let mut normalized = TokenStream::new();
    while let Some(token) = source.next() {
        match token {
            TokenTree::Punct(punctuation) if punctuation.as_char() == '$' => {
                if let Some(TokenTree::Ident(identifier)) = source.peek() {
                    let replacement = Ident::new("__scanner_metavariable", identifier.span());
                    source.next();
                    normalized.extend([TokenTree::Ident(replacement)]);
                } else {
                    normalized.extend([TokenTree::Punct(punctuation)]);
                }
            }
            TokenTree::Group(group) => {
                let mut replacement =
                    Group::new(group.delimiter(), normalize_metavariables(group.stream()));
                replacement.set_span(group.span());
                normalized.extend([TokenTree::Group(replacement)]);
            }
            token => normalized.extend([token]),
        }
    }
    normalized
}

fn expression_stream_has_fetch(tokens: TokenStream) -> bool {
    syn::parse2::<syn::Expr>(tokens).is_ok_and(|expression| expression_has_fetch(&expression))
}

fn expression_has_fetch(expression: &syn::Expr) -> bool {
    let mut visitor = super::CargoFetchCalls::default();
    visitor.visit_expr(expression);
    !visitor.hits.is_empty()
}

fn semicolon_segments_have_fetch(tokens: TokenStream) -> bool {
    let mut segment = TokenStream::new();
    for token in tokens {
        if matches!(&token, TokenTree::Punct(punctuation) if punctuation.as_char() == ';') {
            if statement_segment_has_fetch(segment) {
                return true;
            }
            segment = TokenStream::new();
        } else {
            segment.extend([token]);
        }
    }
    statement_segment_has_fetch(segment)
}

fn statement_segment_has_fetch(tokens: TokenStream) -> bool {
    let mut statement = tokens.clone();
    statement.extend([TokenTree::Punct(Punct::new(';', Spacing::Alone))]);
    syn::parse2::<syn::Stmt>(statement).is_ok_and(|statement| {
        let mut visitor = super::CargoFetchCalls::default();
        visitor.visit_stmt(&statement);
        !visitor.hits.is_empty()
    }) || expression_stream_has_fetch(tokens)
}
