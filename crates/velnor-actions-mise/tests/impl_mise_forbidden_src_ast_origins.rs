//! Parsed destination and handle provenance.
use super::{Origin, expression, named_path};
use std::collections::HashMap;
use syn::{Expr, Pat};

pub(crate) fn origin(
    expr: &Expr,
    locals: &HashMap<String, Origin>,
    function: &str,
    trusted: bool,
) -> Origin {
    let expr = expression(expr);
    if let Expr::Binary(binary) = expr {
        if matches!(binary.op, syn::BinOp::BitOr(_))
            && ((integer(&binary.left, 0x20000) && integer(&binary.right, 0x800))
                || (integer(&binary.left, 0x100) && integer(&binary.right, 0x4)))
        {
            return Origin::Flags;
        }
    }
    if !trusted {
        return Origin::Unknown;
    }
    if let Expr::Path(path) = expr {
        return path
            .path
            .get_ident()
            .and_then(|n| locals.get(&n.to_string()).copied())
            .unwrap_or(Origin::Unknown);
    }
    if let Expr::Call(call) = expr {
        return call_origin(call, locals, function, trusted);
    }
    if let Expr::MethodCall(call) = expr {
        return method_origin(call, locals, function, trusted);
    }
    if let Expr::Macro(mac) = expr {
        if function == "create_private_directory" && allocation_name(mac, locals) {
            return Origin::TempName;
        }
    }
    Origin::Unknown
}

pub(crate) fn method_origin(
    call: &syn::ExprMethodCall,
    locals: &HashMap<String, Origin>,
    function: &str,
    trusted: bool,
) -> Origin {
    if !trusted {
        return Origin::Unknown;
    }
    if function == "create" {
        if let Some(value) = slot_array(call, locals) {
            return value;
        }
    }
    let receiver = origin(&call.receiver, locals, function, trusted);
    if function == "replace_contents"
        && call.method == "metadata"
        && call.args.is_empty()
        && named_path(&call.receiver, &["handle"])
        && matches!(
            receiver,
            Origin::PendingFileHandle | Origin::HeldCheckedHandle | Origin::ExistingFileHandle
        )
    {
        return Origin::HeldMetadata;
    }
    if let Some(origin) = allocator_method(call, function, receiver) {
        return origin;
    }
    if named_path(&call.receiver, &["self"]) && call.args.len() == 1 {
        if call.method == "slot_path" {
            if named_path(&call.args[0], &["slot"])
                && matches!(function, "write_file" | "replace_contents")
            {
                return Origin::FileSlot;
            }
            if named_path(&call.args[0], &["OwnedFile", "EffectiveConfig"]) {
                return Origin::EffectiveConfig;
            }
            if named_path(&call.args[0], &["OwnedFile", "BootstrapConfig"]) {
                return Origin::BootstrapConfig;
            }
        }
        if call.method == "directory_path"
            && named_path(&call.args[0], &["slot"])
            && function == "create_directory"
        {
            return Origin::DirectorySlot;
        }
    }
    if let Some(origin) = options_origin(call, receiver, locals, function, trusted) {
        return origin;
    }
    if receiver == Origin::DirectoryBuilderNew
        && call.method == "mode"
        && call.args.len() == 1
        && integer(&call.args[0], 0o700)
    {
        return Origin::DirectoryBuilder;
    }
    if receiver == Origin::TempBase
        && call.method == "join"
        && call.args.len() == 1
        && origin(&call.args[0], locals, function, trusted) == Origin::TempName
    {
        return Origin::AllocDirectory;
    }
    Origin::Unknown
}

fn integer(expr: &Expr, expected: u32) -> bool {
    matches!(expr, Expr::Lit(l) if matches!(&l.lit, syn::Lit::Int(i) if i.base10_parse::<u32>().ok() == Some(expected)))
}

pub(crate) fn owned_constructor(expr: &syn::ExprStruct, locals: &HashMap<String, Origin>) -> bool {
    if expr.rest.is_some() {
        return false;
    }
    [
        ("path", Origin::AllocRoot),
        ("handle", Origin::RootHandle),
        ("directories", Origin::Directories),
        ("files", Origin::Files),
    ]
    .iter()
    .all(|(name, expected)| {
        expr.fields.iter().any(|f| {
            matches!(&f.member, syn::Member::Named(n) if n == name)
                && origin(&f.expr, locals, "create", true) == *expected
        })
    })
}

