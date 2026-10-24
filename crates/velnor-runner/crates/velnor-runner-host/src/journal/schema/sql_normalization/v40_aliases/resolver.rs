use super::scopes::token_scope;
use super::{Alias, Binding, QueryScope, QuoteStyle, SourceClause, TableSource, Token, TokenKind};

pub(super) fn collect_declarations(
    sql: &str,
    tokens: &[Token<'_>],
    depths: &[usize],
    scopes: &mut [QueryScope],
) -> Option<Vec<bool>> {
    let mut declarations = vec![false; tokens.len()];
    for scope_index in 0..scopes.len() {
        let mut in_from = false;
        let mut index = scopes[scope_index].select + 1;
        while index < scopes[scope_index].end {
            if token_scope(scopes, index) != Some(scope_index)
                || depths[index] != scopes[scope_index].depth
            {
                index += 1;
                continue;
            }
            match source_clause(sql, tokens, index, &mut in_from)? {
                SourceClause::Table(source) => {
                    record_source(tokens, scopes, scope_index, &source, &mut declarations)?;
                    index = source.next;
                }
                SourceClause::Ignore => index += 1,
            }
        }
    }
    Some(declarations)
}

fn source_clause(
    sql: &str,
    tokens: &[Token<'_>],
    index: usize,
    in_from: &mut bool,
) -> Option<SourceClause> {
    match tokens[index].kind {
        TokenKind::Word("from") => {
            *in_from = true;
            table_source(sql, tokens, index + 1).map(SourceClause::Table)
        }
        TokenKind::Word("join") | TokenKind::Symbol(b',') if *in_from => {
            table_source(sql, tokens, index + 1).map(SourceClause::Table)
        }
        TokenKind::Word("where" | "group" | "having" | "order" | "limit" | "offset" | "window") => {
            *in_from = false;
            Some(SourceClause::Ignore)
        }
        _ => Some(SourceClause::Ignore),
    }
}

fn table_source(sql: &str, tokens: &[Token<'_>], start: usize) -> Option<TableSource> {
    let first = tokens.get(start)?;
    if matches!(first.kind, TokenKind::Symbol(b'(')) {
        return None;
    }
    let mut table_index = start;
    let mut table_name = identifier_name(first.kind)?;
    if matches!(
        tokens.get(start + 1).map(|token| token.kind),
        Some(TokenKind::Symbol(b'.'))
    ) {
        table_index = start + 2;
        table_name = identifier_name(tokens.get(table_index)?.kind)?;
        if matches!(
            tokens.get(table_index + 1).map(|token| token.kind),
            Some(TokenKind::Symbol(b'.'))
        ) {
            return None;
        }
    }
    let after_table = table_index + 1;
    if matches!(
        tokens.get(after_table).map(|token| token.kind),
        Some(TokenKind::Symbol(b'('))
    ) {
        return None;
    }
    table_source_with_alias(sql, tokens, table_index, after_table, table_name)
}

fn table_source_with_alias(
    sql: &str,
    tokens: &[Token<'_>],
    table_index: usize,
    after_table: usize,
    table_name: &str,
) -> Option<TableSource> {
    let target = target_table_alias(table_name);
    if matches!(
        tokens.get(after_table).map(|token| token.kind),
        Some(TokenKind::Word("as"))
    ) {
        if !separated(sql, tokens[table_index].end, tokens[after_table].start) {
            return None;
        }
        let alias_index = after_table + 1;
        let alias = Alias::from_name(identifier_name(tokens.get(alias_index)?.kind)?);
        if !separated(sql, tokens[after_table].end, tokens[alias_index].start) {
            return None;
        }
        return Some(TableSource {
            next: alias_index + 1,
            table: target,
            alias,
            alias_token: alias.map(|_| alias_index),
        });
    }
    let alias = tokens
        .get(after_table)
        .and_then(|token| identifier_name(token.kind))
        .and_then(Alias::from_name);
    if let Some(alias) = alias {
        if !separated(sql, tokens[table_index].end, tokens[after_table].start) {
            return None;
        }
        return Some(TableSource {
            next: after_table + 1,
            table: target,
            alias: Some(alias),
            alias_token: Some(after_table),
        });
    }
    let implicit = Alias::from_name(table_name);
    Some(TableSource {
        next: after_table,
        table: target,
        alias: implicit,
        alias_token: None,
    })
}

fn identifier_name(kind: TokenKind<'_>) -> Option<&str> {
    match kind {
        TokenKind::Word(name) => Some(name),
        TokenKind::QuotedIdentifier { name, .. } => name,
        _ => None,
    }
}

fn target_table_alias(table: &str) -> Option<Alias> {
    match table.to_ascii_lowercase().as_str() {
        "physical_attempt_current" | "physical_attempt_jit_conflict_recovery_progress" => {
            Some(Alias::Current)
        }
        "physical_attempt_jit_conflict_recovery_claims" => Some(Alias::Conflict),
        _ => None,
    }
}

fn record_source(
    tokens: &[Token<'_>],
    scopes: &mut [QueryScope],
    scope_index: usize,
    source: &TableSource,
    declarations: &mut [bool],
) -> Option<()> {
    let Some(alias) = source.alias else {
        return Some(());
    };
    let binding = if source.table == Some(alias) {
        Binding::SourceOwned
    } else {
        Binding::Shadow
    };
    let slot = alias.slot();
    if scopes[scope_index].bindings[slot].is_some() {
        return None;
    }
    scopes[scope_index].bindings[slot] = Some(binding);
    if matches!(binding, Binding::SourceOwned)
        && source.alias_token.is_some_and(|index| {
            matches!(
                tokens[index].kind,
                TokenKind::QuotedIdentifier {
                    style: QuoteStyle::Double,
                    ..
                }
            )
        })
    {
        declarations[source.alias_token?] = true;
    }
    Some(())
}

pub(super) fn mark_qualified_references(
    sql: &str,
    tokens: &[Token<'_>],
    scopes: &[QueryScope],
    declarations: &mut [bool],
) {
    for (index, token) in tokens.iter().enumerate() {
        let TokenKind::QuotedIdentifier {
            name: Some(name),
            style: QuoteStyle::Double,
        } = token.kind
        else {
            continue;
        };
        let Some(alias) = Alias::from_name(name) else {
            continue;
        };
        if declarations[index] || !qualified_reference(sql, tokens, index) {
            continue;
        }
        let Some(scope) = token_scope(scopes, index) else {
            continue;
        };
        if resolves_to_source_owned(scopes, scope, alias) {
            declarations[index] = true;
        }
    }
}

fn qualified_reference(sql: &str, tokens: &[Token<'_>], index: usize) -> bool {
    let Some(next) = tokens.get(index + 1) else {
        return false;
    };
    if !matches!(next.kind, TokenKind::Symbol(b'.'))
        || !sql[tokens[index].end..next.start]
            .bytes()
            .all(|byte| byte.is_ascii_whitespace())
    {
        return false;
    }
    if index == 0 {
        return true;
    }
    let previous = tokens[index - 1];
    let adjacent = !sql[previous.end..tokens[index].start]
        .bytes()
        .any(|byte| byte.is_ascii_whitespace());
    !adjacent
        || !matches!(
            previous.kind,
            TokenKind::Word(_)
                | TokenKind::QuotedIdentifier { .. }
                | TokenKind::String
                | TokenKind::Symbol(b'.')
        )
}

fn resolves_to_source_owned(scopes: &[QueryScope], mut scope: usize, alias: Alias) -> bool {
    loop {
        if let Some(binding) = scopes[scope].bindings[alias.slot()] {
            return matches!(binding, Binding::SourceOwned);
        }
        let Some(parent) = scopes[scope].parent else {
            return false;
        };
        scope = parent;
    }
}

pub(super) fn apply_replacements(sql: &str, tokens: &[Token<'_>], replacements: &[bool]) -> String {
    let mut output = String::with_capacity(sql.len());
    let mut cursor = 0;
    for (index, token) in tokens.iter().enumerate() {
        if !replacements[index] {
            continue;
        }
        let Some(alias) = tokens[index].kind.quoted_alias() else {
            continue;
        };
        output.push_str(&sql[cursor..token.start]);
        output.push_str(alias.name());
        cursor = token.end;
    }
    if cursor == 0 {
        return sql.to_owned();
    }
    output.push_str(&sql[cursor..]);
    output
}

fn separated(sql: &str, left_end: usize, right_start: usize) -> bool {
    sql[left_end..right_start]
        .bytes()
        .any(|byte| byte.is_ascii_whitespace())
}

impl TokenKind<'_> {
    fn quoted_alias(self) -> Option<Alias> {
        match self {
            Self::QuotedIdentifier {
                name: Some(name), ..
            } => Alias::from_name(name),
            _ => None,
        }
    }
}
