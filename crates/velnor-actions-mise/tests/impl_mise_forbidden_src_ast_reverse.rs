//! Reverse declaration inventory grants no body or filesystem authority.
use super::{paths, test_gate};
use std::collections::HashSet;
use syn::Item;

pub(crate) fn check_test_reverse_edges(
    authoritative: &[(String, String)],
    reverse: &[(String, String)],
    violations: &mut Vec<String>,
) {
    let admitted: HashSet<String> = authoritative
        .iter()
        .filter(|(p, _)| !p.starts_with(paths::SOURCE))
        .map(|(p, _)| p.clone())
        .collect();
    let mut inventory: HashSet<String> = authoritative.iter().map(|(p, _)| p.clone()).collect();
    let mut seen = HashSet::new();
    for (path, _) in reverse {
        if !paths::ordinary(path) || path.starts_with(paths::SOURCE) || !seen.insert(path.clone()) {
            violations.push(format!("{path}: invalid reverse declaration inventory"));
        }
        inventory.insert(path.clone());
    }
    for (path, source) in reverse {
        match syn::parse_file(source) {
            Ok(file) => {
                let directory = paths::module_directory(path);
                let parent = path.rsplit_once('/').map_or("", |(p, _)| p);
                declarations(
                    path,
                    &file.items,
                    &directory,
                    parent,
                    &inventory,
                    &admitted,
                    violations,
                );
            }
            Err(error) => violations.push(format!("{path}: reverse module AST failed: {error}")),
        }
    }
}

fn declarations(
    path: &str,
    items: &[Item],
    directory: &str,
    attribute_directory: &str,
    inventory: &HashSet<String>,
    admitted: &HashSet<String>,
    violations: &mut Vec<String>,
) {
    for item in items {
        let Item::Mod(module) = item else {
            if has_local_module(item) {
                violations.push(format!(
                    "{path}: unsupported reverse block-local module declaration"
                ));
            }
            continue;
        };
        if module.attrs.iter().any(|a| a.path().is_ident("cfg_attr")) {
            violations.push(format!("{path}: opaque reverse module cfg attribute"));
        }
        if let Some((_, body)) = &module.content {
            let nested = format!("{directory}/{}", module.ident);
            declarations(
                path, body, &nested, &nested, inventory, admitted, violations,
            );
            continue;
        }
        match paths::target(module, directory, attribute_directory, inventory) {
            Some(target) if admitted.contains(&target) && !test_gate(module) => {
                violations.push(format!(
                    "{path}: incoming admitted fixture edge lacks exact cfg(test)"
                ));
            }
            Some(_) => {}
            None => violations.push(format!(
                "{path}: opaque, escaping, or missing reverse module target {}",
                module.ident
            )),
        }
    }
}

#[test]
fn reverse_incoming_edges_require_exact_cfg_without_body_waivers() {
    let authoritative = vec![(
        "crates/velnor-actions-mise/tests/fixture.rs".to_owned(),
        String::new(),
    )];
    for edge in [
        "#[path=\"fixture.rs\"] mod fixture;",
        "#[cfg(any(test,feature=\"extra\"))] #[path=\"fixture.rs\"] mod fixture;",
        "#[cfg_attr(test,path=\"fixture.rs\")] mod fixture;",
        "#[cfg(test)] #[path=env!(\"WRITER\")] mod fixture;",
        "#[cfg(test)] #[path=\"../outside.rs\"] mod fixture;",
        "fn hidden(){ #[cfg(test)] #[path=\"fixture.rs\"] mod fixture; }",
    ] {
        let reverse = vec![(
            "crates/velnor-actions-mise/tests/entry.rs".to_owned(),
            edge.to_owned(),
        )];
        let mut found = Vec::new();
        check_test_reverse_edges(&authoritative, &reverse, &mut found);
        assert!(!found.is_empty(), "{edge}");
    }
    let reverse=vec![("crates/velnor-actions-mise/tests/entry.rs".to_owned(),
        "#[cfg(test)] #[path=\"fixture.rs\"] mod fixture; opaque_body_macro!(); fn writer(){std::fs::write(p,b\"x\");}".to_owned())];
    let mut found = Vec::new();
    check_test_reverse_edges(&authoritative, &reverse, &mut found);
    assert!(found.is_empty(), "{found:?}");
}

fn has_local_module(item: &Item) -> bool {
    struct Local(bool);
    impl<'ast> syn::visit::Visit<'ast> for Local {
        fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {
            self.0 = true;
        }
    }
    let mut found = Local(false);
    syn::visit::Visit::visit_item(&mut found, item);
    found.0
}
