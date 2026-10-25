use std::collections::HashMap;

use syn::visit::{self, Visit};
use syn::{Type, UseTree};

#[derive(Default)]
struct ImportAudit<'a> {
    name: &'a str,
    aliases: HashMap<String, Vec<String>>,
    violations: Vec<String>,
}

pub(super) struct CollectedImports {
    pub(super) aliases: HashMap<String, Vec<String>>,
    pub(super) violations: Vec<String>,
}

pub(super) fn collect(name: &str, file: &syn::File) -> CollectedImports {
    let mut audit = ImportAudit {
        name,
        ..ImportAudit::default()
    };
    audit.visit_file(file);
    CollectedImports {
        aliases: audit.aliases,
        violations: audit.violations,
    }
}

impl<'ast> Visit<'ast> for ImportAudit<'_> {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        collect_use_tree(
            self.name,
            &item.tree,
            &[],
            &mut self.aliases,
            &mut self.violations,
        );
        visit::visit_item_use(self, item);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        let crate_name = item.ident.to_string();
        let alias = item
            .rename
            .as_ref()
            .map_or_else(|| crate_name.clone(), |rename| rename.1.to_string());
        if super::is_external_file_api_root(std::slice::from_ref(&crate_name)) {
            self.violations.push(format!(
                "external crate import exposes path-opening APIs: `{crate_name}`"
            ));
        }
        record_alias(
            alias,
            vec![crate_name],
            &mut self.aliases,
            &mut self.violations,
        );
        visit::visit_item_extern_crate(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        if let Type::Path(path) = &*item.ty {
            let canonical = super::resolve_path(&path.path, &self.aliases);
            if super::is_external_file_api_root(&canonical) {
                self.violations.push(format!(
                    "type alias exposes a path-opening API: `{}`",
                    item.ident
                ));
            }
            record_alias(
                item.ident.to_string(),
                canonical,
                &mut self.aliases,
                &mut self.violations,
            );
        }
        visit::visit_item_type(self, item);
    }
}

fn collect_use_tree(
    name: &str,
    tree: &UseTree,
    prefix: &[String],
    aliases: &mut HashMap<String, Vec<String>>,
    violations: &mut Vec<String>,
) {
    match tree {
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(path.ident.to_string());
            collect_use_tree(name, &path.tree, &next, aliases, violations);
        }
        UseTree::Name(item_name) => {
            let mut imported = prefix.to_vec();
            let local_name = item_name.ident.to_string();
            if local_name != "self" {
                imported.push(local_name.clone());
            }
            record_import(name, imported, local_name, aliases, violations);
        }
        UseTree::Rename(rename) => {
            let mut imported = prefix.to_vec();
            if rename.ident != "self" {
                imported.push(rename.ident.to_string());
            }
            record_import(
                name,
                imported,
                rename.rename.to_string(),
                aliases,
                violations,
            );
        }
        UseTree::Group(group) => {
            for item in &group.items {
                collect_use_tree(name, item, prefix, aliases, violations);
            }
        }
        UseTree::Glob(_) => {
            let canonical = super::resolve_names(prefix.to_vec(), aliases);
            if super::is_external_file_api_root(&canonical) || super::is_std_fs_path(&canonical) {
                violations.push(format!(
                    "glob import leaves file-opening authority unknown: `{}`",
                    canonical.join("::")
                ));
            }
        }
    }
}

fn record_import(
    name: &str,
    imported: Vec<String>,
    local_name: String,
    aliases: &mut HashMap<String, Vec<String>>,
    violations: &mut Vec<String>,
) {
    let canonical = super::resolve_names(imported, aliases);
    if super::is_external_file_api_root(&canonical)
        && !is_reviewed_read_only_rustix_import(name, &canonical)
    {
        violations.push(format!(
            "external crate import exposes path-opening APIs: `{}`",
            canonical.join("::")
        ));
    }
    if !super::executable::is_test_fixture_mutation(name, &canonical)
        && super::is_dangerous_file_api(&canonical)
    {
        violations.push(format!(
            "filesystem mutation import is forbidden: `{}`",
            canonical.join("::")
        ));
    }
    record_alias(local_name, canonical, aliases, violations);
}

fn record_alias(
    local_name: String,
    canonical: Vec<String>,
    aliases: &mut HashMap<String, Vec<String>>,
    violations: &mut Vec<String>,
) {
    match aliases.get(&local_name) {
        Some(previous)
            if previous != &canonical && previous.first().is_some_and(|s| s != "<ambiguous>") =>
        {
            violations.push(format!(
                "scoped alias `{local_name}` has conflicting targets: `{}` and `{}`",
                previous.join("::"),
                canonical.join("::")
            ));
            aliases.insert(local_name, vec!["<ambiguous>".to_owned()]);
        }
        Some(previous) if previous.first().is_some_and(|s| s == "<ambiguous>") => {}
        Some(_) => {}
        None => {
            aliases.insert(local_name, canonical);
        }
    }
}

fn is_reviewed_read_only_rustix_import(name: &str, path: &[String]) -> bool {
    if path.len() != 3 || path[0] != "rustix" {
        return false;
    }
    match path[1].as_str() {
        "fs" => {
            matches!(path[2].as_str(), "OFlags" | "fcntl_getfl" | "fcntl_setfl")
                || (name == "requests_executable_lookup.rs"
                    && matches!(path[2].as_str(), "FileType" | "Mode" | "openat"))
        }
        "process" => matches!(
            path[2].as_str(),
            "Pid"
                | "Signal"
                | "WaitId"
                | "WaitIdOptions"
                | "WaitOptions"
                | "kill_process_group"
                | "waitid"
                | "waitpid"
        ),
        "io" => path[2] == "Errno",
        _ => false,
    }
}
