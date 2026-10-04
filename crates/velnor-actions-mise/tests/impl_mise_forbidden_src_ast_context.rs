//! Allocation authority has one fixed, inspected base; caller paths only exclude.
use super::shapes::{expression, named_path};
use syn::visit::{self, Visit};
use syn::{Expr, Item, Pat};

pub(crate) fn allocator_ready(file: &syn::File) -> bool {
    let helpers: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "absolute_temp_directory" => Some(function),
            _ => None,
        })
        .collect();
    if helpers.len() != 1 {
        return false;
    }
    let helper = helpers[0];
    let Some(syn::Stmt::Local(local)) = helper.block.stmts.first() else {
        return false;
    };
    if !matches!(&local.pat, Pat::Ident(p) if p.ident == "base")
        || !local.init.as_ref().is_some_and(|i| fixed_tmp(&i.expr))
    {
        return false;
    }
    let Some(syn::Stmt::Expr(last, None)) = helper.block.stmts.last() else {
        return false;
    };
    if !base_success(last) {
        return false;
    }
    let mut check = BaseCheck {
        bindings: 0,
        valid: true,
    };
    check.visit_item_fn(helper);
    check.valid && check.bindings == 1
}

fn fixed_tmp(expr: &Expr) -> bool {
    let Expr::MethodCall(map) = expression(expr) else {
        return false;
    };
    if map.method != "map_err" || map.args.len() != 1 {
        return false;
    }
    let Expr::MethodCall(canonical) = map.receiver.as_ref() else {
        return false;
    };
    if canonical.method != "canonicalize" || !canonical.args.is_empty() {
        return false;
    }
    let Expr::Call(new) = canonical.receiver.as_ref() else {
        return false;
    };
    named_path(&new.func, &["Path", "new"])
        && new.args.len() == 1
        && matches!(&new.args[0], Expr::Lit(l) if matches!(&l.lit, syn::Lit::Str(s) if s.value() == "/tmp"))
}

fn base_success(expr: &Expr) -> bool {
    matches!(expression(expr), Expr::Call(c) if base_success_call(c))
}

fn base_success_call(call: &syn::ExprCall) -> bool {
    named_path(&call.func, &["Ok"]) && call.args.len() == 1 && named_path(&call.args[0], &["base"])
}

struct BaseCheck {
    bindings: usize,
    valid: bool,
}
impl<'ast> Visit<'ast> for BaseCheck {
    fn visit_pat_ident(&mut self, pat: &'ast syn::PatIdent) {
        if pat.ident == "base" {
            self.bindings += 1;
        }
        visit::visit_pat_ident(self, pat);
    }
    fn visit_expr_assign(&mut self, expr: &'ast syn::ExprAssign) {
        if named_path(&expr.left, &["base"]) {
            self.valid = false;
        }
        visit::visit_expr_assign(self, expr);
    }
    fn visit_expr_method_call(&mut self, expr: &'ast syn::ExprMethodCall) {
        if named_path(&expr.receiver, &["base"])
            && !matches!(
                expr.method.to_string().as_str(),
                "starts_with" | "ancestors"
            )
        {
            self.valid = false;
        }
        visit::visit_expr_method_call(self, expr);
    }
    fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
        if named_path(&expr.func, &["Ok"]) && !base_success_call(expr) {
            self.valid = false;
        }
        visit::visit_expr_call(self, expr);
    }
}

pub(crate) fn readonly_flags(file: &syn::File) -> bool {
    file.items.iter().all(|item| match item {
        Item::Const(c) if c.ident == "NOFOLLOW_FLAG" => integer_one_of(&c.expr, &[0x20000, 0x100]),
        Item::Const(c) if c.ident == "NONBLOCK_FLAG" => integer_one_of(&c.expr, &[0x800, 0x4]),
        _ => true,
    })
}
fn integer_one_of(expr: &Expr, allowed: &[u32]) -> bool {
    matches!(expr, Expr::Lit(l) if matches!(&l.lit, syn::Lit::Int(i)
        if i.base10_parse::<u32>().is_ok_and(|v| allowed.contains(&v))))
}

