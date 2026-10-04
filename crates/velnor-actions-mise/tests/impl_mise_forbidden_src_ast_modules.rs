//! Test authority exists only through a complete, closed module graph.
use std::collections::{HashMap, HashSet};
use syn::{Item, ItemMod};
#[path = "impl_mise_forbidden_src_ast_module_paths.rs"]
mod paths;
#[path = "impl_mise_forbidden_src_ast_reverse.rs"]
mod reverse;
pub(crate) use reverse::check_test_reverse_edges;
#[path = "impl_mise_forbidden_src_ast_module_tests.rs"]
mod tests;

#[derive(Clone)]
struct Edge {
    target: String,
    test: bool,
    private: bool,
}
struct Unit {
    path: String,
    file: syn::File,
    edges: Vec<Edge>,
}

pub(crate) fn test_module(item: &ItemMod) -> bool {
    matches!(item.vis, syn::Visibility::Inherited) && test_gate(item)
}

fn test_gate(item: &ItemMod) -> bool {
    item.attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .count()
        == 1
        && item.attrs.iter().any(|a| {
            a.path().is_ident("cfg")
                && a.parse_args::<syn::Path>()
                    .is_ok_and(|p| p.is_ident("test"))
        })
        && !item.attrs.iter().any(|a| a.path().is_ident("cfg_attr"))
}

pub(crate) fn check_source_graph(inputs: &[(String, String)], violations: &mut Vec<String>) {
    let start = violations.len();
    let mut units = Vec::new();
    let mut inventory = HashSet::new();
    for (path, source) in inputs {
        if !paths::ordinary(path) || !inventory.insert(path.clone()) {
            violations.push(format!("{path}: invalid or duplicate module inventory"));
            continue;
        }
        match syn::parse_file(source) {
            Ok(file) => units.push(Unit {
                path: path.clone(),
                file,
                edges: Vec::new(),
            }),
            Err(error) => violations.push(format!("{path}: module AST parse failed: {error}")),
        }
    }
    if !inventory.contains("crates/velnor-actions-mise/src/lib.rs") {
        violations.push("missing product crate root in module inventory".to_owned());
    }
    for unit in &mut units {
        let directory = paths::module_directory(&unit.path);
        let parent = unit.path.rsplit_once('/').map_or("", |(p, _)| p);
        collect(
            &unit.path,
            &unit.file.items,
            &directory,
            parent,
            false,
            &inventory,
            &mut unit.edges,
            violations,
        );
    }
    let roles = roles(&units, violations);
    for unit in &units {
        let test = roles.get(&unit.path).copied().unwrap_or(false);
        validate_items(&unit.path, &unit.file.items, test, &units, violations);
    }
    if violations.len() != start {
        return;
    }
    for (path, source) in inputs {
        if !roles.get(path).copied().unwrap_or(false) {
            if let Some(name) = path.strip_prefix(paths::SOURCE) {
                super::scan(name, source, violations, true);
            }
        }
    }
}

fn collect(
    path: &str,
    items: &[Item],
    directory: &str,
    attribute_directory: &str,
    inherited_test: bool,
    inventory: &HashSet<String>,
    edges: &mut Vec<Edge>,
    violations: &mut Vec<String>,
) {
    for item in items {
        let Item::Mod(module) = item else {
            continue;
        };
        let test = test_gate(module);
        if paths::mentions_test(module) && !test {
            violations.push(format!("{path}: ambiguous or public test-module predicate"));
        }
        if inherited_test && !test {
            violations.push(format!(
                "{path}: every nested test-module edge requires exact cfg(test)"
            ));
        }
        let child_test = inherited_test || test;
        if let Some((_, body)) = &module.content {
            let nested = format!("{directory}/{}", module.ident);
            collect(
                path, body, &nested, &nested, child_test, inventory, edges, violations,
            );
            continue;
        }
        match paths::target(module, directory, attribute_directory, inventory) {
            Some(target) => {
                let ordinary_test = !target.starts_with(paths::SOURCE);
                if (ordinary_test || inherited_test) && !test {
                    violations.push(format!("{path}: ungated ordinary test-module edge"));
                }
                if child_test && !matches!(module.vis, syn::Visibility::Inherited) {
                    // Test nodes may expose internal fixture helpers, but source entry edges stay private.
                    if path.starts_with(paths::SOURCE) && !inherited_test {
                        violations.push(format!("{path}: public test entry module"));
                    }
                }
                edges.push(Edge {
                    target,
                    test,
                    private: matches!(module.vis, syn::Visibility::Inherited),
                });
            }
            None => violations.push(format!(
                "{path}: unclosed, escaping, or missing module target {}",
                module.ident
            )),
        }
    }
}

fn roles(units: &[Unit], violations: &mut Vec<String>) -> HashMap<String, bool> {
    let mut incoming: HashMap<String, Vec<&Edge>> = HashMap::new();
    for unit in units {
        for edge in &unit.edges {
            incoming.entry(edge.target.clone()).or_default().push(edge);
        }
    }
    let roots: HashSet<String> = units
        .iter()
        .filter(|unit| {
            unit.path.starts_with(paths::SOURCE)
                && (unit.path.ends_with("/lib.rs")
                    || unit.path.ends_with("/main.rs")
                    || !incoming.contains_key(&unit.path))
        })
        .map(|unit| unit.path.clone())
        .collect();
    let all_reachable = reachable(units, roots.clone(), false);
    let mut product_roots = roots;
    product_roots.extend(
        units
            .iter()
            .filter(|unit| {
                unit.path.starts_with(paths::SOURCE) && !all_reachable.contains(&unit.path)
            })
            .map(|unit| unit.path.clone()),
    );
    let product = reachable(units, product_roots, true);
    let roles: HashMap<_, _> = units
        .iter()
        .map(|unit| {
            let edges = incoming.get(&unit.path);
            let test = !unit.path.starts_with(paths::SOURCE) || !product.contains(&unit.path);
            if edges.is_some_and(|edges| {
                edges.iter().any(|edge| edge.test) && edges.iter().any(|edge| !edge.test)
            }) || (test && edges.is_some_and(|edges| edges.iter().any(|edge| !edge.test)))
            {
                violations.push(format!(
                    "{}: mixed product/test incoming module edges",
                    unit.path
                ));
            }
            (unit.path.clone(), test)
        })
        .collect();
    for unit in units {
        for edge in &unit.edges {
            if edge.test
                && (!roles.get(&edge.target).copied().unwrap_or(false)
                    || (edge.target.starts_with(paths::SOURCE) && !edge.private))
            {
                violations.push(format!(
                    "{}: test edge reaches product module {}",
                    unit.path, edge.target
                ));
            }
        }
    }
    roles
}

