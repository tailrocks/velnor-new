//! Executable-command and source-preparation checks for the offline gate.

use proc_macro2::{TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

#[path = "impl_orch_f2a_offline_execution_fetch.rs"]
mod fetch_tokens;
use fetch_tokens::macro_tokens_include_cargo_fetch;

pub(crate) fn source_prep_executes_text(source: &str) -> Result<bool, syn::Error> {
    let source = super::production_source_of_text(source)?;
    let mut visitor = RunMethodCalls::default();
    if let Ok(syntax) = syn::parse_file(&source) {
        visitor.visit_file(&syntax);
    } else {
        let expression = syn::parse_str::<syn::Expr>(&source)?;
        visitor.visit_expr(&expression);
    }
    Ok(visitor.found)
}

pub(super) fn collect_process_fetch_hits(source: &str, hits: &mut Vec<(usize, String)>) {
    let mut visitor = CargoFetchCalls::default();
    if let Ok(syntax) = syn::parse_file(source) {
        visitor.visit_file(&syntax);
    } else if let Ok(expression) = syn::parse_str::<syn::Expr>(source) {
        visitor.visit_expr(&expression);
    } else {
        let wrapped = format!("fn __scanner_fragment__() {{\n{source}\n}}");
        let Ok(syntax) = syn::parse_file(&wrapped) else {
            return;
        };
        visitor.visit_file(&syntax);
        for (line, _) in &mut visitor.hits {
            *line = line.saturating_sub(1);
        }
    }
    hits.extend(visitor.hits);
}

#[derive(Default)]
struct RunMethodCalls {
    found: bool,
}

impl<'ast> Visit<'ast> for RunMethodCalls {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if call.method.unraw() == "run" {
            self.found = true;
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if associated_run(&call.func) {
            self.found = true;
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        if macro_tokens_include_run(&expression.mac.tokens) {
            self.found = true;
        }
        visit::visit_expr_macro(self, expression);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if macro_tokens_include_run(&item.mac.tokens) {
            self.found = true;
        }
        visit::visit_item_macro(self, item);
    }

    fn visit_stmt_macro(&mut self, statement: &'ast syn::StmtMacro) {
        if macro_tokens_include_run(&statement.mac.tokens) {
            self.found = true;
        }
        visit::visit_stmt_macro(self, statement);
    }

    fn visit_impl_item_macro(&mut self, item: &'ast syn::ImplItemMacro) {
        if macro_tokens_include_run(&item.mac.tokens) {
            self.found = true;
        }
        visit::visit_impl_item_macro(self, item);
    }

    fn visit_trait_item_macro(&mut self, item: &'ast syn::TraitItemMacro) {
        if macro_tokens_include_run(&item.mac.tokens) {
            self.found = true;
        }
        visit::visit_trait_item_macro(self, item);
    }
}

fn associated_run(callee: &syn::Expr) -> bool {
    match callee {
        syn::Expr::Path(path) => {
            path.path
                .segments
                .last()
                .is_some_and(|segment| segment.ident.unraw() == "run")
                && (path.qself.is_some() || path.path.segments.len() > 1)
        }
        syn::Expr::Paren(expression) => associated_run(&expression.expr),
        syn::Expr::Group(expression) => associated_run(&expression.expr),
        _ => false,
    }
}

fn macro_tokens_include_run(tokens: &TokenStream) -> bool {
    if syn::parse2::<syn::Expr>(tokens.clone()).is_ok_and(|expression| {
        let mut visitor = RunMethodCalls::default();
        visitor.visit_expr(&expression);
        visitor.found
    }) {
        return true;
    }
    let expression_list =
        syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    if let Ok(expressions) = syn::parse::Parser::parse2(expression_list, tokens.clone()) {
        let mut visitor = RunMethodCalls::default();
        for expression in expressions {
            visitor.visit_expr(&expression);
        }
        if visitor.found {
            return true;
        }
    }
    token_stream_has_run_call(tokens.clone())
}

fn token_stream_has_run_call(tokens: TokenStream) -> bool {
    let tokens = tokens.into_iter().collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate() {
        if let TokenTree::Group(group) = token
            && token_stream_has_run_call(group.stream())
        {
            return true;
        }
        let is_run = matches!(token, TokenTree::Ident(identifier) if identifier.unraw() == "run");
        let has_method_separator = index > 0
            && matches!(&tokens[index - 1], TokenTree::Punct(punct) if punct.as_char() == '.');
        let has_path_separator = index > 1
            && matches!(&tokens[index - 1], TokenTree::Punct(punct) if punct.as_char() == ':')
            && matches!(&tokens[index - 2], TokenTree::Punct(punct) if punct.as_char() == ':');
        if is_run
            && (has_method_separator || has_path_separator)
            && run_call_follows(&tokens, index + 1)
        {
            return true;
        }
    }
    false
}

fn run_call_follows(tokens: &[TokenTree], mut index: usize) -> bool {
    if tokens.get(index).is_some_and(is_parenthesized_group) {
        return true;
    }
    let has_turbofish = matches!(tokens.get(index), Some(TokenTree::Punct(punct)) if punct.as_char() == ':' && punct.spacing() == proc_macro2::Spacing::Joint)
        && matches!(tokens.get(index + 1), Some(TokenTree::Punct(punct)) if punct.as_char() == ':')
        && matches!(tokens.get(index + 2), Some(TokenTree::Punct(punct)) if punct.as_char() == '<');
    if !has_turbofish {
        return false;
    }
    index += 2;
    let mut depth = 0_usize;
    while let Some(token) = tokens.get(index) {
        match token {
            TokenTree::Punct(punct) if punct.as_char() == '<' => depth += 1,
            TokenTree::Punct(punct)
                if punct.as_char() == '>' && !is_arrow_greater_than(tokens, index) =>
            {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return tokens.get(index + 1).is_some_and(is_parenthesized_group);
                }
            }
            _ => {}
        }
        index += 1;
    }
    false
}

fn is_arrow_greater_than(tokens: &[TokenTree], index: usize) -> bool {
    index > 0
        && matches!(&tokens[index - 1], TokenTree::Punct(punct) if punct.as_char() == '-' && punct.spacing() == proc_macro2::Spacing::Joint)
}

fn is_parenthesized_group(token: &TokenTree) -> bool {
    matches!(token, TokenTree::Group(group) if group.delimiter() == proc_macro2::Delimiter::Parenthesis)
}

#[derive(Default)]
struct CargoFetchCalls {
    hits: Vec<(usize, String)>,
}

impl<'ast> Visit<'ast> for CargoFetchCalls {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if cargo_fetch_method_call(call) {
            self.hits
                .push((call.span().start().line, "cargo fetch".to_owned()));
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        if macro_tokens_include_cargo_fetch(&expression.mac.tokens) {
            self.hits
                .push((expression.span().start().line, "cargo fetch".to_owned()));
        }
        visit::visit_expr_macro(self, expression);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if macro_tokens_include_cargo_fetch(&item.mac.tokens) {
            self.hits
                .push((item.span().start().line, "cargo fetch".to_owned()));
        }
        visit::visit_item_macro(self, item);
    }

    fn visit_stmt_macro(&mut self, statement: &'ast syn::StmtMacro) {
        if macro_tokens_include_cargo_fetch(&statement.mac.tokens) {
            self.hits
                .push((statement.span().start().line, "cargo fetch".to_owned()));
        }
        visit::visit_stmt_macro(self, statement);
    }

    fn visit_impl_item_macro(&mut self, item: &'ast syn::ImplItemMacro) {
        if macro_tokens_include_cargo_fetch(&item.mac.tokens) {
            self.hits
                .push((item.span().start().line, "cargo fetch".to_owned()));
        }
        visit::visit_impl_item_macro(self, item);
    }

    fn visit_trait_item_macro(&mut self, item: &'ast syn::TraitItemMacro) {
        if macro_tokens_include_cargo_fetch(&item.mac.tokens) {
            self.hits
                .push((item.span().start().line, "cargo fetch".to_owned()));
        }
        visit::visit_trait_item_macro(self, item);
    }
}

fn cargo_fetch_method_call(call: &syn::ExprMethodCall) -> bool {
    let method = call.method.unraw();
    if method == "arg" {
        call.args
            .first()
            .and_then(literal_string)
            .is_some_and(|argument| argument == "fetch")
            && is_cargo_command(&call.receiver)
    } else if method == "args" {
        call.args
            .first()
            .is_some_and(expression_contains_fetch_argument)
            && is_cargo_command(&call.receiver)
    } else {
        false
    }
}

fn expression_contains_fetch_argument(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Array(array) => array
            .elems
            .iter()
            .any(|element| literal_string(element).is_some_and(|argument| argument == "fetch")),
        syn::Expr::Repeat(repeat) => {
            fetch_tokens::array_repeat_contains_fetch(&repeat.expr, &repeat.len)
        }
        syn::Expr::Paren(expression) => expression_contains_fetch_argument(&expression.expr),
        syn::Expr::Group(expression) => expression_contains_fetch_argument(&expression.expr),
        syn::Expr::Reference(expression) => expression_contains_fetch_argument(&expression.expr),
        _ => false,
    }
}

