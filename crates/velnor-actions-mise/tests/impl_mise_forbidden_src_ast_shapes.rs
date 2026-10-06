//! Closed AST shapes; arbitrary paths and aliases never carry writable authority.
use std::collections::HashMap;
use syn::{Expr, FnArg, Pat, Signature, Type, UseTree};
#[path = "impl_mise_forbidden_src_ast_contract.rs"]
mod contract;
#[path = "impl_mise_forbidden_src_ast_expansions.rs"]
mod expansions;
#[path = "impl_mise_forbidden_src_ast_origins.rs"]
mod origins;
use contract::return_valid;
pub(crate) use contract::{
    authority_import, formatter_macro, formatter_type, path_mutator, readonly_options,
    root_declaration,
};
pub(crate) use expansions::{attribute_allowed, macro_allowed};
pub(crate) use origins::{method_origin, slot_symbols};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Unknown,
    Input,
    Formatter,
    RootBorrow,
    Flags,
    NoFollowFlag,
    NonBlockFlag,
    Counter,
    Time,
    FileSlot,
    DirectorySlot,
    EffectiveConfig,
    BootstrapConfig,
    OptionsNew,
    WriteEnabled,
    ExclusiveEnabled,
    FlagsEnabled,
    WriteOptions,
    DirectoryBuilderNew,
    DirectoryBuilder,
    OwnedFileHandle,
    ReplaceOptions,
    PendingFileHandle,
    HeldCheckedHandle,
    ExistingFileHandle,
    BeforeMetadata,
    NamedMetadata,
    HeldMetadata,
    TempBase,
    TempName,
    AllocDirectory,
    AllocBuilderNew,
    AllocBuilder,
    AllocRoot,
    RootHandle,
    DirectoryNames,
    FileNames,
    Directories,
    Files,
}

pub(crate) fn expression(expr: &Expr) -> &Expr {
    match expr {
        Expr::Reference(e) => expression(&e.expr),
        Expr::Try(e) => expression(&e.expr),
        Expr::Paren(e) => expression(&e.expr),
        Expr::Group(e) => expression(&e.expr),
        _ => expr,
    }
}

pub(crate) fn named_path(expr: &Expr, names: &[&str]) -> bool {
    matches!(expression(expr), Expr::Path(p) if p.qself.is_none()
        && p.path.segments.len() == names.len()
        && p.path.segments.iter().zip(names).all(|(s,n)| s.ident == *n && matches!(s.arguments, syn::PathArguments::None)))
}

pub(crate) fn self_field(expr: &Expr, name: &str) -> bool {
    matches!(expression(expr), Expr::Field(f) if named_path(&f.base, &["self"])
        && matches!(&f.member, syn::Member::Named(n) if n == name))
}

pub(crate) fn argument(call: &syn::ExprMethodCall, index: usize) -> Option<&Expr> {
    call.args.get(index)
}

pub(crate) fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(p) => p.path.get_ident().map(ToString::to_string),
        _ => None,
    }
}

pub(crate) fn profile(name: &str, owner: &str, function: &str) -> bool {
    match (name, owner) {
        ("command_git_private_root_files.rs", "PrivateGitRoot") => matches!(
            function,
            "write_file" | "replace_contents" | "create_directory" | "install_effective_config"
        ),
        ("command_git_private_root.rs", "PrivateGitRoot") => {
            matches!(function, "cleanup" | "create")
        }
        ("command_git_index_fs.rs", "") => matches!(function, "create_private_directory"),
        _ => false,
    }
}

pub(crate) fn signature_valid(owner: &str, sig: &Signature) -> bool {
    if !return_valid(sig) {
        return false;
    }
    if sig.asyncness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
    {
        return false;
    }
    let expected: &[(&str, &str)] = match sig.ident.to_string().as_str() {
        "write_file" | "replace_contents" => &[("slot", "OwnedFile"), ("bytes", "bytes")],
        "create_directory" => &[("slot", "OwnedDirectory")],
        "install_effective_config" | "cleanup" => &[],
        "create" | "create_private_directory" => &[("excluded", "paths")],

        _ => return false,
    };
    let mut args = sig.inputs.iter();
    if owner == "PrivateGitRoot" && sig.ident != "create" {
        let Some(FnArg::Receiver(receiver)) = args.next() else {
            return false;
        };
        if receiver.reference.is_none()
            || receiver.colon_token.is_some()
            || receiver.mutability.is_some() != (sig.ident == "cleanup")
        {
            return false;
        }
    }
    let remaining: Vec<_> = args.collect();
    remaining.len() == expected.len() && remaining.iter().zip(expected).all(|(arg,(name,ty))| {
        matches!(arg, FnArg::Typed(a) if matches!(a.pat.as_ref(), Pat::Ident(p)
            if p.ident == *name && p.by_ref.is_none() && p.subpat.is_none()) && parameter_type(&a.ty, ty))
    })
}

