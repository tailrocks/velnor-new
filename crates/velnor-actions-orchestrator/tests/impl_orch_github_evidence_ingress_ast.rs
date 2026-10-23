//! Exact Rust syntax checks for the trusted evidence acquisition edges.

use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind};
use std::path::Path;

use syn::visit::{self, Visit};
use syn::{Item, UseTree};

use crate::impl_orch_f2a_offline::production_source_of_text;

type AuditResult<T> = Result<T, Box<dyn Error>>;

pub(super) fn validate_runtime_edges(orch_root: &Path, cli_root: &Path) -> AuditResult<()> {
    let api = read_source(orch_root, "api.rs")?;
    let api_path = orch_root.join("api.rs");
    let masked_api =
        crate::impl_orch_f2a::mask_api_reexport_for_legacy_scan(&api_path, orch_root, &api)?;
    for marker in [
        "GITHUB_EVIDENCE_FETCH_OP",
        "GITHUB_EVIDENCE_IMPORT_OP",
        "run_github_evidence_fetch",
        "run_github_evidence_import",
    ] {
        if super::source_identifiers(&masked_api)?
            .iter()
            .any(|ident| ident == marker)
        {
            return Err(invalid("API contains a call outside its public re-export"));
        }
    }
    crate::impl_orch_f2a_b::validate_evidence_module_mounts(orch_root)?;
    validate_wrapper(&read_source(orch_root, "github_evidence.rs")?)?;
    validate_cli_dispatch(cli_root)?;
    validate_collector_gate(orch_root)?;
    validate_context_reader(orch_root)?;
    Ok(())
}

pub(super) fn validate_wrapper(source: &str) -> AuditResult<()> {
    let syntax = parsed_production(source)?;
    let functions = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function)
                if matches!(
                    function.sig.ident.to_string().as_str(),
                    "run_github_evidence_fetch" | "run_github_evidence_import"
                ) =>
            {
                Some(function)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let fetch_count = functions
        .iter()
        .filter(|function| call_body_is(function, &["collect", "run"]))
        .count();
    let import_count = functions
        .iter()
        .filter(|function| call_body_is(function, &["import", "run"]))
        .count();
    if functions.len() != 2 || fetch_count != 1 || import_count != 1 {
        return Err(invalid(
            "evidence wrappers must route to matching implementations",
        ));
    }
    Ok(())
}

fn call_body_is(function: &syn::ItemFn, path: &[&str]) -> bool {
    if !matches!(function.vis, syn::Visibility::Public(_))
        || !function.sig.inputs.is_empty()
        || !function.sig.generics.params.is_empty()
        || function.sig.asyncness.is_some()
        || function.sig.constness.is_some()
        || matches!(&function.sig.safety, syn::Safety::Unsafe(_))
        || function.sig.abi.is_some()
        || function.block.stmts.len() != 1
    {
        return false;
    }
    let syn::Stmt::Expr(syn::Expr::Call(call), None) = &function.block.stmts[0] else {
        return false;
    };
    call.args.is_empty() && expression_path_is(&call.func, path)
}

fn validate_cli_dispatch(cli_root: &Path) -> AuditResult<()> {
    let dispatch_source = read_source(cli_root, "dispatch.rs")?;
    let operation_source = read_source(cli_root, "dispatch/operation.rs")?;
    validate_cli_sources(&dispatch_source, &operation_source)
}

