//! AST visitor shared by source and include traversal.

use std::error::Error;
use std::path::Path;

use proc_macro2::{TokenStream, TokenTree};
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

use super::{ProductionModules, SourceForm, impl_item_attrs, is_cfg_test_only, trait_item_attrs};

pub(super) struct NestedItems<'a> {
    pub(super) walker: &'a mut ProductionModules,
    pub(super) source_file: &'a Path,
    pub(super) module_dir: &'a Path,
    pub(super) attribute_dir: &'a Path,
    pub(super) error: Option<Box<dyn Error>>,
}

impl NestedItems<'_> {
    fn record_include(&mut self, macro_call: &syn::Macro, source_form: SourceForm) {
        if self.error.is_some() {
            return;
        }
        let result = self
            .walker
            .walk_include_macro(macro_call, self.source_file, source_form)
            .and_then(|()| {
                self.walker
                    .walk_include_tokens(macro_call.tokens.clone(), self.source_file)
            })
            .and_then(|()| {
                ProductionModules::reject_module_macro_tokens(
                    macro_call.tokens.clone(),
                    self.source_file,
                )
            });
        if let Err(error) = result {
            self.error = Some(error);
        }
    }
}

pub(super) fn token_stream_contains_module(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|token| match token {
        TokenTree::Ident(identifier) => identifier.unraw() == "mod",
        TokenTree::Group(group) => token_stream_contains_module(group.stream()),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

impl<'ast> Visit<'ast> for NestedItems<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if self.error.is_some() {
            return;
        }
        if let Err(error) =
            self.walker
                .walk_item(item, self.source_file, self.module_dir, self.attribute_dir)
        {
            self.error = Some(error);
        }
    }

    fn visit_expr(&mut self, expression: &'ast syn::Expr) {
        if super::super::expression_is_cfg_test_only(expression) {
            return;
        }
        visit::visit_expr(self, expression);
    }

    fn visit_expr_macro(&mut self, expression: &'ast syn::ExprMacro) {
        self.record_include(&expression.mac, SourceForm::Expression);
        visit::visit_expr_macro(self, expression);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        self.record_include(&item.mac, SourceForm::Items);
        visit::visit_item_macro(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        if impl_item_attrs(item).is_some_and(is_cfg_test_only) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'ast syn::TraitItem) {
        if trait_item_attrs(item).is_some_and(is_cfg_test_only) {
            return;
        }
        visit::visit_trait_item(self, item);
    }

    fn visit_stmt_macro(&mut self, statement: &'ast syn::StmtMacro) {
        if is_cfg_test_only(&statement.attrs) {
            return;
        }
        self.record_include(&statement.mac, SourceForm::Contextual);
        visit::visit_stmt_macro(self, statement);
    }

    fn visit_impl_item_macro(&mut self, item: &'ast syn::ImplItemMacro) {
        self.record_include(&item.mac, SourceForm::Items);
        visit::visit_impl_item_macro(self, item);
    }

    fn visit_trait_item_macro(&mut self, item: &'ast syn::TraitItemMacro) {
        self.record_include(&item.mac, SourceForm::Items);
        visit::visit_trait_item_macro(self, item);
    }
}
