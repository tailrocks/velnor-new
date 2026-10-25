use super::{QueryScope, Token, TokenKind};

pub(super) fn has_unsupported_query_shape(tokens: &[Token<'_>]) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.kind,
            TokenKind::Word("with" | "union" | "intersect" | "except")
        )
    })
}

pub(super) fn parentheses(tokens: &[Token<'_>]) -> Option<(Vec<usize>, Vec<Option<usize>>)> {
    let mut depths = vec![0; tokens.len()];
    let mut pairs = vec![None; tokens.len()];
    let mut opening = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        depths[index] = opening.len();
        match token.kind {
            TokenKind::Symbol(b'(') => opening.push(index),
            TokenKind::Symbol(b')') => {
                let open = opening.pop()?;
                pairs[open] = Some(index);
                pairs[index] = Some(open);
            }
            _ => {}
        }
    }
    opening.is_empty().then_some((depths, pairs))
}

pub(super) fn query_scopes(
    tokens: &[Token<'_>],
    depths: &[usize],
    pairs: &[Option<usize>],
) -> Option<Vec<QueryScope>> {
    let mut scopes = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.kind, TokenKind::Word("select")) {
            continue;
        }
        let end = select_end(tokens, depths, pairs, index)?;
        scopes.push(QueryScope {
            select: index,
            end,
            depth: depths[index],
            parent: None,
            bindings: [None, None],
        });
    }
    for child in 0..scopes.len() {
        let parent = nearest_parent(&scopes, child);
        scopes[child].parent = parent;
    }
    Some(scopes)
}

fn select_end(
    tokens: &[Token<'_>],
    depths: &[usize],
    pairs: &[Option<usize>],
    select: usize,
) -> Option<usize> {
    for open in (0..select).rev() {
        if matches!(tokens[open].kind, TokenKind::Symbol(b'('))
            && pairs[open].is_some_and(|close| close > select)
        {
            return pairs[open];
        }
    }
    Some(
        (select + 1..tokens.len())
            .find(|index| {
                depths[*index] == depths[select]
                    && matches!(tokens[*index].kind, TokenKind::Symbol(b';'))
            })
            .unwrap_or(tokens.len()),
    )
}

fn nearest_parent(scopes: &[QueryScope], child: usize) -> Option<usize> {
    let start = scopes[child].select;
    (0..scopes.len())
        .filter(|candidate| {
            *candidate != child
                && scopes[*candidate].select < start
                && start < scopes[*candidate].end
        })
        .max_by_key(|candidate| scopes[*candidate].select)
}

pub(super) fn token_scope(scopes: &[QueryScope], token: usize) -> Option<usize> {
    scopes
        .iter()
        .enumerate()
        .filter(|(_, scope)| scope.select <= token && token < scope.end)
        .max_by_key(|(_, scope)| scope.select)
        .map(|(index, _)| index)
}