pub(super) fn validate_cli_sources(
    dispatch_source: &str,
    operation_source: &str,
) -> AuditResult<()> {
    let dispatch = parsed_production(dispatch_source)?;
    let run_internal = function(&dispatch, "run_internal")?;
    let dispatch_arms = arms_in(run_internal);
    for (operation, function_name) in [
        ("GitHubEvidenceFetch", "run_github_evidence_fetch"),
        ("GitHubEvidenceImport", "run_github_evidence_import"),
    ] {
        if !use_imports_symbol(
            &dispatch,
            &["velnor_actions_orchestrator", function_name],
            function_name,
        ) {
            return Err(invalid("evidence CLI API import changed"));
        }
        let matching = dispatch_arms
            .iter()
            .filter(|arm| pat_path_is(&arm.pat, &["InternalOp", operation]))
            .collect::<Vec<_>>();
        if matching.len() != 1 || !dispatch_arm_is(matching[0], operation, function_name) {
            return Err(invalid("evidence CLI operation arm changed"));
        }
        if super::source_identifiers(dispatch_source)?
            .iter()
            .filter(|ident| ident.as_str() == function_name)
            .count()
            != 2
        {
            return Err(invalid("evidence CLI callable has an extra reference"));
        }
    }
    let try_internal = function(&dispatch, "try_internal")?;
    if !first_local_is_call(try_internal, "request", &["gate_request"], true) {
        return Err(invalid("evidence CLI call moved outside the internal gate"));
    }

    let operation = parsed_production(operation_source)?;
    let from_env = function(&operation, "from_env")?;
    let operation_arms = arms_in(from_env);
    for (constant, variant) in [
        ("GITHUB_EVIDENCE_FETCH_OP", "GitHubEvidenceFetch"),
        ("GITHUB_EVIDENCE_IMPORT_OP", "GitHubEvidenceImport"),
    ] {
        if !use_imports_symbol(&operation, &["super", constant], constant) {
            return Err(invalid("evidence operation constant import changed"));
        }
        let matching = operation_arms
            .iter()
            .filter(|arm| operation_arm_is(arm, constant, variant))
            .count();
        if matching != 1
            || super::source_identifiers(operation_source)?
                .iter()
                .filter(|ident| ident.as_str() == constant)
                .count()
                != 2
        {
            return Err(invalid("evidence operation selection changed"));
        }
    }
    Ok(())
}

fn validate_collector_gate(orch_root: &Path) -> AuditResult<()> {
    let source = read_source(orch_root, "github_evidence_collect.rs")?;
    let syntax = parsed_production(&source)?;
    let run = function(&syntax, "run")?;
    if !first_local_is_call(run, "invocation", &["Invocation", "from_env"], true)
        || !second_local_is_path(run, "reader", &["NativeGitHubEvidenceRead"])
    {
        return Err(invalid(
            "runtime admission must precede native reader construction",
        ));
    }
    Ok(())
}

fn validate_context_reader(orch_root: &Path) -> AuditResult<()> {
    let syntax = parsed_production(&read_source(
        orch_root,
        "github_evidence_collect_context.rs",
    )?)?;
    let from_env = super::invocation_from_env(&syntax)?;
    if from_env.block.stmts.len() != 1 {
        return Err(invalid(
            "production context reader no longer shares validation",
        ));
    }
    let syn::Stmt::Expr(syn::Expr::Call(call), None) = &from_env.block.stmts[0] else {
        return Err(invalid("production context reader changed"));
    };
    let Some(argument) = call.args.first() else {
        return Err(invalid("production context reader lost context values"));
    };
    if call.args.len() != 1
        || !expression_path_is(&call.func, &["Self", "from_context"])
        || !call_is(argument, &["ContextValues", "from_env"], 0)
    {
        return Err(invalid(
            "production context reader bypasses shared validation",
        ));
    }
    Ok(())
}

fn first_local_is_call(function: &syn::ItemFn, name: &str, path: &[&str], fallible: bool) -> bool {
    let Some(syn::Stmt::Local(local)) = function.block.stmts.first() else {
        return false;
    };
    let Some(initializer) = &local.init else {
        return false;
    };
    let expression = if fallible {
        let syn::Expr::Try(try_expression) = &*initializer.expr else {
            return false;
        };
        &*try_expression.expr
    } else {
        &*initializer.expr
    };
    binding_is(&local.pat, name) && call_is(expression, path, 0)
}

fn second_local_is_path(function: &syn::ItemFn, name: &str, path: &[&str]) -> bool {
    let Some(syn::Stmt::Local(local)) = function.block.stmts.get(1) else {
        return false;
    };
    local.init.as_ref().is_some_and(|initializer| {
        binding_is(&local.pat, name) && expression_path_is(&initializer.expr, path)
    })
}

fn binding_is(pattern: &syn::Pat, name: &str) -> bool {
    matches!(pattern, syn::Pat::Ident(binding) if binding.ident == name && binding.subpat.is_none() && binding.mutability.is_none())
}

fn dispatch_arm_is(arm: &syn::Arm, operation: &str, function_name: &str) -> bool {
    if !arm.attrs.is_empty() || !pat_path_is(&arm.pat, &["InternalOp", operation]) {
        return false;
    }
    let syn::Expr::Call(outer) = &*arm.body else {
        return false;
    };
    if !expression_path_is(&outer.func, &["unit_exit"]) || outer.args.len() != 1 {
        return false;
    }
    let Some(inner) = outer.args.first() else {
        return false;
    };
    call_is(inner, &[function_name], 0)
}

