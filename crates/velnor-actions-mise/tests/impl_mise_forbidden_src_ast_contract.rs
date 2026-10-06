//! Exact structural contracts for root and standard filesystem types.
use super::{expression, named_path, type_name};
use syn::{Expr, Signature, Type, UseTree};
pub(crate) fn root_declaration(item: &syn::ItemStruct) -> bool {
    if !item.fields.iter().all(root_field_type) {
        return false;
    }
    let names: Vec<_> = item
        .fields
        .iter()
        .filter_map(|f| f.ident.as_ref().map(ToString::to_string))
        .collect();
    names
        == [
            "path",
            "directories",
            "files",
            "armed",
            "identity",
            "handle",
        ]
        && item
            .fields
            .iter()
            .all(|f| matches!(f.vis, syn::Visibility::Inherited))
        && item.attrs.iter().all(|a| {
            a.path().is_ident("derive")
                && a.parse_args::<syn::Path>()
                    .is_ok_and(|p| p.is_ident("Debug"))
        })
}

pub(crate) fn authority_import(name: &str, tree: &UseTree, prefix: &[String]) -> bool {
    if matches!(tree, UseTree::Name(n) if n.ident == "Formatter") {
        return prefix == ["std", "fmt"];
    }
    if matches!(tree, UseTree::Name(n) if n.ident == "fmt" || n.ident == "io") {
        return prefix == ["std"];
    }
    if matches!(tree, UseTree::Name(n) if n.ident == "Path" || n.ident == "PathBuf") {
        return prefix == ["std", "path"];
    }
    if let UseTree::Name(n) = tree {
        match n.ident.to_string().as_str() {
            "std" | "serde" | "serde_json" => return prefix.is_empty(),
            "fs" => {
                return prefix == ["std"]
                    || (name == "command_git_repository.rs" && prefix == ["super"]);
            }
            "Serialize" | "Deserialize" => return prefix == ["serde"],
            "json" => return prefix == ["serde_json"],
            "assert" | "assert_eq" | "assert_ne" | "debug_assert" | "concat" | "eprintln"
            | "format" | "include_str" | "include_bytes" | "matches" | "unreachable" | "vec"
            | "debug_assert_eq" | "debug_assert_ne" | "env" | "option_env" | "stringify"
            | "cfg" | "write" | "writeln" => return prefix == ["std"],
            "Debug" => return prefix == ["std", "fmt"],
            "Clone" => return prefix == ["std", "clone"],
            "Copy" => return prefix == ["std", "marker"],
            "PartialEq" | "Eq" => return prefix == ["std", "cmp"],
            "Hash" => return prefix == ["std", "hash"],
            "Default" => return prefix == ["std", "default"],
            "MetadataExt" | "DirBuilderExt" | "OpenOptionsExt" | "PermissionsExt" => {
                return prefix == ["std", "os", "unix", "fs"];
            }
            "Write" => return prefix == ["std", "io"] || prefix == ["std", "fmt"],
            "Read" => return prefix == ["std", "io"],
            "Metadata" => return prefix == ["std", "fs"],
            _ => {}
        }
    }
    match tree {
        UseTree::Path(p) => {
            let mut path = prefix.to_vec();
            path.push(p.ident.to_string());
            authority_import(name, &p.tree, &path)
        }
        UseTree::Group(g) => g.items.iter().all(|t| authority_import(name, t, prefix)),
        UseTree::Name(n) => match n.ident.to_string().as_str() {
            "self" if prefix.last().is_some_and(|p| p == "fs") => prefix == ["std", "fs"],
            "OpenOptions" | "DirBuilder" => prefix == ["std", "fs"],
            "PrivateGitRoot" | "OwnedFile" | "OwnedDirectory"
                if name == "command_git_private_root_files.rs" =>
            {
                prefix == ["super"]
            }
            "PrivateGitRoot" | "OwnedFile" | "OwnedDirectory"
                if name == "command_git_private_root.rs" =>
            {
                false
            }
            "AtomicU64" | "Ordering" => prefix == ["std", "sync", "atomic"],
            "SystemTime" | "UNIX_EPOCH" => prefix == ["std", "time"],
            _ => private_import(name, &n.ident.to_string(), prefix),
        },
        UseTree::Rename(n) => {
            (n.ident == "fs" && n.rename == "std_fs" && prefix == ["std"])
                || (n.ident == "Result" && n.rename == "FmtResult" && prefix == ["std", "fmt"])
        }
        UseTree::Glob(_) => prefix == ["super"] && !authority_module(name),
    }
}

