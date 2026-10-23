//! Exact AST/dataflow recognition for the passive resolved evidence value.

use proc_macro2::Span;
use syn::{Expr, Item, Pat, Stmt};

use super::projection;
use super::source;

pub(super) fn passive_spec_identifiers(
    syntax: &syn::File,
    source_text: &str,
) -> Result<Vec<(Span, &'static str)>, syn::Error> {
    let function = canonical_emitter(syntax)?;
    let loop_expr = canonical_suite_loop(function)?;
    let (evidence_index, evidence_if) = evidence_binding(loop_expr)?;
    validate_evidence_condition(evidence_if)?;
    let (fetch_binding, fetch_value) = validate_evidence_branches(evidence_if)?;
    let (push_index, policy) = canonical_policy_push(loop_expr)?;
    if evidence_index >= push_index {
        return Err(source::invalid(
            "evidence data does not precede policy construction",
        ));
    }
    let field = projection::validate_policy_fields(policy)?;
    let counts = source::identifier_counts(source_text)?;
    if counts.get("fetch_spec").copied() != Some(2)
        || counts.get("evidence_fetch_spec").copied() != Some(1)
    {
        return Err(source::invalid(
            "passive evidence identifiers have extra or missing uses",
        ));
    }
    Ok(vec![
        (fetch_binding, "fetch_spec"),
        (fetch_value, "fetch_spec"),
        (field, "evidence_fetch_spec"),
    ])
}

fn canonical_emitter(syntax: &syn::File) -> Result<&syn::ItemFn, syn::Error> {
    let functions = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fn(function) if function.sig.ident == "suite_workflow_files" => Some(function),
            _ => None,
        })
        .collect::<Vec<_>>();
    if functions.len() != 1 {
        return Err(source::invalid(
            "expected one canonical suite_workflow_files function",
        ));
    }
    validate_function_signature(functions[0])?;
    Ok(functions[0])
}

fn canonical_suite_loop(function: &syn::ItemFn) -> Result<&syn::ExprForLoop, syn::Error> {
    let loops = function
        .block
        .stmts
        .iter()
        .filter_map(|statement| match statement {
            Stmt::Expr(Expr::ForLoop(loop_expr), None) => Some(loop_expr),
            _ => None,
        })
        .collect::<Vec<_>>();
    if loops.len() != 1
        || !loops[0].attrs.is_empty()
        || loops[0].label.is_some()
        || !source::plain_ident(&loops[0].pat, "suite")
        || !source::immutable_ref_field_chain(
            &loops[0].expr,
            "prep",
            &["config", "workflow", "task_suites"],
        )
    {
        return Err(source::invalid("task-suite loop input changed"));
    }
    Ok(loops[0])
}

fn evidence_binding(loop_expr: &syn::ExprForLoop) -> Result<(usize, &syn::ExprIf), syn::Error> {
    let matches = loop_expr
        .body
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(index, statement)| match statement {
            Stmt::Local(local) if source::plain_ident(&local.pat, "evidence") => {
                Some((index, local))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(source::invalid("expected one evidence data binding"));
    }
    let (index, local) = matches[0];
    match source::local_initializer(local)? {
        Expr::If(if_expr) if if_expr.attrs.is_empty() => Ok((index, if_expr)),
        _ => Err(source::invalid(
            "evidence binding is not the canonical if-let",
        )),
    }
}

fn canonical_policy_push(
    loop_expr: &syn::ExprForLoop,
) -> Result<(usize, &syn::ExprStruct), syn::Error> {
    let pushes = loop_expr
        .body
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(index, statement)| {
            projection::suite_policy_push(statement).map(|value| (index, value))
        })
        .collect::<Vec<_>>();
    if pushes.len() != 1 {
        return Err(source::invalid(
            "expected one direct MiseTaskSuitePolicy push",
        ));
    }
    Ok(pushes[0])
}

