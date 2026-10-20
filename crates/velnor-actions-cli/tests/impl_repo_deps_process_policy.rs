//! CLI process policy: the CLI binary never reaches `std::process` beyond
//! `ExitCode`, whatever spelling, import, or namespace alias it uses.
//!
//! Text tokens catch tool names and calls written anywhere, macro bodies
//! included. The Rust syntax pass resolves every `use` tree and path, so
//! a renamed or regrouped import (`use std::{process as p}`) or a
//! namespace alias (`use std as s`) cannot hide `std::process::Command`.

use syn::ext::IdentExt;
use syn::visit::Visit;

/// Text that must not appear anywhere in CLI source.
const BANNED_TOKENS: &[&str] = &[
    "process::Command",
    "Command::new",
    ".spawn(",
    ".status()",
    ".output()",
    "cargo",
    // "mise" stays unbanned: repo-policy names a mise-version operation.
    "mbx",
    "nextest",
    "rustup",
    "\"gh\"",
    "shell",
    "Shell",
    "sh -c",
];

/// The only `std::process` item the CLI may name.
const ALLOWED_PROCESS_ITEM: &str = "ExitCode";

/// First violation in one CLI source file, or `None` when it complies.
pub(crate) fn direct_tool_policy_violation(source: &str) -> Option<String> {
    if let Some(token) = BANNED_TOKENS.iter().find(|token| source.contains(**token)) {
        return Some((*token).to_owned());
    }
    for (index, _) in source.match_indices("std::process::") {
        let rest = &source[index + "std::process::".len()..];
        if !rest.starts_with(ALLOWED_PROCESS_ITEM) {
            return Some(format!("std::process::{rest}"));
        }
    }
    let Ok(syntax) = syn::parse_file(source) else {
        return Some("Rust syntax parse failed".to_owned());
    };
    let mut visitor = ProcessPolicy::default();
    visitor.visit_file(&syntax);
    visitor.violation
}

#[derive(Default)]
struct ProcessPolicy {
    violation: Option<String>,
}

impl ProcessPolicy {
    fn reject(&mut self, problem: impl Into<String>) {
        if self.violation.is_none() {
            self.violation = Some(problem.into());
        }
    }

    fn check_use(&mut self, tree: &syn::UseTree, prefix: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(ident_name(&path.ident));
                self.check_use(&path.tree, prefix);
                prefix.pop();
            }
            syn::UseTree::Name(name) => self.check_use_leaf(prefix, &ident_name(&name.ident)),
            syn::UseTree::Rename(rename) => {
                let target = ident_name(&rename.rename);
                if is_namespace(&target) {
                    self.reject(format!("{target} namespace binding"));
                }
                self.check_use_leaf(prefix, &ident_name(&rename.ident));
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.check_use(item, prefix);
                }
            }
            syn::UseTree::Glob(_) => {
                if prefix.iter().any(|segment| is_namespace(segment)) {
                    self.reject(format!("{}::* glob", prefix.join("::")));
                }
            }
        }
    }

    /// One imported item `prefix::leaf` (`leaf` is `self` for the module).
    fn check_use_leaf(&mut self, prefix: &[String], leaf: &str) {
        let mut path = prefix.to_vec();
        if leaf != "self" {
            path.push(leaf.to_owned());
        }
        match path.as_slice() {
            [only] if only == "std" => self.reject("std namespace alias"),
            _ => self.check_segments(&path),
        }
    }

    /// Any `process` segment must open `std::process::ExitCode` (or one of
    /// its associated items).
    fn check_segments(&mut self, segments: &[String]) {
        for (index, segment) in segments.iter().enumerate() {
            if segment != "process" {
                continue;
            }
            let allowed = index > 0
                && segments[index - 1] == "std"
                && segments.get(index + 1).map(String::as_str) == Some(ALLOWED_PROCESS_ITEM);
            if !allowed {
                self.reject(segments.join("::"));
            }
        }
    }
}

impl<'ast> Visit<'ast> for ProcessPolicy {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.check_use(&item.tree, &mut Vec::new());
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        let bound = item
            .rename
            .as_ref()
            .map_or_else(|| ident_name(&item.ident), |(_, rename)| ident_name(rename));
        if ident_name(&item.ident) == "std" || is_namespace(&bound) {
            self.reject(format!("extern crate binding {bound}"));
        }
        syn::visit::visit_item_extern_crate(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        let name = ident_name(&item.ident);
        if is_namespace(&name) {
            self.reject(format!("{name} module shadows the namespace"));
        }
        syn::visit::visit_item_mod(self, item);
    }

    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<String> = path
            .segments
            .iter()
            .map(|segment| ident_name(&segment.ident))
            .collect();
        self.check_segments(&segments);
        syn::visit::visit_path(self, path);
    }
}

/// `std` and `process` are the namespaces a binding must not rename.
fn is_namespace(name: &str) -> bool {
    matches!(name, "std" | "process")
}

/// Identifier text with any `r#` raw prefix removed.
fn ident_name(ident: &syn::Ident) -> String {
    ident.unraw().to_string()
}

#[cfg(test)]
#[path = "impl_repo_deps_process_policy_tests.rs"]
mod tests;