pub(crate) fn return_valid(sig: &Signature) -> bool {
    let expected = match sig.ident.to_string().as_str() {
        "create" => "Self",
        "create_private_directory" | "create_directory" => "PathBuf",
        _ => "unit",
    };
    let syn::ReturnType::Type(_, ty) = &sig.output else {
        return false;
    };
    let Type::Path(path) = ty.as_ref() else {
        return false;
    };
    if path.qself.is_some()
        || path.path.segments.len() != 2
        || path.path.segments[0].ident != "io"
        || path.path.segments[1].ident != "Result"
    {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &path.path.segments[1].arguments else {
        return false;
    };
    if args.args.len() != 1 {
        return false;
    }
    matches!(&args.args[0], syn::GenericArgument::Type(ty) if type_name(ty).as_deref() == Some(expected)
        || expected == "unit" && matches!(ty, Type::Tuple(t) if t.elems.is_empty()))
}

pub(crate) fn path_mutator(name: &str) -> bool {
    matches!(
        name,
        "push"
            | "pop"
            | "clear"
            | "set_extension"
            | "set_file_name"
            | "extend"
            | "insert"
            | "remove"
            | "swap"
            | "sort"
            | "fill"
            | "copy_from_slice"
            | "clone_from"
            | "get_mut"
            | "as_mut"
    )
}

fn root_field_type(field: &syn::Field) -> bool {
    if !field.attrs.is_empty()
        && !(field.ident.as_ref().is_some_and(|n| n == "identity")
            && field.attrs.len() == 1
            && matches!(&field.attrs[0].meta, syn::Meta::List(l) if l.path.is_ident("cfg") && l.tokens.to_string() == "unix"))
    {
        return false;
    }
    let Some(name) = &field.ident else {
        return false;
    };
    match name.to_string().as_str() {
        "path" => type_name(&field.ty).as_deref() == Some("PathBuf"),
        "armed" => type_name(&field.ty).as_deref() == Some("bool"),
        "handle" => {
            matches!(&field.ty, Type::Path(p) if p.qself.is_none() && p.path.segments.len() == 2
            && p.path.segments[0].ident == "fs" && p.path.segments[1].ident == "File")
        }
        "files" | "directories" => {
            matches!(&field.ty, Type::Array(a) if type_name(&a.elem).as_deref() == Some("PathBuf")
            && matches!(&a.len, Expr::Lit(l) if matches!(&l.lit, syn::Lit::Int(i)
                if i.base10_parse::<usize>().ok() == Some(if name == "files" { 7 } else { 5 }))))
        }
        "identity" => matches!(&field.ty, Type::Tuple(t) if t.elems.len() == 3
            && type_name(&t.elems[0]).as_deref() == Some("u64") && type_name(&t.elems[1]).as_deref() == Some("u64")
            && type_name(&t.elems[2]).as_deref() == Some("u32")),
        _ => false,
    }
}

pub(crate) fn readonly_options(
    expr: &Expr,
    locals: &std::collections::HashMap<String, super::Origin>,
) -> bool {
    match expression(expr) {
        Expr::Call(call) => {
            call.args.is_empty()
                && (named_path(&call.func, &["OpenOptions", "new"])
                    || named_path(&call.func, &["fs", "OpenOptions", "new"])
                    || named_path(&call.func, &["std", "fs", "OpenOptions", "new"]))
        }
        Expr::MethodCall(call) if call.args.len() == 1 => match call.method.to_string().as_str() {
            "read" => {
                matches!(&call.args[0], Expr::Lit(l) if matches!(&l.lit, syn::Lit::Bool(b) if b.value))
                    && readonly_options(&call.receiver, locals)
            }
            "custom_flags" => {
                readonly_options(&call.receiver, locals)
                    && (matches!(expression(&call.args[0]), Expr::Path(p)
                if p.path.get_ident().is_some_and(|n| locals.get(&n.to_string()) == Some(&super::Origin::Flags)))
                        || matches!(&call.args[0], Expr::Binary(b) if matches!(b.op, syn::BinOp::BitOr(_))
                    && flag_role(&b.left, locals, super::Origin::NoFollowFlag) && flag_role(&b.right, locals, super::Origin::NonBlockFlag)))
            }
            _ => false,
        },
        _ => false,
    }
}

pub(crate) fn formatter_type(ty: &Type) -> bool {
    matches!(ty, Type::Reference(r) if r.mutability.is_some() && matches!(r.elem.as_ref(), Type::Path(p)
        if p.qself.is_none() && p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>() == ["Formatter"]
        || p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>() == ["fmt", "Formatter"]
        || p.path.segments.iter().map(|s| s.ident.to_string()).collect::<Vec<_>>() == ["std", "fmt", "Formatter"]))
}

pub(crate) fn formatter_macro(
    mac: &syn::Macro,
    locals: &std::collections::HashMap<String, super::Origin>,
) -> bool {
    use syn::parse::Parser;
    let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
    parser.parse2(mac.tokens.clone()).is_ok_and(|args| matches!(args.first(), Some(Expr::Path(p))
        if p.path.get_ident().is_some_and(|n| locals.get(&n.to_string()) == Some(&super::Origin::Formatter))))
}

fn flag_role(
    expr: &Expr,
    locals: &std::collections::HashMap<String, super::Origin>,
    role: super::Origin,
) -> bool {
    matches!(expression(expr), Expr::Path(p) if p.path.get_ident().is_some_and(|n| locals.get(&n.to_string()) == Some(&role)))
}

fn authority_module(name: &str) -> bool {
    matches!(
        name,
        "command_git_private_root.rs"
            | "command_git_private_root_files.rs"
            | "command_git_index_fs.rs"
    )
}

fn private_import(name: &str, ident: &str, prefix: &[String]) -> bool {
    match (name, ident) {
        ("command_git_private_root_files.rs", "open_bound_directory") => prefix == ["super"],
        ("command_git_private_root.rs", "RepositoryContext") => {
            prefix == ["super", "index", "repository"]
        }
        ("command_git_private_root.rs", "CommonRepository") => {
            prefix == ["super", "index", "repository_format"]
        }
        _ => !authority_module(name),
    }
}