fn reachable(units: &[Unit], mut found: HashSet<String>, product_only: bool) -> HashSet<String> {
    loop {
        let before = found.len();
        for unit in units {
            if !found.contains(&unit.path) {
                continue;
            }
            for edge in &unit.edges {
                if !product_only || !edge.test {
                    found.insert(edge.target.clone());
                }
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

fn validate_items(
    path: &str,
    items: &[Item],
    test: bool,
    units: &[Unit],
    violations: &mut Vec<String>,
) {
    let names = test_names(units);
    let mut visitor = Boundary {
        path,
        test,
        names: &names,
        violations,
        blocks: 0,
    };
    for item in items {
        syn::visit::Visit::visit_item(&mut visitor, item);
    }
}

struct Boundary<'a> {
    path: &'a str,
    test: bool,
    names: &'a HashSet<String>,
    violations: &'a mut Vec<String>,
    blocks: usize,
}
impl<'ast> syn::visit::Visit<'ast> for Boundary<'_> {
    fn visit_ident(&mut self, ident: &'ast syn::Ident) {
        if ident.to_string().starts_with("r#") {
            self.violations.push(format!(
                "{}: unsupported raw authority identifier",
                self.path
            ));
        }
    }
    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.blocks += 1;
        syn::visit::visit_block(self, block);
        self.blocks -= 1;
    }
    fn visit_item_mod(&mut self, module: &'ast ItemMod) {
        if self.blocks != 0 {
            self.violations.push(format!(
                "{}: unsupported block-local module declaration",
                self.path
            ));
        }
        let old = self.test;
        if self.test && !test_gate(module) {
            self.violations.push(format!(
                "{}: nested test edge lacks exact cfg(test)",
                self.path
            ));
        }
        self.test |= test_gate(module);
        syn::visit::visit_item_mod(self, module);
        self.test = old;
    }
    fn visit_item_extern_crate(&mut self, _: &'ast syn::ItemExternCrate) {
        self.violations.push(format!(
            "{}: unsupported crate namespace remapping",
            self.path
        ));
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if self.test
            && (super::shapes::dangerous_import(&item.tree)
                || !super::shapes::authority_import(self.path, &item.tree, &[]))
        {
            self.violations
                .push(format!("{}: opaque test namespace import", self.path));
        }
        if !self.test && paths::uses_test(&item.tree, self.names) {
            self.violations.push(format!(
                "{}: product import/reexport of test namespace",
                self.path
            ));
        }
        syn::visit::visit_item_use(self, item);
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if !self.test
            && path.segments.len() > 1
            && path
                .segments
                .iter()
                .any(|s| self.names.contains(&s.ident.to_string()))
        {
            self.violations.push(format!(
                "{}: product reference to test namespace",
                self.path
            ));
        }
        syn::visit::visit_path(self, path);
    }
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if !self.test && !paths::data_macro(mac) && paths::macro_test_path(mac, self.names) {
            self.violations.push(format!(
                "{}: product macro reference to test namespace",
                self.path
            ));
        }
        if !self.test && !paths::data_macro(mac) {
            use syn::parse::Parser;
            let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
            if let Ok(expressions) = parser.parse2(mac.tokens.clone()) {
                for expression in &expressions {
                    syn::visit::Visit::visit_expr(self, expression);
                }
            }
        }
        if self.test
            && (!super::shapes::macro_allowed(mac)
                || mac.path.segments.last().is_some_and(|s| {
                    matches!(
                        s.ident.to_string().as_str(),
                        "include"
                            | "include_str"
                            | "include_bytes"
                            | "env"
                            | "option_env"
                            | "concat"
                    )
                }))
        {
            self.violations
                .push(format!("{}: opaque test-boundary expansion", self.path));
        }
        syn::visit::visit_macro(self, mac);
    }
    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        if self.test && attr.path().is_ident("cfg_attr") {
            self.violations
                .push(format!("{}: opaque test cfg attribute", self.path));
        }
        if !attr.path().is_ident("path") && !super::shapes::attribute_allowed(attr) {
            self.violations
                .push(format!("{}: opaque module graph attribute", self.path));
        }
        syn::visit::visit_attribute(self, attr);
    }
}

fn test_names(units: &[Unit]) -> HashSet<String> {
    struct Names(HashSet<String>);
    impl<'ast> syn::visit::Visit<'ast> for Names {
        fn visit_item_mod(&mut self, module: &'ast ItemMod) {
            if test_gate(module) {
                self.0.insert(module.ident.to_string());
            }
            syn::visit::visit_item_mod(self, module);
        }
    }
    let mut names = Names(HashSet::new());
    for unit in units {
        syn::visit::Visit::visit_file(&mut names, &unit.file);
    }
    names.0
}
