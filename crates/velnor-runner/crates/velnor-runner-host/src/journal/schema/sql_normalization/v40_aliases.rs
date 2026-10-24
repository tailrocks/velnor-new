#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Alias {
    Current,
    Conflict,
}

impl Alias {
    fn from_name(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("current") {
            Some(Self::Current)
        } else if name.eq_ignore_ascii_case("conflict") {
            Some(Self::Conflict)
        } else {
            None
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Conflict => "conflict",
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::Current => 0,
            Self::Conflict => 1,
        }
    }
}

#[derive(Clone, Copy)]
enum QuoteStyle {
    Double,
    Backtick,
    Bracket,
}

#[derive(Clone, Copy)]
enum TokenKind<'a> {
    Word(&'a str),
    QuotedIdentifier {
        name: Option<&'a str>,
        style: QuoteStyle,
    },
    String,
    Symbol(u8),
    Other,
}

#[derive(Clone, Copy)]
struct Token<'a> {
    start: usize,
    end: usize,
    kind: TokenKind<'a>,
}

#[derive(Clone, Copy)]
enum Binding {
    SourceOwned,
    Shadow,
}

struct QueryScope {
    select: usize,
    end: usize,
    depth: usize,
    parent: Option<usize>,
    bindings: [Option<Binding>; 2],
}

struct TableSource {
    next: usize,
    table: Option<Alias>,
    alias: Option<Alias>,
    alias_token: Option<usize>,
}

enum SourceClause {
    Ignore,
    Table(TableSource),
}

#[path = "v40_aliases/lexer.rs"]
mod lexer;
#[path = "v40_aliases/resolver.rs"]
mod resolver;
#[path = "v40_aliases/scopes.rs"]
mod scopes;

pub(super) fn canonicalize(sql: &str) -> String {
    let Some(tokens) = lexer::tokenize(sql) else {
        return sql.to_owned();
    };
    if lexer::has_identifier_adjacency(&tokens) || scopes::has_unsupported_query_shape(&tokens) {
        return sql.to_owned();
    }
    let Some((depths, pairs)) = scopes::parentheses(&tokens) else {
        return sql.to_owned();
    };
    let Some(mut scopes) = scopes::query_scopes(&tokens, &depths, &pairs) else {
        return sql.to_owned();
    };
    if scopes.is_empty() {
        return sql.to_owned();
    }
    let Some(mut declarations) = resolver::collect_declarations(sql, &tokens, &depths, &mut scopes)
    else {
        return sql.to_owned();
    };
    resolver::mark_qualified_references(sql, &tokens, &scopes, &mut declarations);
    resolver::apply_replacements(sql, &tokens, &declarations)
}
