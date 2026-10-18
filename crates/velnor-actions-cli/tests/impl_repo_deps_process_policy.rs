use std::collections::BTreeSet;

use syn::visit::Visit;

pub(crate) fn direct_tool_policy_violation(source: &str) -> Option<String> {
    let Ok(syntax) = syn::parse_file(source) else {
        return Some("Rust syntax parse failed".to_owned());
    };
    let imports = ProcessImports::from_file(&syntax);
    if imports.has_process_glob {
        return Some("std::process::*".to_owned());
    }
    let mut visitor = CapabilityVisitor::new(imports);
    visitor.visit_file(&syntax);
    if visitor.violation.is_some() {
        return visitor.violation;
    }
    if let Some(violation) = process_import_violation(&visitor.imports.trees, &visitor.imports) {
        return Some(violation);
    }
    for token in ["process::Command", "Command::new"] {
        if source.contains(token) {
            return Some(token.to_owned());
        }
    }
    for (index, _) in source.match_indices("std::process::") {
        let rest = &source[index + "std::process::".len()..];
        if !rest.starts_with("ExitCode") && !rest.starts_with("id()") {
            return Some(rest.to_owned());
        }
    }
    for token in ["\"gh\"", "shell", "Shell", "sh -c"] {
        if source.contains(token) {
            return Some(token.to_owned());
        }
    }
    None
}

#[derive(Default)]
struct ProcessImports {
    trees: Vec<syn::UseTree>,
    command_aliases: BTreeSet<String>,
    process_aliases: BTreeSet<String>,
    std_aliases: BTreeSet<String>,
    has_process_glob: bool,
}

impl ProcessImports {
    fn from_file(file: &syn::File) -> Self {
        let mut result = Self::default();
        let mut collector = ImportCollector::default();
        collector.visit_file(file);
        result.trees = collector.trees;
        let trees = result.trees.clone();
        for _ in 0..=trees.len() {
            let old_count = result.std_aliases.len() + result.process_aliases.len();
            for tree in &trees {
                collect_module_aliases(tree, &mut Vec::new(), &mut result);
            }
            if result.std_aliases.len() + result.process_aliases.len() == old_count {
                break;
            }
        }
        for tree in &trees {
            collect_command_aliases(tree, &mut Vec::new(), &mut result);
        }
        result
    }
}

fn process_import_violation(trees: &[syn::UseTree], imports: &ProcessImports) -> Option<String> {
    trees
        .iter()
        .find_map(|tree| find_process_import_violation(tree, &mut Vec::new(), imports))
}

fn find_process_import_violation(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    imports: &ProcessImports,
) -> Option<String> {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            let violation = find_process_import_violation(&path.tree, prefix, imports);
            prefix.pop();
            violation
        }
        syn::UseTree::Name(name) => process_import_member(prefix, &name.ident.to_string(), imports),
        syn::UseTree::Rename(rename) => {
            let mut source = prefix.clone();
            source.push(rename.ident.to_string());
            if is_process_command_path(&source, imports)
                || imports.command_aliases.contains(&rename.ident.to_string())
            {
                return Some("process::Command".to_owned());
            }
            process_import_member(prefix, &rename.ident.to_string(), imports)
        }
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .find_map(|item| find_process_import_violation(item, prefix, imports)),
        syn::UseTree::Glob(_) => None,
    }
}

fn process_import_member(
    prefix: &[String],
    member: &str,
    imports: &ProcessImports,
) -> Option<String> {
    if !is_process_module_path(prefix, imports)
        || matches!(member, "self" | "id" | "ExitCode" | "Command")
    {
        return None;
    }
    Some(format!("process::{member}"))
}

#[derive(Default)]
struct ImportCollector {
    trees: Vec<syn::UseTree>,
}

impl<'ast> Visit<'ast> for ImportCollector {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.trees.push(item.tree.clone());
        syn::visit::visit_item_use(self, item);
    }
}

