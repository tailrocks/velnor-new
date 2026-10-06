//! Parsers for `include!` content whose Rust syntactic position is macro-defined.

pub(super) enum ContextualSource {
    Items(syn::File),
    Expression(syn::Expr),
}

pub(super) fn parse_contextual(source: &str) -> Result<ContextualSource, syn::Error> {
    if let Ok(items) = syn::parse_file(source) {
        return Ok(ContextualSource::Items(items));
    }
    if let Ok(expression) = syn::parse_str::<syn::Expr>(source) {
        return Ok(ContextualSource::Expression(expression));
    }
    let block = format!("{{\n{source}\n}}");
    syn::parse_str::<syn::Expr>(&block).map(ContextualSource::Expression)
}
