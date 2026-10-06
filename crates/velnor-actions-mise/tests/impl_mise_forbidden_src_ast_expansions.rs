//! Only inspected source forms may reach the filesystem visitor.

pub(crate) fn macro_allowed(mac: &syn::Macro) -> bool {
    let names: Vec<_> = mac
        .path
        .segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect();
    if names == ["serde_json", "json"] {
        return true;
    }
    names.len() == 1
        && matches!(
            names[0].as_str(),
            "assert"
                | "assert_eq"
                | "assert_ne"
                | "debug_assert"
                | "debug_assert_eq"
                | "debug_assert_ne"
                | "concat"
                | "eprintln"
                | "format"
                | "include_str"
                | "include_bytes"
                | "json"
                | "matches"
                | "unreachable"
                | "vec"
                | "write"
                | "writeln"
                | "env"
                | "option_env"
                | "stringify"
                | "cfg"
        )
}

pub(crate) fn attribute_allowed(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("derive") {
        let parser = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated;
        return attr.parse_args_with(parser).is_ok_and(|paths| {
            paths.iter().all(|p| {
                p.get_ident().is_some_and(|n| {
                    matches!(
                        n.to_string().as_str(),
                        "Debug"
                            | "Clone"
                            | "Copy"
                            | "PartialEq"
                            | "Eq"
                            | "Hash"
                            | "Default"
                            | "Serialize"
                            | "Deserialize"
                    )
                })
            })
        });
    }
    if attr.path().is_ident("path") {
        return matches!(&attr.meta, syn::Meta::NameValue(v) if matches!(&v.value, syn::Expr::Lit(l)
            if matches!(&l.lit, syn::Lit::Str(s) if safe_source_path(&s.value()))));
    }
    attr.path().get_ident().is_some_and(|n| {
        matches!(
            n.to_string().as_str(),
            "cfg"
                | "expect"
                | "must_use"
                | "non_exhaustive"
                | "path"
                | "serde"
                | "test"
                | "doc"
                | "allow"
                | "deny"
                | "forbid"
        )
    })
}

fn safe_source_path(value: &str) -> bool {
    !value.starts_with('/')
        && !value.contains('\\')
        && value.ends_with(".rs")
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != ".." && part != ".")
}