fn slot_array(call: &syn::ExprMethodCall, locals: &HashMap<String, Origin>) -> Option<Origin> {
    if call.method != "map" || call.args.len() != 1 {
        return None;
    }
    let names = origin(&call.receiver, locals, "create", true);
    let Expr::Closure(closure) = &call.args[0] else {
        return None;
    };
    if closure.inputs.len() != 1
        || !matches!(&closure.inputs[0], Pat::Ident(p) if p.ident == "name")
    {
        return None;
    }
    let Expr::MethodCall(join) = closure.body.as_ref() else {
        return None;
    };
    if join.method != "join"
        || join.args.len() != 1
        || !named_path(&join.args[0], &["name"])
        || origin(&join.receiver, locals, "create", true) != Origin::AllocRoot
    {
        return None;
    }
    match names {
        Origin::DirectoryNames => Some(Origin::Directories),
        Origin::FileNames => Some(Origin::Files),
        _ => None,
    }
}

pub(crate) fn slot_symbols(file: &syn::File) -> Option<HashMap<String, Origin>> {
    let mut result = HashMap::new();
    for (name, values, origin) in [
        (
            "DIRECTORY_NAMES",
            &[
                "gitdir",
                "gitdir/info",
                "gitdir/refs",
                "gitdir/refs/heads",
                "gitdir/objects",
            ][..],
            Origin::DirectoryNames,
        ),
        (
            "FILE_NAMES",
            &[
                "index",
                "gitdir/HEAD",
                "gitdir/config",
                "gitdir/effective.config",
                "gitdir/config.fragment",
                "gitdir/info/attributes",
                "gitdir/info/exclude",
            ][..],
            Origin::FileNames,
        ),
    ] {
        let mut declarations = file.items.iter().filter_map(|item| match item {
            syn::Item::Const(c) if c.ident == name => Some(c),
            _ => None,
        });
        let c = declarations.next()?;
        if declarations.next().is_some()
            || !c.attrs.is_empty()
            || !matches!(c.vis, syn::Visibility::Inherited)
        {
            return None;
        }
        let syn::Type::Array(ty) = c.ty.as_ref() else {
            return None;
        };
        let syn::Type::Reference(element) = ty.elem.as_ref() else {
            return None;
        };
        if element.mutability.is_some()
            || element.lifetime.is_some()
            || !matches!(element.elem.as_ref(), syn::Type::Path(p)
                if p.qself.is_none() && p.path.is_ident("str"))
            || !integer(&ty.len, values.len() as u32)
        {
            return None;
        }
        let Expr::Array(array) = c.expr.as_ref() else {
            return None;
        };
        if array.elems.len() != values.len() || !array.elems.iter().zip(values).all(|(expr,value)|
            matches!(expr, Expr::Lit(l) if matches!(&l.lit, syn::Lit::Str(s) if s.value() == *value)))
        { return None; }
        result.insert(name.to_owned(), origin);
    }
    Some(result)
}

fn allocation_name(mac: &syn::ExprMacro, locals: &HashMap<String, Origin>) -> bool {
    use syn::parse::Parser;
    if !mac.mac.path.is_ident("format") {
        return false;
    }
    let parser = syn::punctuated::Punctuated::<Expr, syn::Token![,]>::parse_terminated;
    let Ok(args) = parser.parse2(mac.mac.tokens.clone()) else {
        return false;
    };
    locals.get("time") == Some(&Origin::Time)
        && locals.get("counter") == Some(&Origin::Counter)
        && args.len() == 5
        && matches!(&args[0], Expr::Lit(l) if matches!(&l.lit, syn::Lit::Str(s)
        if s.value() == "velnor-git-index-{}-{}-{}-{}"))
        && matches!(&args[1], Expr::Call(c) if named_path(&c.func, &["std", "process", "id"]) && c.args.is_empty())
        && matches!(&args[2], Expr::MethodCall(c) if named_path(&c.receiver, &["time"]) && c.method == "as_secs" && c.args.is_empty())
        && matches!(&args[3], Expr::MethodCall(c) if named_path(&c.receiver, &["time"]) && c.method == "subsec_nanos" && c.args.is_empty())
        && named_path(&args[4], &["counter"])
}

