use syn::{Expr, ExprCall, Path};

const EXECUTABLE_LOOKUP_TESTS: &str = "requests_executable_lookup_tests.rs";

pub(super) fn exact_read_only_rustix_call(name: &str, path: &[String], call: &ExprCall) -> bool {
    if name == "check_file_read.rs" {
        return canonical_is(path, &["rustix", "fs", "open"])
            && call.args.len() == 3
            && expr_path_is(&call.args[0], &["path"])
            && exact_flags(&call.args[1], &["RDONLY", "NOFOLLOW", "CLOEXEC"], false)
            && empty_mode_call(&call.args[2], false);
    }
    if name != "requests_executable_lookup.rs" {
        return false;
    }
    if canonical_is(path, &["rustix", "fs", "open"]) {
        return call.args.len() == 3
            && empty_mode_call(&call.args[2], true)
            && expr_string_is(&call.args[0], "/")
            && exact_flags(
                &call.args[1],
                &["RDONLY", "DIRECTORY", "NOFOLLOW", "CLOEXEC"],
                true,
            );
    }
    canonical_is(path, &["rustix", "fs", "openat"])
        && (exact_openat_with_flags(call, &["RDONLY", "DIRECTORY", "NOFOLLOW", "CLOEXEC"])
            || exact_openat_with_flags(call, &["RDONLY", "NOFOLLOW", "NONBLOCK", "CLOEXEC"]))
}

pub(super) fn exact_openat_with_flags(call: &ExprCall, flags: &[&str]) -> bool {
    call.args.len() == 4
        && is_descriptor_reference(&call.args[0])
        && is_path_component_reference(&call.args[1])
        && exact_flags(&call.args[2], flags, true)
        && empty_mode_call(&call.args[3], true)
}

pub(super) fn canonical_is(path: &[String], expected: &[&str]) -> bool {
    path.len() == expected.len()
        && path
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

pub(super) fn is_test_fixture_mutation(name: &str, canonical: &[String]) -> bool {
    name == EXECUTABLE_LOOKUP_TESTS
        && matches!(
            canonical.last().map(String::as_str),
            Some("create_dir_all" | "write" | "set_permissions" | "symlink")
        )
}

fn exact_flags(expression: &Expr, expected: &[&str], allow_short: bool) -> bool {
    let mut flags = Vec::new();
    collect_flag_paths(expression, &mut flags, allow_short);
    flags
        == expected
            .iter()
            .map(|flag| {
                vec!["rustix", "fs", "OFlags", *flag]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            })
            .collect::<Vec<Vec<String>>>()
}

fn collect_flag_paths(expression: &Expr, flags: &mut Vec<Vec<String>>, allow_short: bool) {
    match expression {
        Expr::Binary(binary) if matches!(binary.op, syn::BinOp::BitOr(_)) => {
            collect_flag_paths(&binary.left, flags, allow_short);
            collect_flag_paths(&binary.right, flags, allow_short);
        }
        Expr::Path(path) if path.qself.is_none() => {
            let name = path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string());
            if name
                .as_deref()
                .is_some_and(|flag| FLAG_NAMES.contains(&flag))
                && flag_prefix_is_allowed(&path.path, allow_short)
            {
                flags.push(
                    vec![
                        "rustix",
                        "fs",
                        "OFlags",
                        name.as_deref().unwrap_or_default(),
                    ]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                );
            } else {
                flags.push(vec!["invalid".to_owned()]);
            }
        }
        _ => flags.push(vec!["invalid".to_owned()]),
    }
}

const FLAG_NAMES: &[&str] = &["RDONLY", "NOFOLLOW", "CLOEXEC", "DIRECTORY", "NONBLOCK"];

fn flag_prefix_is_allowed(path: &Path, allow_short: bool) -> bool {
    let qualified = path.segments.len() == 4
        && path.segments[0].ident == "rustix"
        && path.segments[1].ident == "fs"
        && path.segments[2].ident == "OFlags";
    let short = allow_short && path.segments.len() == 2 && path.segments[0].ident == "OFlags";
    qualified || short
}

fn expr_string_is(expression: &Expr, expected: &str) -> bool {
    matches!(expression, Expr::Lit(literal)
        if matches!(&literal.lit, syn::Lit::Str(value) if value.value() == expected))
}

fn is_descriptor_reference(expression: &Expr) -> bool {
    matches!(expression, Expr::Reference(reference)
        if matches!(&*reference.expr, Expr::Path(path)
            if path.qself.is_none() && path.path.segments.len() == 1
                && matches!(path.path.segments[0].ident.to_string().as_str(), "current" | "directory")))
}

fn is_path_component_reference(expression: &Expr) -> bool {
    matches!(expression, Expr::Path(path)
        if path.qself.is_none() && path.path.segments.len() == 1
            && path.path.segments[0].ident == "name"
            && matches!(path.path.segments[0].arguments, syn::PathArguments::None))
        || matches!(expression, Expr::Unary(unary)
            if matches!(unary.op, syn::UnOp::Deref(_))
                && matches!(&*unary.expr, Expr::Path(path)
                    if path.qself.is_none() && path.path.segments.len() == 1
                        && path.path.segments[0].ident == "name"))
}

fn empty_mode_call(expression: &Expr, allow_short: bool) -> bool {
    let Expr::Call(call) = expression else {
        return false;
    };
    call.args.is_empty()
        && matches!(&*call.func, Expr::Path(path)
            if path.qself.is_none()
                && (path_is(&path.path, &["rustix", "fs", "Mode", "empty"])
                    || (allow_short && path_is(&path.path, &["Mode", "empty"]))))
}

fn expr_path_is(expression: &Expr, expected: &[&str]) -> bool {
    matches!(expression, Expr::Path(path)
        if path.qself.is_none() && path_is(&path.path, expected))
}

fn path_is(path: &Path, expected: &[&str]) -> bool {
    path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected)
}