fn parameter_type(ty: &Type, expected: &str) -> bool {
    match (ty, expected) {
        (Type::Reference(r), "bytes") if r.mutability.is_none() => {
            matches!(r.elem.as_ref(), Type::Slice(s) if type_name(&s.elem).as_deref() == Some("u8"))
        }
        (Type::Reference(r), "path") if r.mutability.is_none() => {
            type_name(&r.elem).as_deref() == Some("Path")
        }
        (Type::Reference(r), "paths") if r.mutability.is_none() => {
            matches!(r.elem.as_ref(), Type::Slice(s)
            if matches!(s.elem.as_ref(), Type::Reference(p) if p.mutability.is_none() && type_name(&p.elem).as_deref() == Some("Path")))
        }
        (_, name) => type_name(ty).as_deref() == Some(name),
    }
}

pub(crate) fn method_sink(name: &str) -> bool {
    matches!(
        name,
        "append"
            | "write"
            | "write_all"
            | "write_vectored"
            | "write_fmt"
            | "create"
            | "create_new"
            | "truncate"
            | "set_len"
            | "set_permissions"
            | "write_at"
            | "write_all_at"
    )
}
pub(crate) fn function_sink(name: &str) -> bool {
    matches!(
        name,
        "append"
            | "truncate"
            | "write_all"
            | "write_vectored"
            | "write_fmt"
            | "write_at"
            | "write_all_at"
            | "set_len"
            | "write"
            | "create"
            | "create_new"
            | "create_dir"
            | "create_dir_all"
            | "copy"
            | "rename"
            | "remove_created_directory"
            | "remove_file"
            | "remove_dir"
            | "remove_dir_all"
            | "set_permissions"
            | "symlink"
            | "hard_link"
    )
}

pub(crate) fn dangerous_import(tree: &UseTree) -> bool {
    match tree {
        UseTree::Path(p) => {
            (p.ident == "fs" && contains_glob(&p.tree)) || dangerous_import(&p.tree)
        }
        UseTree::Name(n) => function_sink(&n.ident.to_string()),
        UseTree::Rename(n) => {
            function_sink(&n.ident.to_string())
                || function_sink(&n.rename.to_string())
                || !(n.ident == "fs" && n.rename == "std_fs"
                    || n.ident == "Result" && n.rename == "FmtResult")
        }
        UseTree::Group(g) => g.items.iter().any(dangerous_import),
        UseTree::Glob(_) => false,
    }
}

pub(crate) fn slots(name: &str) -> Option<Vec<String>> {
    let slots: &[&str] = match name {
        "OwnedFile" => &[
            "Index",
            "Head",
            "BootstrapConfig",
            "EffectiveConfig",
            "ConfigFragment",
            "InfoAttributes",
            "InfoExclude",
        ],
        "OwnedDirectory" => &["GitDir", "GitInfo", "GitRefs", "GitHeads", "GitObjects"],
        _ => return None,
    };
    Some(slots.iter().map(|s| (*s).to_owned()).collect())
}

pub(crate) fn successful_verification(stmt: &syn::Stmt) -> bool {
    matches!(stmt, syn::Stmt::Expr(Expr::Try(t), _) if matches!(t.expr.as_ref(), Expr::MethodCall(c)
        if named_path(&c.receiver, &["self"]) && c.method == "verify_binding" && c.args.is_empty()))
}

fn contains_glob(tree: &UseTree) -> bool {
    match tree {
        UseTree::Glob(_) => true,
        UseTree::Path(p) => contains_glob(&p.tree),
        UseTree::Group(g) => g.items.iter().any(contains_glob),
        _ => false,
    }
}