fn collect_module_aliases(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    result: &mut ProcessImports,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_module_aliases(&path.tree, prefix, result);
            prefix.pop();
        }
        syn::UseTree::Rename(rename) => {
            let mut source = prefix.clone();
            source.push(rename.ident.to_string());
            if is_std_module_path(&source, result) {
                result.std_aliases.insert(rename.rename.to_string());
            }
            if is_process_module_path(&source, result) {
                result.process_aliases.insert(rename.rename.to_string());
            }
        }
        syn::UseTree::Name(name) => {
            let identifier = name.ident.to_string();
            if identifier == "self" {
                if is_std_module_path(prefix, result) {
                    result.std_aliases.insert("std".to_owned());
                }
                if is_process_module_path(prefix, result) {
                    result.process_aliases.insert("process".to_owned());
                }
            } else {
                let mut source = prefix.clone();
                source.push(identifier.clone());
                if is_std_module_path(&source, result) {
                    result.std_aliases.insert(identifier.clone());
                }
                if is_process_module_path(&source, result) {
                    result.process_aliases.insert(identifier);
                }
            }
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_module_aliases(item, prefix, result);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

fn collect_command_aliases(
    tree: &syn::UseTree,
    prefix: &mut Vec<String>,
    imports: &mut ProcessImports,
) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_command_aliases(&path.tree, prefix, imports);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            if is_process_command_path(prefix, imports) {
                imports.command_aliases.insert(name.ident.to_string());
            }
            prefix.pop();
        }
        syn::UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            if is_process_command_path(prefix, imports) {
                imports.command_aliases.insert(rename.rename.to_string());
            }
            prefix.pop();
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                collect_command_aliases(item, prefix, imports);
            }
        }
        syn::UseTree::Glob(_) => {
            if is_process_module_path(prefix, imports) {
                imports.has_process_glob = true;
            }
        }
    }
}

fn is_process_command_path(path: &[String], imports: &ProcessImports) -> bool {
    path.len() == 3
        && (path[0] == "std" || imports.std_aliases.contains(&path[0]))
        && path[1] == "process"
        && path[2] == "Command"
        || path.len() == 2 && imports.process_aliases.contains(&path[0]) && path[1] == "Command"
}

fn is_process_module_path(path: &[String], imports: &ProcessImports) -> bool {
    path.len() == 2
        && (path[0] == "std" || imports.std_aliases.contains(&path[0]))
        && path[1] == "process"
        || path.len() == 1 && imports.process_aliases.contains(&path[0])
}

fn is_std_module_path(path: &[String], imports: &ProcessImports) -> bool {
    path.len() == 1 && (path[0] == "std" || imports.std_aliases.contains(&path[0]))
}

struct CapabilityVisitor {
    imports: ProcessImports,
    violation: Option<String>,
}

impl CapabilityVisitor {
    fn new(imports: ProcessImports) -> Self {
        Self {
            imports,
            violation: None,
        }
    }

    fn check_path(&mut self, path: &syn::Path) {
        if self.violation.is_some() {
            return;
        }
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        if path_is_process_command(&segments, &self.imports) {
            self.violation = Some("process::Command".to_owned());
            return;
        }
        if let Some(alias) = segments.first()
            && self.imports.command_aliases.contains(alias)
        {
            self.violation = Some(if segments.get(1).is_some_and(|segment| segment == "new") {
                format!("{alias}::new")
            } else {
                "process::Command".to_owned()
            });
            return;
        }
        if let Some(module_index) = process_module_index(&segments, &self.imports) {
            let member_index = module_index + 1;
            if let Some(member) = segments.get(member_index)
                && member != "ExitCode"
                && member != "id"
            {
                self.violation = Some(format!("process::{member}"));
            }
        }
    }
}

impl<'ast> Visit<'ast> for CapabilityVisitor {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.check_path(path);
        syn::visit::visit_path(self, path);
    }
}

fn path_is_process_command(path: &[String], imports: &ProcessImports) -> bool {
    if path.len() >= 3
        && (path[0] == "std" || imports.std_aliases.contains(&path[0]))
        && path[1] == "process"
        && path[2] == "Command"
    {
        return true;
    }
    path.len() >= 2 && imports.process_aliases.contains(&path[0]) && path[1] == "Command"
}

fn process_module_index(path: &[String], imports: &ProcessImports) -> Option<usize> {
    if path.len() >= 2
        && (path[0] == "std" || imports.std_aliases.contains(&path[0]))
        && path[1] == "process"
    {
        return Some(1);
    }
    imports.process_aliases.contains(&path[0]).then_some(0)
}

#[cfg(test)]
#[path = "impl_repo_deps_process_policy_tests.rs"]
mod tests;
