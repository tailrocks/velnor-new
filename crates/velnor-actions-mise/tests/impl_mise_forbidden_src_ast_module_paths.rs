//! Literal module targets remain inside explicitly inventoried ordinary roots.
use std::collections::HashSet;
use syn::{ItemMod, UseTree};
pub(crate) const SOURCE: &str = "crates/velnor-actions-mise/src/";
const TESTS: &str = "crates/velnor-actions-mise/tests/";
const SUPPORT: &str = "crates/test_support/";

pub(crate) fn ordinary(path: &str) -> bool {
    [SOURCE, TESTS, SUPPORT]
        .iter()
        .any(|root| path.starts_with(root))
        && path.ends_with(".rs")
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(crate) fn module_directory(path: &str) -> String {
    let (parent, file) = path.rsplit_once('/').unwrap_or(("", path));
    if matches!(file, "lib.rs" | "main.rs" | "mod.rs") {
        parent.to_owned()
    } else {
        format!("{parent}/{}", file.trim_end_matches(".rs"))
    }
}

pub(crate) fn target(
    module: &ItemMod,
    directory: &str,
    attribute_directory: &str,
    inventory: &HashSet<String>,
) -> Option<String> {
    let attrs: Vec<_> = module
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("path"))
        .collect();
    if attrs.len() > 1 {
        return None;
    }
    if let Some(attr) = attrs.first() {
        let syn::Meta::NameValue(value) = &attr.meta else {
            return None;
        };
        let syn::Expr::Lit(literal) = &value.value else {
            return None;
        };
        let syn::Lit::Str(value) = &literal.lit else {
            return None;
        };
        // A path attribute is relative to the containing source file, supplied by collect.
        let normalized = resolve(attribute_directory, &value.value())?;
        return (ordinary(&normalized) && inventory.contains(&normalized)).then_some(normalized);
    }
    let file = format!("{directory}/{}.rs", module.ident);
    let nested = format!("{directory}/{}/mod.rs", module.ident);
    match (inventory.contains(&file), inventory.contains(&nested)) {
        (true, false) => Some(file),
        (false, true) => Some(nested),
        _ => None,
    }
}

fn resolve(parent: &str, literal: &str) -> Option<String> {
    if literal.starts_with('/') || literal.contains('\\') || !literal.ends_with(".rs") {
        return None;
    }
    let mut parts: Vec<_> = parent.split('/').collect();
    for part in literal.split('/') {
        match part {
            "" | "." => return None,
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

pub(crate) fn uses_test(tree: &UseTree, names: &HashSet<String>) -> bool {
    match tree {
        UseTree::Path(p) => names.contains(&p.ident.to_string()) || uses_test(&p.tree, names),
        UseTree::Name(n) => names.contains(&n.ident.to_string()),
        UseTree::Rename(n) => names.contains(&n.ident.to_string()),
        UseTree::Group(g) => g.items.iter().any(|tree| uses_test(tree, names)),
        UseTree::Glob(_) => true,
    }
}

pub(crate) fn mentions_test(module: &ItemMod) -> bool {
    struct Test(bool);
    impl<'ast> syn::visit::Visit<'ast> for Test {
        fn visit_path(&mut self, path: &'ast syn::Path) {
            self.0 |= path.is_ident("test");
            syn::visit::visit_path(self, path);
        }
    }
    module.attrs.iter().any(|attr| {
        if attr.path().is_ident("cfg_attr") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        let Ok(meta) = attr.parse_args::<syn::Meta>() else {
            return true;
        };
        let mut visitor = Test(false);
        syn::visit::Visit::visit_meta(&mut visitor, &meta);
        if let syn::Meta::List(list) = meta {
            let parser = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated;
            if let Ok(items) = list.parse_args_with(parser) {
                for item in items {
                    syn::visit::Visit::visit_meta(&mut visitor, &item);
                }
            }
        }
        visitor.0
    })
}

// Token literals stay intact; only actual identifier/path/import tokens confer reachability.
pub(crate) fn macro_test_path(mac: &syn::Macro, names: &HashSet<String>) -> bool {
    token_paths(mac, names, false)
}
fn token_paths(mac: &syn::Macro, names: &HashSet<String>, mut import: bool) -> bool {
    let tokens: Vec<String> = mac
        .tokens
        .clone()
        .into_iter()
        .map(|token| token.to_string())
        .collect();
    for (index, token) in tokens.iter().enumerate() {
        if token.starts_with("r#") && !token.contains('"') {
            return true;
        }
        if token == "use" {
            import = true;
        }
        if token == ";" {
            import = false;
        }
        if names.contains(token)
            && (import
                || (tokens.get(index + 1).is_some_and(|s| s == ":")
                    && tokens.get(index + 2).is_some_and(|s| s == ":")))
        {
            return true;
        }
        if let Ok(group) = syn::parse_str::<syn::Macro>(&format!("__tokens!{token}")) {
            if token_paths(&group, names, import) {
                return true;
            }
        }
    }
    false
}

pub(crate) fn data_macro(mac: &syn::Macro) -> bool {
    mac.path.get_ident().is_some_and(|name| {
        matches!(
            name.to_string().as_str(),
            "stringify" | "concat" | "include_str" | "include_bytes" | "env" | "option_env" | "cfg"
        )
    })
}
