//! Filesystem authority follows parsed receivers and closed destination provenance.
use std::collections::HashMap;
use syn::visit::{self, Visit};
use syn::{Expr, ImplItemFn, ItemFn, Pat};
#[path = "impl_mise_forbidden_src_ast_context.rs"]
mod context;
#[path = "impl_mise_forbidden_src_ast_modules.rs"]
mod modules;
pub(crate) use modules::{check_source_graph, check_test_reverse_edges};
#[path = "impl_mise_forbidden_src_ast_replacement.rs"]
mod replacement;
#[path = "impl_mise_forbidden_src_ast_shapes.rs"]
mod shapes;
#[path = "impl_mise_forbidden_src_ast_sinks.rs"]
mod sinks;
#[path = "impl_mise_forbidden_src_ast_tests.rs"]
mod tests;
use shapes::{Origin, argument, expression, named_path};

pub(crate) fn check_filesystem(name: &str, source: &str, violations: &mut Vec<String>) {
    scan(name, source, violations, false);
}

fn scan(name: &str, source: &str, violations: &mut Vec<String>, graph: bool) {
    match syn::parse_file(source) {
        Ok(file) => {
            if !context::standard_names(name, &file)
                || !context::readonly_flags(&file)
                || (name == "command_git_private_root.rs"
                    && (!context::bound_handle_helper(&file)
                        || shapes::slot_symbols(&file).is_none()))
            {
                violations.push(format!(
                    "{name}: changed retained filesystem handle/flags contract"
                ));
            }
            if name == "command_git_index_fs.rs" && !context::allocator_ready(&file) {
                violations.push(format!(
                    "{name}: filesystem allocator base is not fixed /tmp"
                ));
            }
            Scanner {
                name,
                violations,
                owner: String::new(),
                function: String::new(),
                trusted: false,
                origins: HashMap::new(),
                verified: false,
                flag_symbols: context::flag_symbols(name, &file),
                graph,
            }
            .visit_file(&file)
        }
        Err(error) => violations.push(format!("{name}: filesystem AST parse failed: {error}")),
    }
}

struct Scanner<'a> {
    name: &'a str,
    violations: &'a mut Vec<String>,
    owner: String,
    function: String,
    trusted: bool,
    origins: HashMap<String, Origin>,
    verified: bool,
    flag_symbols: HashMap<String, Origin>,
    graph: bool,
}

