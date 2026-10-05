//! Rust module ownership uses parsed paths and imports, including raw identifiers.

use syn::visit::{self, Visit};

struct OwnerVisitor<'a> {
    owner: &'a str,
    problem: Option<String>,
}

impl OwnerVisitor<'_> {
    fn reject(&mut self, problem: impl Into<String>) {
        if self.problem.is_none() {
            self.problem = Some(problem.into());
        }
    }

    fn identifier(&mut self, ident: &syn::Ident) {
        let text = ident.to_string();
        let name = text.strip_prefix("r#").unwrap_or(&text);
        if super::DOMAINS.contains(&name) && name != self.owner {
            self.reject(format!("sibling_domain:{name}"));
        }
        if [
            "velnor_actions_mise",
            "velnor_actions_workflow_renderer",
            "velnor_actions_rust",
            "velnor_actions_tofu",
            "velnor_actions_orchestrator",
            "process",
        ]
        .contains(&name)
        {
            self.reject(format!("foreign_owner_or_execution:{name}"));
        }
    }

    fn macro_expression(&mut self, source: &str) {
        match syn::parse_str::<syn::Expr>(source) {
            Ok(expression) => self.visit_expr(&expression),
            Err(error) => self.reject(format!("unsupported_macro_expression:{error}")),
        }
    }
}

impl<'ast> Visit<'ast> for OwnerVisitor<'_> {
    fn visit_expr(&mut self, value: &'ast syn::Expr) {
        if matches!(value, syn::Expr::Verbatim(_)) {
            self.reject("opaque_owner_expression");
        } else {
            visit::visit_expr(self, value);
        }
    }

    fn visit_item(&mut self, value: &'ast syn::Item) {
        if matches!(value, syn::Item::Verbatim(_)) {
            self.reject("opaque_owner_item");
        } else {
            visit::visit_item(self, value);
        }
    }

    fn visit_foreign_item(&mut self, value: &'ast syn::ForeignItem) {
        if matches!(value, syn::ForeignItem::Verbatim(_)) {
            self.reject("opaque_foreign_item");
        } else {
            visit::visit_foreign_item(self, value);
        }
    }

    fn visit_impl_item(&mut self, value: &'ast syn::ImplItem) {
        if matches!(value, syn::ImplItem::Verbatim(_)) {
            self.reject("opaque_impl_item");
        } else {
            visit::visit_impl_item(self, value);
        }
    }

    fn visit_trait_item(&mut self, value: &'ast syn::TraitItem) {
        if matches!(value, syn::TraitItem::Verbatim(_)) {
            self.reject("opaque_trait_item");
        } else {
            visit::visit_trait_item(self, value);
        }
    }

    fn visit_pat(&mut self, value: &'ast syn::Pat) {
        if matches!(value, syn::Pat::Verbatim(_)) {
            self.reject("opaque_owner_pattern");
        } else {
            visit::visit_pat(self, value);
        }
    }

    fn visit_type(&mut self, value: &'ast syn::Type) {
        if matches!(value, syn::Type::Verbatim(_)) {
            self.reject("opaque_owner_type");
        } else {
            visit::visit_type(self, value);
        }
    }

    fn visit_type_param_bound(&mut self, value: &'ast syn::TypeParamBound) {
        if matches!(value, syn::TypeParamBound::Verbatim(_)) {
            self.reject("opaque_type_bound");
        } else {
            visit::visit_type_param_bound(self, value);
        }
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        for segment in &path.segments {
            self.identifier(&segment.ident);
        }
        visit::visit_path(self, path);
    }

    fn visit_use_tree(&mut self, tree: &'ast syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => self.identifier(&path.ident),
            syn::UseTree::Name(name) => self.identifier(&name.ident),
            syn::UseTree::Rename(rename) => self.identifier(&rename.ident),
            syn::UseTree::Glob(_) => self.reject("wildcard_owner_import"),
            syn::UseTree::Group(_) => {}
        }
        visit::visit_use_tree(self, tree);
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if module.attrs.iter().any(test_only) {
            return;
        }
        self.identifier(&module.ident);
        visit::visit_item_mod(self, module);
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if let Err(problem) = attribute_policy(attribute) {
            self.reject(problem);
            return;
        }
        if attribute.path().is_ident("cfg_attr") {
            self.reject("conditional_owner_attribute");
            return;
        }
        if attribute.path().is_ident("path") {
            match &attribute.meta {
                syn::Meta::NameValue(value) => match &value.value {
                    syn::Expr::Lit(literal) => match &literal.lit {
                        syn::Lit::Str(path) => {
                            if !owned_include(&path.value()) {
                                self.reject("cross_owner_module_path");
                            }
                        }
                        _ => self.reject("nonliteral_module_path"),
                    },
                    _ => self.reject("computed_module_path"),
                },
                _ => self.reject("malformed_module_path"),
            }
        }
        // Approved attributes contain only documentation, compiler conditions,
        // fixed derives or the literal path checked above, never opaque code.
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.visit_path(&invocation.path);
        let Some(name) = invocation
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            self.reject("empty_macro_path");
            return;
        };
        let tokens = invocation.tokens.to_string();
        match name.as_str() {
            "include_str" | "include_bytes" => {
                match syn::parse2::<syn::LitStr>(invocation.tokens.clone()) {
                    Ok(path) if owned_include(&path.value()) => {}
                    _ => self.reject("unowned_or_computed_source_include"),
                }
            }
            "vec" => self.macro_expression(&format!("[{tokens}]")),
            "format" | "format_args" | "write" | "writeln" => {
                self.macro_expression(&format!("({tokens})"));
            }
            "matches" => match matches_parts(invocation) {
                Ok((value, pattern, guard)) => {
                    self.visit_expr(&value);
                    self.visit_pat(&pattern);
                    if let Some(guard) = guard {
                        self.visit_expr(&guard);
                    }
                }
                Err(error) => self.reject(format!("unsupported_matches_macro:{error}")),
            },
            _ => self.reject(format!("unqualified_owner_macro:{name}")),
        }
    }
}

