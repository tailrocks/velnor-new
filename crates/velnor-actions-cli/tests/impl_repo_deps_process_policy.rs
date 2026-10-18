use syn::ext::IdentExt;
use syn::visit::Visit;

/// This check reads Rust syntax. It does not read macro output.
pub(crate) fn direct_tool_policy_violation(source: &str) -> Option<String> {
    let Ok(syntax) = syn::parse_file(source) else {
        return Some("Rust syntax parse failed".to_owned());
    };
    let mut visitor = ProcessPolicyVisitor::default();
    visitor.visit_file(&syntax);
    if visitor.violation.is_some() {
        return visitor.violation;
    }
    for token in ["\"gh\"", "shell", "Shell", "sh -c"] {
        if source.contains(token) {
            return Some(token.to_owned());
        }
    }
    None
}

#[derive(Default)]
struct ProcessPolicyVisitor {
    violation: Option<String>,
    command_aliases: Vec<String>,
}

impl ProcessPolicyVisitor {
    fn reject(&mut self, value: impl Into<String>) {
        if self.violation.is_none() {
            self.violation = Some(value.into());
        }
    }

    fn check_import(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        if self.violation.is_some() {
            return;
        }
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(ident_name(&path.ident));
                self.check_import(&path.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Name(name) => {
                self.check_import_leaf(prefix, &ident_name(&name.ident), false);
            }
            syn::UseTree::Rename(rename) => {
                let source_name = ident_name(&rename.ident);
                let target_name = ident_name(&rename.rename);
                if matches!(target_name.as_str(), "std" | "process") {
                    self.reject(format!("{target_name} namespace binding"));
                    return;
                }
                if prefix.is_empty() && source_name == "std" {
                    self.reject("std namespace alias");
                    return;
                }
                if prefix.len() == 1 && prefix[0] == "std" && source_name == "self" {
                    self.reject("std namespace alias");
                    return;
                }
                if is_process_module(prefix) && source_name == "Command" {
                    self.command_aliases.push(target_name);
                }
                self.check_import_leaf(prefix, &source_name, true);
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.check_import(item, prefix);
                    if self.violation.is_some() {
                        return;
                    }
                }
            }
            syn::UseTree::Glob(_) => {
                if is_process_module(prefix) {
                    self.reject("std::process::*");
                } else if prefix.len() == 1 && prefix[0] == "std" {
                    self.reject("std namespace glob");
                }
            }
        }
    }

    fn check_import_leaf(&mut self, prefix: &[String], name: &str, renamed: bool) {
        if prefix.is_empty() && name == "std" {
            self.reject("std namespace alias");
            return;
        }
        if prefix.len() == 1 && prefix[0] == "std" && name == "self" {
            self.reject("std namespace alias");
            return;
        }
        if prefix.len() == 1 && prefix[0] == "std" && name == "process" {
            self.reject("process::Command");
            return;
        }
        if is_process_module(prefix) && (renamed || !matches!(name, "id" | "ExitCode")) {
            self.reject("process::Command");
        }
    }

    fn check_path(&mut self, path: &syn::Path) {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| ident_name(&segment.ident))
            .collect();
        if segments.len() >= 2
            && segments[1] == "new"
            && self
                .command_aliases
                .iter()
                .any(|alias| alias == &segments[0])
        {
            self.violation = Some(format!("{}::new", segments[0]));
            return;
        }
        if self.violation.is_some() {
            return;
        }
        if segments.first().is_some_and(|first| first == "process") {
            self.reject("process::Command");
            return;
        }
        if segments.len() >= 2 && segments[0] == "std" && segments[1] == "process" {
            match segments.get(2).map(String::as_str) {
                Some("id") if segments.len() == 3 => {}
                Some("ExitCode") => {}
                Some(_) | None => self.reject("process::Command"),
            }
        }
    }
}

impl<'ast> Visit<'ast> for ProcessPolicyVisitor {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.check_import(&item.tree, &mut Vec::new());
        syn::visit::visit_item_use(self, item);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        if ident_name(&item.ident) == "std"
            || item
                .rename
                .as_ref()
                .is_some_and(|(_, rename)| matches!(ident_name(rename).as_str(), "std" | "process"))
        {
            self.reject("std namespace binding");
        }
        syn::visit::visit_item_extern_crate(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if matches!(ident_name(&item.ident).as_str(), "std" | "process") {
            self.reject("std namespace binding");
        }
        syn::visit::visit_item_mod(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.check_path(path);
        syn::visit::visit_path(self, path);
    }
}

fn ident_name(ident: &syn::Ident) -> String {
    ident.unraw().to_string()
}

fn is_process_module(path: &[String]) -> bool {
    path.len() == 2 && path[0] == "std" && path[1] == "process"
}

#[cfg(test)]
#[path = "impl_repo_deps_process_policy_tests.rs"]
mod tests;