fn operation_arm_is(arm: &syn::Arm, constant: &str, variant: &str) -> bool {
    if !arm.attrs.is_empty() {
        return false;
    }
    let syn::Pat::Guard(guarded) = &arm.pat else {
        return false;
    };
    if !ok_tag_pattern(&guarded.pat) {
        return false;
    }
    let syn::Expr::Binary(binary) = &*guarded.guard else {
        return false;
    };
    matches!(binary.op, syn::BinOp::Eq(_))
        && expression_path_is(&binary.left, &["tag"])
        && expression_path_is(&binary.right, &[constant])
        && expression_path_is(&arm.body, &["InternalOp", variant])
}

fn ok_tag_pattern(pattern: &syn::Pat) -> bool {
    let syn::Pat::TupleStruct(tuple) = pattern else {
        return false;
    };
    path_is(&tuple.path, &["Ok"]) && tuple.elems.len() == 1 && binding_is(&tuple.elems[0], "tag")
}

fn use_imports_symbol(syntax: &syn::File, expected_path: &[&str], symbol: &str) -> bool {
    let mut imports = Vec::new();
    for item in &syntax.items {
        if let Item::Use(item) = item {
            collect_imports(&item.tree, &mut Vec::new(), &mut imports);
        }
    }
    let imports = imports
        .iter()
        .filter(|(path, _)| path.last().is_some_and(|name| name == symbol))
        .collect::<Vec<_>>();
    imports.len() == 1
        && imports[0]
            .0
            .iter()
            .map(String::as_str)
            .eq(expected_path.iter().copied())
        && !imports[0].1
}

fn collect_imports(
    tree: &UseTree,
    prefix: &mut Vec<String>,
    imports: &mut Vec<(Vec<String>, bool)>,
) {
    match tree {
        UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            collect_imports(&path.tree, prefix, imports);
            prefix.pop();
        }
        UseTree::Name(name) => imports.push((
            prefix
                .iter()
                .cloned()
                .chain([name.ident.to_string()])
                .collect(),
            false,
        )),
        UseTree::Rename(rename) => imports.push((
            prefix
                .iter()
                .cloned()
                .chain([rename.ident.to_string()])
                .collect(),
            true,
        )),
        UseTree::Group(group) => {
            for item in &group.items {
                collect_imports(item, prefix, imports);
            }
        }
        UseTree::Glob(_) => {}
    }
}

fn arms_in(function: &syn::ItemFn) -> Vec<&syn::Arm> {
    #[derive(Default)]
    struct Arms<'ast>(Vec<&'ast syn::Arm>);
    impl<'ast> Visit<'ast> for Arms<'ast> {
        fn visit_expr_match(&mut self, expression: &'ast syn::ExprMatch) {
            self.0.extend(expression.arms.iter());
            visit::visit_expr_match(self, expression);
        }
    }
    let mut arms = Arms::default();
    arms.visit_block(&function.block);
    arms.0
}

fn function<'a>(syntax: &'a syn::File, name: &str) -> AuditResult<&'a syn::ItemFn> {
    let functions = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == name => Some(function),
            _ => None,
        })
        .collect::<Vec<_>>();
    if functions.len() != 1 {
        return Err(invalid(&format!("expected one {name} function")));
    }
    Ok(functions[0])
}

fn call_is(expression: &syn::Expr, path: &[&str], arguments: usize) -> bool {
    matches!(expression, syn::Expr::Call(call) if expression_path_is(&call.func, path) && call.args.len() == arguments)
}

fn expression_path_is(expression: &syn::Expr, expected: &[&str]) -> bool {
    matches!(expression, syn::Expr::Path(path) if path.qself.is_none() && path_is(&path.path, expected))
}

fn pat_path_is(pattern: &syn::Pat, expected: &[&str]) -> bool {
    matches!(pattern, syn::Pat::Path(path) if path.qself.is_none() && path_is(&path.path, expected))
}

fn path_is(path: &syn::Path, expected: &[&str]) -> bool {
    path.leading_colon.is_none()
        && path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| {
                segment.ident == *expected && matches!(segment.arguments, syn::PathArguments::None)
            })
}

fn parsed_production(source: &str) -> AuditResult<syn::File> {
    syn::parse_file(&production_source_of_text(source)?).map_err(Into::into)
}

fn read_source(root: &Path, relative: &str) -> AuditResult<String> {
    fs::read_to_string(root.join(relative)).map_err(Into::into)
}

fn invalid(message: &str) -> Box<dyn Error> {
    IoError::new(ErrorKind::InvalidData, message.to_owned()).into()
}