fn is_cargo_command(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Call(call) => {
            let is_command_constructor = match call.func.as_ref() {
                syn::Expr::Path(path) => {
                    let mut segments = path.path.segments.iter().rev();
                    segments
                        .next()
                        .is_some_and(|segment| segment.ident.unraw() == "new")
                        && segments
                            .next()
                            .is_some_and(|segment| segment.ident.unraw() == "Command")
                }
                _ => false,
            };
            is_command_constructor
                && call
                    .args
                    .first()
                    .and_then(literal_string)
                    .is_some_and(|program| program == "cargo")
        }
        syn::Expr::MethodCall(call) => is_cargo_command(&call.receiver),
        syn::Expr::Paren(expression) => is_cargo_command(&expression.expr),
        syn::Expr::Group(expression) => is_cargo_command(&expression.expr),
        _ => false,
    }
}

fn literal_string(expression: &syn::Expr) -> Option<String> {
    match expression {
        syn::Expr::Lit(expression) => match &expression.lit {
            syn::Lit::Str(literal) => Some(literal.value()),
            _ => None,
        },
        syn::Expr::Paren(expression) => literal_string(&expression.expr),
        syn::Expr::Group(expression) => literal_string(&expression.expr),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_builder_requires_executable_cargo_fetch_arguments() {
        for source in [
            r#"Command::new("cargo").arg("fetch");"#,
            r#"std::process::Command::new("cargo").args(["fetch", "--locked"]);"#,
            r#"Command::new("cargo").args(&["fetch"]).status();"#,
            r#"macro_rules! hidden { () => { Command::new("cargo").arg("fetch") }; }"#,
            r#"macro_rules! hidden_statement { () => { Command::new("cargo").args(&["fetch"]).status(); }; }"#,
            r#"macro_rules! hidden_metavariable { ($x:expr) => { let _ = $x; Command::new("cargo").args(&["fetch"]).status(); }; }"#,
            r#"macro_rules! m {
    ($x:expr) => {
        let _ = $x;
        let _: _ = Command::new("cargo").args(&["fetch"]).status();
    };
}
fn production() { m!(0); }"#,
            r#"macro_rules! env_fetch { ($x:expr) => { Command::new("cargo").env("X", $x).args(&["fetch"]).status(); }; }
fn production() { env_fetch!("value"); }"#,
            r#"macro_rules! typed_fetch { ($ty:ty) => { let _: $ty = Command::new("cargo").args(&["fetch"]).status(); }; }
fn production() { typed_fetch!(std::io::Result<std::process::ExitStatus>); }"#,
            r#"macro_rules! attributed_fetch { ($attribute:meta, $ty:ty) => { #[$attribute] let _: $ty = Command::new("cargo").args(&["fetch"]).status(); }; }
fn production() { attributed_fetch!(allow(unused_variables), std::io::Result<std::process::ExitStatus>); }"#,
            r#"macro_rules! repeated_fetch {
    ($($extra:expr),*) => {
        Command::new("cargo").args(["fetch", $($extra),*]).status();
    };
}
fn production() { repeated_fetch!("--locked"); }"#,
            r#"macro_rules! lifetime_fetch {
    ($lt:lifetime) => {
        Command::new("cargo").env("X", { let x: &$lt str = ""; x }).args(["fetch"]).status();
    };
}
fn production() { lifetime_fetch!('static); }"#,
        ] {
            let mut hits = Vec::new();
            collect_process_fetch_hits(source, &mut hits);
            assert!(!hits.is_empty(), "{source}");
        }
        for source in [
            r#"let description = "cargo fetch";"#,
            r#"workflow_step("cargo", "fetch");"#,
            r#"println!("cargo fetch");"#,
            r#"macro_rules! mapped_fetch { () => { Command::new("cargo").args(["fetch"].map(|_| "build")).status(); }; }"#,
        ] {
            let mut hits = Vec::new();
            collect_process_fetch_hits(source, &mut hits);
            assert!(hits.is_empty(), "{source}: {hits:?}");
        }
    }

    #[test]
    fn associated_and_raw_run_calls_are_detected_in_code_and_macro_tokens() {
        for source in [
            "fn f() { Command::run(&command); }",
            "fn f() { <Command as Runner>::run(&command); }",
            "fn f() { Command::r#run(&command); }",
            "macro_rules! hidden { () => { Command::r#run(&command) }; }",
        ] {
            assert!(
                source_prep_executes_text(source).expect("source parses"),
                "{source}"
            );
        }
    }
}