fn attribute_policy(attribute: &syn::Attribute) -> Result<(), String> {
    if attribute.path().is_ident("derive") {
        let parser = syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated;
        let paths = attribute
            .parse_args_with(parser)
            .map_err(|error| error.to_string())?;
        for path in paths {
            let names: Vec<_> = path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            let standard = names.len() == 1
                && [
                    "Debug",
                    "Clone",
                    "Copy",
                    "PartialEq",
                    "Eq",
                    "PartialOrd",
                    "Ord",
                    "Default",
                    "Hash",
                ]
                .contains(&names[0].as_str());
            let serde = (names.len() == 1 || (names.len() == 2 && names[0] == "serde"))
                && names
                    .last()
                    .is_some_and(|name| ["Serialize", "Deserialize"].contains(&name.as_str()));
            if !(standard || serde)
                || path
                    .segments
                    .iter()
                    .any(|segment| !matches!(&segment.arguments, syn::PathArguments::None))
            {
                return Err("unqualified_owner_derive".to_owned());
            }
        }
        return Ok(());
    }
    if attribute.path().is_ident("cfg") && matches!(&attribute.meta, syn::Meta::List(_)) {
        return Ok(());
    }
    if attribute.path().is_ident("path") {
        return Ok(());
    }
    if attribute.path().is_ident("doc") || attribute.path().is_ident("must_use") {
        let accepted = match &attribute.meta {
            syn::Meta::Path(_) => attribute.path().is_ident("must_use"),
            syn::Meta::NameValue(value) => matches!(&value.value,
                syn::Expr::Lit(value) if matches!(&value.lit, syn::Lit::Str(_))),
            syn::Meta::List(list) => {
                attribute.path().is_ident("doc") && list.tokens.to_string() == "hidden"
            }
        };
        return if accepted {
            Ok(())
        } else {
            Err("computed_owner_attribute".to_owned())
        };
    }
    Err("unqualified_owner_attribute".to_owned())
}

fn test_only(attribute: &syn::Attribute) -> bool {
    match &attribute.meta {
        syn::Meta::List(list) => list.path.is_ident("cfg") && list.tokens.to_string() == "test",
        _ => false,
    }
}

fn owned_include(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && !path.split('/').any(|part| matches!(part, ".." | "." | ""))
}

type MatchesParts = (syn::Expr, syn::Pat, Option<syn::Expr>);

fn matches_parts(invocation: &syn::Macro) -> Result<MatchesParts, syn::Error> {
    invocation.parse_body_with(|input: syn::parse::ParseStream<'_>| {
        let value = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let pattern = syn::Pat::parse_multi_with_leading_vert(input)?;
        let guard = if input.peek(syn::Token![if]) {
            input.parse::<syn::Token![if]>()?;
            Some(input.parse()?)
        } else {
            None
        };
        if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
        }
        Ok((value, pattern, guard))
    })
}

pub(super) fn problem(owner: &str, source: &str) -> Option<String> {
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(error) => return Some(format!("invalid_owner_rust_source:{error}")),
    };
    let mut visitor = OwnerVisitor {
        owner,
        problem: None,
    };
    visitor.visit_file(&file);
    visitor.problem
}

#[cfg(test)]
mod tests {
    #[test]
    fn raw_commented_nested_and_reexported_aliases_are_rejected() {
        for source in [
            "use crate::r#ruby as r; fn call(){r::syntax(paths);}",
            "use crate::ruby /* owner */ as r;",
            "use crate /* owner */ :: ruby as r;",
            "pub use crate::{r#ruby as r};",
            "use crate::*;",
            "fn call(){crate::r#ruby /* owner */ ::syntax(paths);}",
            "fn call(){let _=vec![crate::r#ruby::syntax(paths)];}",
        ] {
            assert!(super::problem("shell", source).is_some(), "{source}");
        }
    }

    #[test]
    fn macros_and_module_includes_cannot_hide_foreign_owners() {
        for source in [
            "macro_rules! escape {()=>{crate::ruby::syntax(paths)}}",
            "include!(\"policy.rs\");",
            "const BODY:&str=include_str!(concat!(\"../\",\"ruby/code.rs\"));",
            "#[path=\"../ruby/mod.rs\"]mod local;",
            "#[cfg_attr(all(),path=\"../ruby/mod.rs\")]mod local;",
            "#[owner_escape(crate::ruby)]fn call(){}",
            "#[derive(crate::ruby::Debug)]struct Value;",
            "#[doc=include_str!(\"../ruby/body.rs\")]fn call(){}",
            "#[must_use=crate::ruby::LABEL]fn call(){}",
            "fn call(){become crate::ruby::syntax(paths)}",
            "fn call(){foreign_macro!(crate::ruby::syntax(paths));}",
        ] {
            assert!(super::problem("shell", source).is_some(), "{source}");
        }
    }

    #[test]
    fn own_includes_standard_macros_and_contract_paths_are_accepted() {
        let source = "use velnor_actions_contract::ContractError; const BODY:&str=include_str!(\"body.sh\"); fn value(){let x=vec![1,2];let _=matches!(x.as_slice(),[]|[_] if x.len()<2); let _=format!(\"{}\",x.len());}";
        assert_eq!(super::problem("shell", source), None);
    }
}