pub(crate) fn bound_handle_helper(file: &syn::File) -> bool {
    let helpers: Vec<_> = file
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Fn(f) if f.sig.ident == "open_bound_directory" => Some(f),
            _ => None,
        })
        .collect();
    if helpers.len() != 1 {
        return false;
    }
    let helper = helpers[0];
    if helper.sig.inputs.len() != 1
        || !matches!(&helper.sig.inputs[0], syn::FnArg::Typed(a)
        if matches!(a.pat.as_ref(), Pat::Ident(p) if p.ident == "path")
            && matches!(a.ty.as_ref(), syn::Type::Reference(r) if r.mutability.is_none()
                && super::shapes::type_name(&r.elem).as_deref() == Some("Path")))
    {
        return false;
    }
    if !matches!(&helper.sig.output, syn::ReturnType::Type(_, t) if matches!(t.as_ref(), syn::Type::Path(p)
        if p.path.segments.len() == 2 && p.path.segments[0].ident == "io" && p.path.segments[1].ident == "Result"
        && matches!(&p.path.segments[1].arguments, syn::PathArguments::AngleBracketed(a)
            if a.args.len() == 1 && matches!(&a.args[0], syn::GenericArgument::Type(syn::Type::Path(f))
                if f.path.segments.len() == 2 && f.path.segments[0].ident == "fs" && f.path.segments[1].ident == "File"))))
    {
        return false;
    }
    let mut check = HandleCheck {
        valid: true,
        handles: 0,
        returns: 0,
    };
    check.visit_item_fn(helper);
    check.valid && check.handles == 1 && check.returns == 1
}
struct HandleCheck {
    valid: bool,
    handles: usize,
    returns: usize,
}
impl<'ast> Visit<'ast> for HandleCheck {
    fn visit_local(&mut self, local: &'ast syn::Local) {
        if matches!(&local.pat, Pat::Ident(p) if p.ident == "handle") {
            self.handles += 1;
            if !local.init.as_ref().is_some_and(|i| matches!(expression(&i.expr), Expr::MethodCall(c)
                if c.method == "open" && c.args.len() == 1 && named_path(&c.args[0], &["path"])
                    && matches!(c.receiver.as_ref(), Expr::MethodCall(flags) if flags.method == "custom_flags"
                        && matches!(flags.receiver.as_ref(), Expr::MethodCall(read) if read.method == "read"
                            && matches!(read.receiver.as_ref(), Expr::Call(new) if named_path(&new.func, &["fs", "OpenOptions", "new"])))))) { self.valid = false; }
        }
        visit::visit_local(self, local);
    }
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if named_path(&call.func, &["Ok"]) {
            self.returns += 1;
            if call.args.len() != 1 || !named_path(&call.args[0], &["handle"]) {
                self.valid = false;
            }
        }
        visit::visit_expr_call(self, call);
    }
    fn visit_expr_assign(&mut self, assign: &'ast syn::ExprAssign) {
        if named_path(&assign.left, &["handle"]) {
            self.valid = false;
        }
        visit::visit_expr_assign(self, assign);
    }
}

pub(crate) fn flag_symbols(
    name: &str,
    file: &syn::File,
) -> std::collections::HashMap<String, super::Origin> {
    let mut symbols: std::collections::HashMap<_, _> = file
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Const(c)
                if c.ident == "NOFOLLOW_FLAG" && integer_one_of(&c.expr, &[0x20000, 0x100]) =>
            {
                Some((c.ident.to_string(), super::Origin::NoFollowFlag))
            }
            Item::Const(c)
                if c.ident == "NONBLOCK_FLAG" && integer_one_of(&c.expr, &[0x800, 0x4]) =>
            {
                Some((c.ident.to_string(), super::Origin::NonBlockFlag))
            }
            _ => None,
        })
        .collect();
    if name == "command_git_private_root.rs" {
        symbols.extend(super::shapes::slot_symbols(file).unwrap_or_default());
    }
    symbols
}