impl Scanner<'_> {
    fn reject(&mut self, detail: &str) {
        self.violations.push(format!(
            "{}: {}: filesystem authority {detail}",
            self.name, self.function
        ));
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let old = self.owner.clone();
        self.owner = shapes::type_name(&item.self_ty).unwrap_or_default();
        if self.owner == "PrivateGitRoot"
            && !matches!(
                self.name,
                "command_git_private_root.rs" | "command_git_private_root_files.rs"
            )
        {
            self.reject("foreign PrivateGitRoot implementation");
        }
        visit::visit_item_impl(self, item);
        self.owner = old;
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.begin(&item.sig, &item.attrs);
        self.visit_block(&item.block);
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        self.begin(&item.sig, &item.attrs);
        if context::protected_extension(self.name, &item.sig.ident) {
            self.reject("locally forged filesystem extension method");
        }
        if self.owner == "PrivateGitRoot"
            && self.function == "create"
            && !matches!(item.vis, syn::Visibility::Inherited)
        {
            self.reject("public root path allocator");
        }
        if self.owner == "PrivateGitRoot"
            && matches!(self.function.as_str(), "slot_path" | "directory_path")
            && !shapes::slot_accessor(item)
        {
            self.reject("dynamic owned slot accessor");
        }
        self.visit_block(&item.block);
    }

    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        if context::protected_extension(self.name, &item.sig.ident) {
            self.reject("locally forged filesystem extension trait method");
        }
        visit::visit_trait_item_fn(self, item);
    }

    fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
        if pat.ident == "handle" {
            self.origins.retain(|_, origin| {
                !matches!(origin, Origin::HeldMetadata | Origin::NamedMetadata)
            });
        }
        self.origins.remove(&pat.ident.to_string());
        visit::visit_pat_ident(self, pat);
    }

    fn visit_item_const(&mut self, item: &'ast syn::ItemConst) {
        self.origins.remove(&item.ident.to_string());
        visit::visit_item_const(self, item);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        self.check_root_binding(local);
        visit::visit_local(self, local);
        if let Pat::Ident(pat) = &local.pat {
            let origin = local.init.as_ref().map_or(Origin::Unknown, |init| {
                if matches!(expression(&init.expr), Expr::Path(_)) {
                    if let Expr::Path(p) = expression(&init.expr) {
                        if p.path.get_ident().is_some_and(|n| {
                            self.origins.get(&n.to_string()) == Some(&Origin::RootBorrow)
                        }) {
                            return Origin::RootBorrow;
                        }
                    }
                    Origin::Unknown
                } else {
                    self.origin(&init.expr)
                }
            });
            self.origins.insert(pat.ident.to_string(), origin);
        }
        for name in shapes::root_alias_bindings(local, &self.origins) {
            self.origins.insert(name, Origin::RootBorrow);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.check_root_receiver(call);
        if shapes::path_mutator(&call.method.to_string())
            && (self.projection(&call.receiver)
                || !matches!(self.origin(&call.receiver), Origin::Unknown | Origin::Input))
        {
            self.reject("mutated destination provenance");
        }
        visit::visit_expr_method_call(self, call);
        if shapes::method_sink(&call.method.to_string())
            || (call.method == "mode" && !call.args.is_empty())
            || (call.method == "open" && !shapes::readonly_options(&call.receiver, &self.origins))
        {
            if !self.method_allowed(call) {
                self.reject(&format!("unowned method `{}`", call.method));
            }
        }
        if call.method == "mode" && self.method_origin(call) == Origin::AllocBuilder {
            if let Expr::Path(path) = call.receiver.as_ref() {
                if let Some(name) = path.path.get_ident() {
                    self.origins.insert(name.to_string(), Origin::AllocBuilder);
                }
            }
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let sink = matches!(call.func.as_ref(), Expr::Path(path)
            if path.path.segments.last().is_some_and(|s| shapes::function_sink(&s.ident.to_string())));
        if sink {
            for arg in &call.args {
                self.visit_expr(arg);
            }
            if !self.call_allowed(call) {
                self.reject("unowned filesystem call");
            }
        } else {
            visit::visit_expr_call(self, call);
        }
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        let origins = self.origins.clone();
        let verified = self.verified;
        for stmt in &block.stmts {
            self.visit_stmt(stmt);
            if self.trusted && self.function == "replace_contents" {
                replacement::admit_guard(stmt, &mut self.origins);
            }
            if shapes::successful_verification(stmt) {
                self.verified = true;
            }
        }
        self.origins = origins;
        self.verified = verified;
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        if path
            .path
            .segments
            .last()
            .is_some_and(|s| shapes::function_sink(&s.ident.to_string()))
        {
            // Direct calls receive a destination check; references/aliases never gain authority.
            self.reject("filesystem function reference or alias");
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
        if let Expr::Path(path) = expression(&assign.left) {
            if let Some(name) = path.path.get_ident() {
                if self
                    .origins
                    .get(&name.to_string())
                    .is_some_and(|o| !matches!(o, Origin::Unknown | Origin::Input))
                {
                    self.reject("reassigned destination provenance");
                }
                self.origins.remove(&name.to_string());
            }
        }
        if self.projection(&assign.left) {
            self.reject("mutable root destination");
        }
        visit::visit_expr_assign(self, assign);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if shapes::dangerous_import(&item.tree)
            || !shapes::authority_import(self.name, &item.tree, &[])
        {
            self.reject("filesystem sink import or alias");
        }
        visit::visit_item_use(self, item);
    }

    fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
        if expr.path.segments.last().is_some_and(|s| {
            s.ident == "PrivateGitRoot" || (s.ident == "Self" && self.owner == "PrivateGitRoot")
        }) {
            if !self.trusted
                || self.function != "create"
                || !shapes::owned_constructor(expr, &self.origins)
            {
                self.reject("forged PrivateGitRoot construction");
            }
        }
        visit::visit_expr_struct(self, expr);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if matches!(item.ident.to_string().as_str(), "Formatter" | "fmt" | "std") {
            self.reject("shadowed standard type");
        }
        if item.ident == "PrivateGitRoot"
            && (self.name != "command_git_private_root.rs" || !shapes::root_declaration(item))
        {
            self.reject("forgeable PrivateGitRoot declaration");
        }
        visit::visit_item_struct(self, item);
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        let origins = self.origins.clone();
        let verified = self.verified;
        for input in &closure.inputs {
            if let Pat::Ident(p) = input {
                self.origins.remove(&p.ident.to_string());
            }
        }
        visit::visit_expr_closure(self, closure);
        self.origins = origins;
        self.verified = verified;
    }

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if self.graph && modules::test_module(item) {
            return;
        }
        if !shapes::canonical_fs_module(self.name, item)
            && matches!(
                item.ident.to_string().as_str(),
                "std"
                    | "fmt"
                    | "Formatter"
                    | "fs"
                    | "PrivateGitRoot"
                    | "OwnedFile"
                    | "OwnedDirectory"
            )
        {
            self.reject("shadowed authority namespace");
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_expr_reference(&mut self, reference: &'ast syn::ExprReference) {
        if reference.mutability.is_some()
            && (self.projection(&reference.expr)
                || !matches!(
                    self.origin(&reference.expr),
                    Origin::Unknown | Origin::Input
                ))
        {
            self.reject("mutable destination alias");
        }
        visit::visit_expr_reference(self, reference);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        if shapes::root_type(&item.ty)
            || matches!(
                item.ident.to_string().as_str(),
                "PrivateGitRoot" | "OwnedFile" | "OwnedDirectory" | "Formatter" | "fmt" | "std"
            )
        {
            self.reject("root type alias");
        }
        visit::visit_item_type(self, item);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if !shapes::macro_allowed(mac)
            || shapes::macro_sink(mac)
            || (mac
                .path
                .segments
                .last()
                .is_some_and(|s| s.ident == "write" || s.ident == "writeln")
                && !shapes::formatter_macro(mac, &self.origins))
        {
            self.reject("filesystem sink hidden in macro tokens");
        }
        visit::visit_macro(self, mac);
    }

    fn visit_attribute(&mut self, attr: &'ast syn::Attribute) {
        if !shapes::attribute_allowed(attr) {
            self.reject("uninspected source expansion attribute");
        }
        visit::visit_attribute(self, attr);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if item.ident == "PrivateGitRoot" {
            self.reject("alternate root declaration");
        }
        if let Some(expected) = shapes::slots(&item.ident.to_string()) {
            let actual: Vec<_> = item.variants.iter().map(|v| v.ident.to_string()).collect();
            if self.name != "command_git_private_root.rs"
                || actual != expected
                || item.variants.iter().any(|v| {
                    !matches!(v.fields, syn::Fields::Unit)
                        || v.discriminant.is_some()
                        || !v.attrs.is_empty()
                })
            {
                self.reject("open or changed owned slots");
            }
        }
        visit::visit_item_enum(self, item);
    }
}