fn allocator_method(
    call: &syn::ExprMethodCall,
    function: &str,
    receiver: Origin,
) -> Option<Origin> {
    if function == "create_private_directory" {
        if named_path(&call.receiver, &["DIRECTORY_COUNTER"])
            && call.method == "fetch_add"
            && call.args.len() == 2
            && integer(&call.args[0], 1)
            && named_path(&call.args[1], &["Ordering", "Relaxed"])
        {
            return Some(Origin::Counter);
        }
        if call.method == "duration_since"
            && call.args.len() == 1
            && named_path(&call.args[0], &["UNIX_EPOCH"])
            && matches!(call.receiver.as_ref(), Expr::Call(c) if named_path(&c.func, &["SystemTime", "now"]) && c.args.is_empty())
        {
            return Some(Origin::Time);
        }
        if call.method == "map_err" && call.args.len() == 1 && receiver == Origin::Time {
            return Some(receiver);
        }
    }
    None
}

fn call_origin(
    call: &syn::ExprCall,
    locals: &HashMap<String, Origin>,
    function: &str,
    trusted: bool,
) -> Origin {
    if function == "replace_contents"
        && named_path(&call.func, &["fs", "symlink_metadata"])
        && call.args.len() == 1
        && named_path(&call.args[0], &["path"])
        && locals.get("path") == Some(&Origin::FileSlot)
    {
        return match locals.get("handle") {
            None => Origin::BeforeMetadata,
            Some(Origin::HeldCheckedHandle | Origin::ExistingFileHandle) => Origin::NamedMetadata,
            _ => Origin::Unknown,
        };
    }
    if named_path(&call.func, &["fs", "OpenOptions", "new"]) && call.args.is_empty() {
        return Origin::OptionsNew;
    }
    if (named_path(&call.func, &["fs", "DirBuilder", "new"])
        || named_path(&call.func, &["DirBuilder", "new"]))
        && call.args.is_empty()
    {
        return if function == "create_private_directory" {
            Origin::AllocBuilderNew
        } else {
            Origin::DirectoryBuilderNew
        };
    }
    if function == "create_private_directory"
        && named_path(&call.func, &["absolute_temp_directory"])
        && call.args.len() == 1
        && named_path(&call.args[0], &["excluded"])
    {
        return Origin::TempBase;
    }
    if function == "create"
        && named_path(
            &call.func,
            &["super", "index", "fs", "create_private_directory"],
        )
        && call.args.len() == 1
        && named_path(&call.args[0], &["excluded"])
    {
        return Origin::AllocRoot;
    }
    if function == "create"
        && named_path(&call.func, &["open_bound_directory"])
        && call.args.len() == 1
        && origin(&call.args[0], locals, function, trusted) == Origin::AllocRoot
    {
        return Origin::RootHandle;
    }
    Origin::Unknown
}

fn options_origin(
    call: &syn::ExprMethodCall,
    receiver: Origin,
    locals: &HashMap<String, Origin>,
    function: &str,
    trusted: bool,
) -> Option<Origin> {
    if call.args.len() == 1 {
        let yes = matches!(call.args.first(), Some(Expr::Lit(l)) if matches!(&l.lit, syn::Lit::Bool(b) if b.value));
        if receiver == Origin::OptionsNew && call.method == "write" && yes {
            return Some(Origin::WriteEnabled);
        }
        if receiver == Origin::WriteEnabled && call.method == "create_new" && yes {
            return Some(Origin::ExclusiveEnabled);
        }
        if receiver == Origin::WriteEnabled
            && function == "replace_contents"
            && call.method == "custom_flags"
            && origin(&call.args[0], locals, function, trusted) == Origin::Flags
        {
            return Some(Origin::ReplaceOptions);
        }
        if receiver == Origin::ReplaceOptions
            && function == "replace_contents"
            && call.method == "open"
            && named_path(&call.args[0], &["path"])
            && locals.get("path") == Some(&Origin::FileSlot)
        {
            return Some(Origin::PendingFileHandle);
        }
        if receiver == Origin::ExclusiveEnabled
            && call.method == "custom_flags"
            && origin(&call.args[0], locals, function, trusted) == Origin::Flags
        {
            return Some(Origin::FlagsEnabled);
        }
        if receiver == Origin::FlagsEnabled
            && call.method == "mode"
            && integer(&call.args[0], 0o600)
        {
            return Some(Origin::WriteOptions);
        }
        if receiver == Origin::WriteOptions
            && call.method == "open"
            && origin(&call.args[0], locals, function, trusted) == Origin::FileSlot
        {
            return Some(Origin::OwnedFileHandle);
        }
        if receiver == Origin::AllocBuilderNew
            && call.method == "mode"
            && integer(&call.args[0], 0o700)
        {
            return Some(Origin::AllocBuilder);
        }
    }
    None
}