pub(crate) fn slot_accessor(item: &syn::ImplItemFn) -> bool {
    let field = if item.sig.ident == "slot_path" {
        "files"
    } else {
        "directories"
    };
    let ty = if field == "files" {
        "OwnedFile"
    } else {
        "OwnedDirectory"
    };
    if !matches!(&item.sig.output, syn::ReturnType::Type(_, ty)
        if matches!(ty.as_ref(), Type::Reference(r) if r.mutability.is_none() && type_name(&r.elem).as_deref() == Some("Path")))
    {
        return false;
    }
    if item.sig.inputs.len() != 2
        || !matches!(&item.sig.inputs[0], FnArg::Receiver(r) if r.reference.is_some() && r.mutability.is_none())
        || !matches!(&item.sig.inputs[1], FnArg::Typed(a) if parameter_type(&a.ty, ty)
            && matches!(a.pat.as_ref(), Pat::Ident(p) if p.ident == "slot"))
        || item.block.stmts.len() != 1
    {
        return false;
    }
    matches!(&item.block.stmts[0], syn::Stmt::Expr(e, None) if matches!(expression(e), Expr::Index(i)
        if self_field(&i.expr, field) && matches!(i.index.as_ref(), Expr::Cast(c)
            if named_path(&c.expr, &["slot"]) && type_name(&c.ty).as_deref() == Some("usize"))))
}

pub(crate) fn macro_sink(mac: &syn::Macro) -> bool {
    mac.tokens.clone().into_iter().any(|token| {
        let text = token.to_string();
        if matches!(text.chars().next(), Some('(' | '[' | '{')) {
            return syn::parse_str::<syn::Macro>(&format!("policy!{text}"))
                .map_or(true, |nested| macro_sink(&nested));
        }
        function_sink(&text) || method_sink(&text)
    })
}

pub(crate) fn origin(
    expr: &Expr,
    locals: &HashMap<String, Origin>,
    function: &str,
    trusted: bool,
) -> Origin {
    origins::origin(expr, locals, function, trusted)
}
pub(crate) fn owned_constructor(expr: &syn::ExprStruct, locals: &HashMap<String, Origin>) -> bool {
    origins::owned_constructor(expr, locals)
}

pub(crate) fn root_type(ty: &Type) -> bool {
    match ty {
        Type::Reference(r) => root_type(&r.elem),
        Type::Path(p) => p
            .path
            .segments
            .last()
            .is_some_and(|s| s.ident == "PrivateGitRoot"),
        _ => false,
    }
}

pub(crate) fn root_projection(
    expr: &Expr,
    locals: &HashMap<String, Origin>,
    private_module: bool,
) -> bool {
    match expression(expr) {
        Expr::Index(i) => root_projection(&i.expr, locals, private_module),
        Expr::Field(f)
            if matches!(&f.member, syn::Member::Named(n)
            if ["path", "files", "directories", "handle", "identity"].iter().any(|name| n == name)) =>
        {
            private_module
                || matches!(expression(&f.base), Expr::Path(p)
                if p.path.get_ident().is_some_and(|n| locals.get(&n.to_string()) == Some(&Origin::RootBorrow)))
        }
        Expr::Field(f) => root_projection(&f.base, locals, private_module),
        _ => false,
    }
}

pub(crate) fn root_alias_bindings(
    local: &syn::Local,
    locals: &HashMap<String, Origin>,
) -> Vec<String> {
    use syn::visit::{self, Visit};
    struct RootUse<'a> {
        locals: &'a HashMap<String, Origin>,
        found: bool,
    }
    impl<'ast> Visit<'ast> for RootUse<'_> {
        fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
            if expr
                .path
                .get_ident()
                .is_some_and(|n| self.locals.get(&n.to_string()) == Some(&Origin::RootBorrow))
            {
                self.found = true;
            }
            visit::visit_expr_path(self, expr);
        }
    }
    struct Bindings {
        names: Vec<String>,
    }
    impl<'ast> Visit<'ast> for Bindings {
        fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
            self.names.push(pat.ident.to_string());
            visit::visit_pat_ident(self, pat);
        }
    }
    let Some(init) = &local.init else {
        return Vec::new();
    };
    let mut uses = RootUse {
        locals,
        found: false,
    };
    uses.visit_expr(&init.expr);
    if !uses.found {
        return Vec::new();
    }
    let mut bindings = Bindings { names: Vec::new() };
    bindings.visit_pat(&local.pat);
    bindings.names
}

pub(crate) fn canonical_fs_module(name: &str, item: &syn::ItemMod) -> bool {
    name == "command_git_index.rs" && item.ident == "fs" && item.content.is_none()
        && item.attrs.iter().any(|a| a.path().is_ident("path") && matches!(&a.meta, syn::Meta::NameValue(v)
            if matches!(&v.value, Expr::Lit(l) if matches!(&l.lit, syn::Lit::Str(s) if s.value() == "command_git_index_fs.rs"))))
}