fn sensitive(name: &syn::Ident) -> bool {
    matches!(
        name.to_string().as_str(),
        "std"
            | "fmt"
            | "Formatter"
            | "Path"
            | "PathBuf"
            | "File"
            | "OpenOptions"
            | "DirBuilder"
            | "AtomicU64"
            | "Ordering"
            | "SystemTime"
            | "Debug"
            | "Clone"
            | "Copy"
            | "PartialEq"
            | "Eq"
            | "Hash"
            | "Default"
            | "Serialize"
            | "Deserialize"
            | "MetadataExt"
            | "DirBuilderExt"
            | "OpenOptionsExt"
            | "PermissionsExt"
    )
}
pub(crate) fn standard_names(name: &str, file: &syn::File) -> bool {
    struct Names {
        valid: bool,
        private_root: bool,
    }
    impl<'ast> Visit<'ast> for Names {
        fn visit_pat_struct(&mut self, pat: &'ast syn::PatStruct) {
            if pat.path.segments.last().is_some_and(|s| {
                s.ident == "PrivateGitRoot" || (self.private_root && s.ident == "Self")
            }) {
                self.valid = false;
            }
            visit::visit_pat_struct(self, pat);
        }
        fn visit_ident(&mut self, ident: &'ast syn::Ident) {
            self.valid &= !ident.to_string().starts_with("r#");
        }
        fn visit_item_extern_crate(&mut self, _: &'ast syn::ItemExternCrate) {
            self.valid = false;
        }
        fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
            if sensitive(&item.ident) {
                self.valid = false;
            }
            visit::visit_item_type(self, item);
        }
        fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
            if sensitive(&item.ident) {
                self.valid = false;
            }
            visit::visit_item_struct(self, item);
        }
        fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
            if sensitive(&item.ident) {
                self.valid = false;
            }
            visit::visit_item_enum(self, item);
        }
        fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
            if sensitive(&item.ident) || item.ident == "PrivateGitRoot" {
                self.valid = false;
            }
            visit::visit_item_union(self, item);
        }
        fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
            if sensitive(&item.ident) || item.ident == "PrivateGitRoot" {
                self.valid = false;
            }
            visit::visit_item_trait(self, item);
        }
        fn visit_type_param(&mut self, item: &'ast syn::TypeParam) {
            if sensitive(&item.ident) {
                self.valid = false;
            }
            visit::visit_type_param(self, item);
        }
    }
    let mut names = Names {
        valid: true,
        private_root: matches!(
            name,
            "command_git_private_root.rs" | "command_git_private_root_files.rs"
        ),
    };
    names.visit_file(file);
    names.valid
}

pub(crate) fn signature_attributes(sig: &syn::Signature, attrs: &[syn::Attribute]) -> bool {
    attrs.iter().all(super::shapes::attribute_allowed)
        && sig.inputs.iter().all(|arg| {
            let attrs = match arg {
                syn::FnArg::Receiver(r) => &r.attrs,
                syn::FnArg::Typed(t) => &t.attrs,
            };
            attrs.iter().all(super::shapes::attribute_allowed)
        })
}

// Authority modules admit only canonical extension operations.
pub(crate) fn protected_extension(name: &str, method: &syn::Ident) -> bool {
    matches!(
        name,
        "command_git_private_root.rs"
            | "command_git_private_root_files.rs"
            | "command_git_index_fs.rs"
    ) && matches!(
        method.to_string().as_str(),
        "nlink"
            | "dev"
            | "ino"
            | "mode"
            | "custom_flags"
            | "file_type"
            | "is_file"
            | "metadata"
            | "display"
            | "join"
    )
}