fn validate_function_signature(function: &syn::ItemFn) -> Result<(), syn::Error> {
    let signature = &function.sig;
    if !canonical_function_attributes(&function.attrs)
        || !matches!(&function.vis, syn::Visibility::Restricted(vis) if vis.path.is_ident("crate"))
        || function.modifiers.defaultness.is_some()
        || !signature.generics.params.is_empty()
        || signature.constness.is_some()
        || signature.asyncness.is_some()
        || !matches!(&signature.safety, syn::Safety::Default)
        || signature.abi.is_some()
        || signature.inputs.len() != 1
    {
        return Err(source::invalid("suite_workflow_files signature changed"));
    }
    let syn::FnArg::Typed(argument) = &signature.inputs[0] else {
        return Err(source::invalid("suite_workflow_files parameter changed"));
    };
    if !argument.attrs.is_empty() || !source::plain_ident(&argument.pat, "prep") {
        return Err(source::invalid(
            "suite_workflow_files parameter pattern changed",
        ));
    }
    let syn::Type::Reference(reference) = &*argument.ty else {
        return Err(source::invalid(
            "suite_workflow_files parameter type changed",
        ));
    };
    let syn::Type::Path(type_path) = &*reference.elem else {
        return Err(source::invalid(
            "suite_workflow_files parameter type changed",
        ));
    };
    if !reference.attrs.is_empty()
        || reference.lifetime.is_some()
        || reference.mutability.is_some()
        || type_path.qself.is_some()
        || !source::path_is(&type_path.path, &["GenerationPreparation"])
    {
        return Err(source::invalid(
            "suite_workflow_files parameter type changed",
        ));
    }
    Ok(())
}

const CANONICAL_FUNCTION_DOC: &str = " Resolve and render standalone task-suite workflow files.";

fn canonical_function_attributes(attributes: &[syn::Attribute]) -> bool {
    match attributes {
        [] => true,
        [attribute] => canonical_function_doc(attribute),
        _ => false,
    }
}

fn canonical_function_doc(attribute: &syn::Attribute) -> bool {
    if !matches!(&attribute.style, syn::AttrStyle::Outer) {
        return false;
    }
    let syn::Meta::NameValue(name_value) = &attribute.meta else {
        return false;
    };
    let path = &name_value.path;
    if path.leading_colon.is_some() || path.segments.len() != 1 || path.segments.trailing_punct() {
        return false;
    }
    let Some(segment) = path.segments.first() else {
        return false;
    };
    if segment.ident != "doc" || !matches!(&segment.arguments, syn::PathArguments::None) {
        return false;
    }
    let Expr::Lit(expression) = &name_value.value else {
        return false;
    };
    if !expression.attrs.is_empty() {
        return false;
    }
    matches!(&expression.lit, syn::Lit::Str(literal)
        if literal.suffix().is_empty() && literal.value() == CANONICAL_FUNCTION_DOC)
}

fn validate_evidence_condition(if_expr: &syn::ExprIf) -> Result<(), syn::Error> {
    let Expr::Let(condition) = &*if_expr.cond else {
        return Err(source::invalid("evidence condition is not an if-let"));
    };
    let Pat::TupleStruct(pattern) = condition.pat.as_ref() else {
        return Err(source::invalid("evidence option pattern changed"));
    };
    if !condition.attrs.is_empty()
        || !pattern.attrs.is_empty()
        || pattern.qself.is_some()
        || !source::path_is(&pattern.path, &["Some"])
        || pattern.elems.len() != 1
        || !source::plain_ident(&pattern.elems[0], "input")
        || !source::immutable_ref_field_chain(&condition.expr, "suite", &["evidence_snapshot"])
    {
        return Err(source::invalid("evidence option input changed"));
    }
    Ok(())
}

