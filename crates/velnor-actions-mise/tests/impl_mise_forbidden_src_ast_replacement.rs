//! Existing-file writes gain authority only after held/named descriptor guards.
use super::shapes::{Origin, expression, named_path};
use std::collections::HashMap;
use syn::{Expr, Stmt};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Atom {
    Regular,
    SingleLink,
    PrivateMode,
    HeldNamed,
    NamedHeld,
    Unknown,
}

pub(crate) fn admit_guard(stmt: &Stmt, locals: &mut HashMap<String, Origin>) {
    let Stmt::Expr(Expr::If(guard), _) = stmt else {
        return;
    };
    if guard.else_branch.is_some() || !failure_return(&guard.then_branch) {
        return;
    }
    let mut atoms = Vec::new();
    collect_atoms(&guard.cond, locals, &mut atoms);
    let held = atoms.len() == 4
        && [
            Atom::Regular,
            Atom::SingleLink,
            Atom::PrivateMode,
            Atom::HeldNamed,
        ]
        .iter()
        .all(|atom| atoms.contains(atom));
    let named = atoms == [Atom::NamedHeld];
    if let Some(handle) = locals.get_mut("handle") {
        if held && *handle == Origin::PendingFileHandle {
            *handle = Origin::HeldCheckedHandle;
        }
        if named && *handle == Origin::HeldCheckedHandle {
            *handle = Origin::ExistingFileHandle;
        }
    }
}

fn failure_return(block: &syn::Block) -> bool {
    block.stmts.len() == 1
        && matches!(&block.stmts[0], Stmt::Expr(Expr::Return(r), _)
        if r.expr.as_ref().is_some_and(|e| matches!(e.as_ref(), Expr::Call(c) if named_path(&c.func, &["Err"])
            && c.args.len() == 1 && matches!(&c.args[0], Expr::Call(error) if named_path(&error.func, &["io", "Error", "other"])
                && error.args.len() == 1 && matches!(&error.args[0], Expr::Lit(l) if matches!(&l.lit, syn::Lit::Str(_)))))))
}

fn collect_atoms(expr: &Expr, locals: &HashMap<String, Origin>, atoms: &mut Vec<Atom>) {
    if let Expr::Binary(binary) = expression(expr) {
        if matches!(binary.op, syn::BinOp::Or(_)) {
            collect_atoms(&binary.left, locals, atoms);
            collect_atoms(&binary.right, locals, atoms);
            return;
        }
    }
    atoms.push(atom(expression(expr), locals));
}

fn atom(expr: &Expr, locals: &HashMap<String, Origin>) -> Atom {
    if matches!(expr, Expr::Unary(u) if matches!(u.op, syn::UnOp::Not(_)) && matches!(u.expr.as_ref(), Expr::MethodCall(c)
        if c.method == "is_file" && c.args.is_empty() && metadata_method(&c.receiver, "file_type", Origin::HeldMetadata, locals)))
    {
        return Atom::Regular;
    }
    let Expr::Binary(binary) = expr else {
        return Atom::Unknown;
    };
    if !matches!(binary.op, syn::BinOp::Ne(_)) {
        return Atom::Unknown;
    }
    if metadata_method(&binary.left, "nlink", Origin::HeldMetadata, locals)
        && integer(&binary.right, 1)
    {
        return Atom::SingleLink;
    }
    if matches!(binary.left.as_ref(), Expr::Binary(mode) if matches!(mode.op, syn::BinOp::BitAnd(_))
        && metadata_method(&mode.left, "mode", Origin::HeldMetadata, locals) && integer(&mode.right, 0o7777))
        && integer(&binary.right, 0o600)
    {
        return Atom::PrivateMode;
    }
    if identity(&binary.left, Origin::HeldMetadata, locals)
        && identity(&binary.right, Origin::BeforeMetadata, locals)
    {
        return Atom::HeldNamed;
    }
    if identity(&binary.left, Origin::NamedMetadata, locals)
        && identity(&binary.right, Origin::HeldMetadata, locals)
    {
        return Atom::NamedHeld;
    }
    Atom::Unknown
}

fn metadata_method(
    expr: &Expr,
    method: &str,
    origin: Origin,
    locals: &HashMap<String, Origin>,
) -> bool {
    matches!(expression(expr), Expr::MethodCall(c) if c.method == method && c.args.is_empty()
        && matches!(expression(&c.receiver), Expr::Path(p) if p.path.get_ident().is_some_and(|n| locals.get(&n.to_string()) == Some(&origin))))
}
fn identity(expr: &Expr, origin: Origin, locals: &HashMap<String, Origin>) -> bool {
    matches!(expression(expr), Expr::Tuple(t) if t.elems.len() == 2
        && metadata_method(&t.elems[0], "dev", origin, locals) && metadata_method(&t.elems[1], "ino", origin, locals))
}
fn integer(expr: &Expr, value: u64) -> bool {
    matches!(expression(expr), Expr::Lit(l) if matches!(&l.lit, syn::Lit::Int(i) if i.base10_parse::<u64>().ok() == Some(value)))
}
