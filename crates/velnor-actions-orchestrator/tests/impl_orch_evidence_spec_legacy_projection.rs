//! Closed policy-field projection checks for the legacy scanner adapter.

use proc_macro2::Span;
use syn::{Expr, Stmt};

use super::source;

pub(super) fn suite_policy_push(statement: &Stmt) -> Option<&syn::ExprStruct> {
    let Stmt::Expr(Expr::MethodCall(call), Some(_)) = statement else {
        return None;
    };
    if !call.attrs.is_empty()
        || call.method != "push"
        || call.turbofish.is_some()
        || !source::simple_expr_path(&call.receiver, &["suites"])
        || call.args.len() != 1
    {
        return None;
    }
    match call.args.first()? {
        Expr::Struct(policy)
            if policy.qself.is_none()
                && policy.dot2_token.is_none()
                && policy.rest.is_none()
                && source::path_is(&policy.path, &["MiseTaskSuitePolicy"]) =>
        {
            Some(policy)
        }
        _ => None,
    }
}

pub(super) fn validate_policy_fields(policy: &syn::ExprStruct) -> Result<Span, syn::Error> {
    let spec_fields = policy
        .fields
        .iter()
        .filter_map(|field| named_field(field, "evidence_fetch_spec"))
        .collect::<Vec<_>>();
    let helper_fields = policy
        .fields
        .iter()
        .filter_map(|field| named_field(field, "evidence_helper"))
        .collect::<Vec<_>>();
    if spec_fields.len() != 1 || helper_fields.len() != 1 {
        return Err(source::invalid(
            "evidence policy fields are missing or duplicated",
        ));
    }
    let spec_field = spec_fields[0];
    let helper_field = helper_fields[0];
    if !spec_field.attrs.is_empty()
        || spec_field.colon_token.is_none()
        || !helper_field.attrs.is_empty()
        || helper_field.colon_token.is_none()
    {
        return Err(source::invalid("evidence policy fields must be explicit"));
    }
    validate_spec_projection(&spec_field.expr)?;
    validate_helper_projection(&helper_field.expr)?;
    let syn::Member::Named(ident) = &spec_field.member else {
        return Err(source::invalid("evidence spec field member changed"));
    };
    Ok(ident.span())
}

fn named_field<'a>(field: &'a syn::FieldValue, name: &str) -> Option<&'a syn::FieldValue> {
    match &field.member {
        syn::Member::Named(ident) if ident == name => Some(field),
        _ => None,
    }
}

fn validate_spec_projection(expression: &Expr) -> Result<(), syn::Error> {
    let Some(map) = method_call(expression, "map") else {
        return Err(source::invalid("evidence spec projection changed"));
    };
    if map.args.len() != 1 {
        return Err(source::invalid("evidence spec map arguments changed"));
    }
    let Some(as_ref) = method_call(&map.receiver, "as_ref") else {
        return Err(source::invalid("evidence spec receiver changed"));
    };
    if !as_ref.args.is_empty() || !source::simple_expr_path(&as_ref.receiver, &["evidence"]) {
        return Err(source::invalid("evidence spec receiver changed"));
    }
    let closure = map
        .args
        .first()
        .ok_or_else(|| source::invalid("spec map closure missing"))?;
    let body = closure_body(closure, &["spec", "_"])?;
    let Some(clone) = method_call(body, "clone") else {
        return Err(source::invalid("evidence spec clone projection changed"));
    };
    if !clone.args.is_empty() || !source::simple_expr_path(&clone.receiver, &["spec"]) {
        return Err(source::invalid("evidence spec clone projection changed"));
    }
    Ok(())
}

fn validate_helper_projection(expression: &Expr) -> Result<(), syn::Error> {
    let Some(map) = method_call(expression, "map") else {
        return Err(source::invalid("evidence helper projection changed"));
    };
    if !source::simple_expr_path(&map.receiver, &["evidence"]) || map.args.len() != 1 {
        return Err(source::invalid("evidence helper receiver changed"));
    }
    let closure = map
        .args
        .first()
        .ok_or_else(|| source::invalid("helper map closure missing"))?;
    let body = closure_body(closure, &["_", "helper"])?;
    if !source::simple_expr_path(body, &["helper"]) {
        return Err(source::invalid("evidence helper projection changed"));
    }
    Ok(())
}

fn closure_body<'a>(expression: &'a Expr, pattern: &[&str; 2]) -> Result<&'a Expr, syn::Error> {
    let Expr::Closure(closure) = expression else {
        return Err(source::invalid("evidence projection is not a closure"));
    };
    if !closure.attrs.is_empty()
        || closure.lifetimes.is_some()
        || closure.constness.is_some()
        || closure.asyncness.is_some()
        || closure.capture.is_some()
        || !matches!(&closure.output, syn::ReturnType::Default)
        || closure.modifiers.require_empty().is_err()
        || closure.inputs.len() != 1
    {
        return Err(source::invalid("evidence projection closure changed"));
    }
    let syn::Pat::Tuple(tuple) = &closure.inputs[0] else {
        return Err(source::invalid("evidence projection tuple pattern changed"));
    };
    let values = tuple.elems.iter().collect::<Vec<_>>();
    let Some(first) = values.first().copied() else {
        return Err(source::invalid("evidence projection tuple pattern changed"));
    };
    let Some(second) = values.get(1).copied() else {
        return Err(source::invalid("evidence projection tuple pattern changed"));
    };
    if !tuple.attrs.is_empty()
        || values.len() != 2
        || !tuple_pattern_matches(first, pattern[0])
        || !tuple_pattern_matches(second, pattern[1])
    {
        return Err(source::invalid("evidence projection tuple pattern changed"));
    }
    Ok(&closure.body)
}

fn tuple_pattern_matches(pattern: &syn::Pat, expected: &str) -> bool {
    match (expected, pattern) {
        ("_", syn::Pat::Wild(wild)) => wild.attrs.is_empty(),
        ("_", _) => false,
        (name, pattern) => source::plain_ident(pattern, name),
    }
}

fn method_call<'a>(expression: &'a Expr, name: &str) -> Option<&'a syn::ExprMethodCall> {
    match expression {
        Expr::MethodCall(call)
            if call.attrs.is_empty() && call.method == name && call.turbofish.is_none() =>
        {
            Some(call)
        }
        _ => None,
    }
}