fn validate_evidence_branches(if_expr: &syn::ExprIf) -> Result<(Span, Span), syn::Error> {
    let statements = &if_expr.then_branch.stmts;
    if statements.len() != 3 {
        return Err(source::invalid("evidence branch statements changed"));
    }
    let Stmt::Local(fetch_local) = &statements[0] else {
        return Err(source::invalid("resolved spec is not a local binding"));
    };
    let fetch_binding = source::plain_ident_span(&fetch_local.pat, "fetch_spec")
        .ok_or_else(|| source::invalid("resolved spec binding changed"))?;
    validate_resolve_initializer(source::local_initializer(fetch_local)?)?;

    let Stmt::Local(helper_local) = &statements[1] else {
        return Err(source::invalid("immutable helper is not a local binding"));
    };
    if !source::plain_ident(&helper_local.pat, "helper") {
        return Err(source::invalid("immutable helper binding changed"));
    }
    let _ = source::local_initializer(helper_local)?;

    let Stmt::Expr(tail, None) = &statements[2] else {
        return Err(source::invalid("evidence tuple is not the branch result"));
    };
    let fetch_value = validate_some_tuple(tail)?;

    let Some((_, else_expr)) = &if_expr.else_branch else {
        return Err(source::invalid("evidence None branch is missing"));
    };
    validate_none_branch(else_expr)?;
    Ok((fetch_binding, fetch_value))
}

fn validate_resolve_initializer(expression: &Expr) -> Result<(), syn::Error> {
    let Expr::Try(try_expr) = expression else {
        return Err(source::invalid("resolved spec must use the checked result"));
    };
    let Expr::MethodCall(call) = &*try_expr.expr else {
        return Err(source::invalid("resolved spec method changed"));
    };
    let arguments = call.args.iter().collect::<Vec<_>>();
    if !try_expr.attrs.is_empty()
        || !call.attrs.is_empty()
        || call.method != "resolve_spec"
        || call.turbofish.is_some()
        || !source::simple_expr_path(&call.receiver, &["input"])
        || arguments.len() != 3
        || !source::immutable_ref_field_chain(
            arguments[0],
            "prep",
            &["config", "workflow", "task_suites"],
        )
        || !source::immutable_ref_field_chain(arguments[1], "suite", &["id"])
        || !source::immutable_ref_field_chain(arguments[2], "prep", &["default_branch"])
    {
        return Err(source::invalid("resolved spec input expression changed"));
    }
    Ok(())
}

fn validate_some_tuple(expression: &Expr) -> Result<Span, syn::Error> {
    let Expr::Call(call) = expression else {
        return Err(source::invalid("evidence Some constructor changed"));
    };
    let Some(argument) = call.args.first() else {
        return Err(source::invalid("evidence Some tuple is missing"));
    };
    let Expr::Tuple(tuple) = argument else {
        return Err(source::invalid("evidence Some value is not a tuple"));
    };
    let tuple_items = tuple.elems.iter().collect::<Vec<_>>();
    let Some(spec_value) = tuple_items.first().copied() else {
        return Err(source::invalid("evidence Some tuple is missing its spec"));
    };
    let Some(helper_value) = tuple_items.get(1).copied() else {
        return Err(source::invalid("evidence Some tuple is missing its helper"));
    };
    if !call.attrs.is_empty()
        || !source::simple_expr_path(&call.func, &["Some"])
        || call.args.len() != 1
        || !tuple.attrs.is_empty()
        || tuple_items.len() != 2
        || !source::simple_expr_path(helper_value, &["helper"])
    {
        return Err(source::invalid("evidence tuple shape changed"));
    }
    let Expr::Path(path) = spec_value else {
        return Err(source::invalid(
            "resolved spec is not the first tuple value",
        ));
    };
    if !source::simple_expr_path(spec_value, &["fetch_spec"]) {
        return Err(source::invalid(
            "resolved spec is not the first tuple value",
        ));
    }
    Ok(path.path.segments[0].ident.span())
}

fn validate_none_branch(expression: &Expr) -> Result<(), syn::Error> {
    let Expr::Block(block) = expression else {
        return Err(source::invalid("evidence None branch is not a block"));
    };
    if !block.attrs.is_empty() || block.label.is_some() || block.block.stmts.len() != 1 {
        return Err(source::invalid("evidence None branch changed"));
    }
    let Stmt::Expr(value, None) = &block.block.stmts[0] else {
        return Err(source::invalid("evidence None branch result changed"));
    };
    if !source::simple_expr_path(value, &["None"]) {
        return Err(source::invalid("evidence None value changed"));
    }
    Ok(())
}
