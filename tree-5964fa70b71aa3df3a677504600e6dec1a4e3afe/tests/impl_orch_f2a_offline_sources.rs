//! Production module graph for the offline-fetch structural gate.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};

use super::{impl_item_attrs, trait_item_attrs};
use proc_macro2::{TokenStream, TokenTree};
#[path = "impl_orch_f2a_offline_sources_attrs.rs"]
mod attrs;
#[path = "impl_orch_f2a_offline_source_forms.rs"]
mod source_forms;
use attrs::{inline_dirs, is_cfg_test_only, item_attrs, path_attribute, resolve_external_module};
use source_forms::{ContextualSource, parse_contextual};
#[path = "impl_orch_f2a_offline_source_visitor.rs"]
mod source_visitor;
use source_visitor::NestedItems;
use syn::ext::IdentExt;
use syn::visit::{self, Visit};

pub(crate) fn production_src_files(src_root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let root = src_root.join("lib.rs");
    production_src_files_from(&root, src_root, src_root)
}

pub(crate) fn production_src_files_from(
    root: &Path,
    module_dir: &Path,
    attribute_dir: &Path,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut walker = ProductionModules::default();
    walker.walk_file(root, module_dir, attribute_dir, SourceForm::Items)?;
    Ok(walker.files.into_iter().collect())
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SourceForm {
    Items,
    Expression,
    Contextual,
}

#[derive(Default)]
struct ProductionModules {
    files: BTreeSet<PathBuf>,
    visited: BTreeSet<(PathBuf, PathBuf, PathBuf, PathBuf, SourceForm)>,
}

impl ProductionModules {
    fn walk_file(
        &mut self,
        file: &Path,
        module_dir: &Path,
        attribute_dir: &Path,
        source_form: SourceForm,
    ) -> Result<(), Box<dyn Error>> {
        let canonical = file.canonicalize()?;
        let logical = file.to_path_buf();
        let key = (
            canonical.clone(),
            logical.clone(),
            module_dir.to_path_buf(),
            attribute_dir.to_path_buf(),
            source_form,
        );
        if !self.visited.insert(key) {
            return Ok(());
        }
        let source = fs::read_to_string(&canonical)?;
        match source_form {
            SourceForm::Items => {
                let syntax = syn::parse_file(&source).map_err(|error| {
                    IoError::new(
                        ErrorKind::InvalidData,
                        format!(
                            "could not parse production Rust source {}: {error}",
                            logical.display()
                        ),
                    )
                })?;
                if super::has_cfg_test(&syntax.attrs) {
                    return Ok(());
                }
                self.files.insert(canonical);
                self.walk_items(&syntax.items, &logical, module_dir, attribute_dir)
            }
            SourceForm::Expression => {
                let expression = syn::parse_str::<syn::Expr>(&source).map_err(|error| {
                    IoError::new(
                        ErrorKind::InvalidData,
                        format!(
                            "could not parse included Rust expression {}: {error}",
                            logical.display()
                        ),
                    )
                })?;
                self.files.insert(canonical);
                self.walk_expression(&expression, &logical, module_dir, attribute_dir)
            }
            SourceForm::Contextual => {
                match parse_contextual(&source).map_err(|error| {
                    IoError::new(
                        ErrorKind::InvalidData,
                        format!(
                            "could not parse included Rust source {}: {error}",
                            logical.display()
                        ),
                    )
                })? {
                    ContextualSource::Items(syntax) => {
                        if super::has_cfg_test(&syntax.attrs) {
                            return Ok(());
                        }
                        self.files.insert(canonical);
                        self.walk_items(&syntax.items, &logical, module_dir, attribute_dir)
                    }
                    ContextualSource::Expression(expression) => {
                        self.files.insert(canonical);
                        self.walk_expression(&expression, &logical, module_dir, attribute_dir)
                    }
                }
            }
        }
    }

    fn walk_expression(
        &mut self,
        expression: &syn::Expr,
        source_file: &Path,
        module_dir: &Path,
        attribute_dir: &Path,
    ) -> Result<(), Box<dyn Error>> {
        let mut visitor = NestedItems {
            walker: self,
            source_file,
            module_dir,
            attribute_dir,
            error: None,
        };
        visitor.visit_expr(expression);
        visitor.error.map_or(Ok(()), Err)
    }

    fn walk_items(
        &mut self,
        items: &[syn::Item],
        source_file: &Path,
        module_dir: &Path,
        attribute_dir: &Path,
    ) -> Result<(), Box<dyn Error>> {
        for item in items {
            self.walk_item(item, source_file, module_dir, attribute_dir)?;
        }
        Ok(())
    }

    fn walk_item(
        &mut self,
        item: &syn::Item,
        source_file: &Path,
        module_dir: &Path,
        attribute_dir: &Path,
    ) -> Result<(), Box<dyn Error>> {
        if item_attrs(item).is_some_and(is_cfg_test_only) {
            return Ok(());
        }
        let syn::Item::Mod(module) = item else {
            let mut visitor = NestedItems {
                walker: self,
                source_file,
                module_dir,
                attribute_dir,
                error: None,
            };
            visit::visit_item(&mut visitor, item);
            return visitor.error.map_or(Ok(()), Err);
        };
        let explicit_path = path_attribute(&module.attrs)?;
        if let Some((_, items)) = &module.content {
            let (inline_module_dir, inline_attribute_dir) =
                inline_dirs(module, explicit_path.as_deref(), module_dir, attribute_dir);
            self.walk_items(
                items,
                source_file,
                &inline_module_dir,
                &inline_attribute_dir,
            )?;
        } else {
            let (path, child_module_dir, child_attribute_dir) = resolve_external_module(
                module,
                explicit_path.as_deref(),
                module_dir,
                attribute_dir,
                source_file,
            )?;
            self.walk_file(
                &path,
                &child_module_dir,
                &child_attribute_dir,
                SourceForm::Items,
            )?;
        }
        Ok(())
    }

    fn walk_include_macro(
        &mut self,
        macro_call: &syn::Macro,
        source_file: &Path,
        source_form: SourceForm,
    ) -> Result<(), Box<dyn Error>> {
        if macro_call
            .path
            .segments
            .last()
            .is_none_or(|segment| segment.ident.unraw() != "include")
        {
            return Ok(());
        }
        let literal = syn::parse2::<syn::LitStr>(macro_call.tokens.clone()).map_err(|error| {
            IoError::new(
                ErrorKind::InvalidData,
                format!(
                    "include! in {} must use a literal path: {error}",
                    source_file.display()
                ),
            )
        })?;
        self.walk_include_literal(&literal, source_file, source_form)
    }

    fn walk_include_tokens(
        &mut self,
        tokens: TokenStream,
        source_file: &Path,
    ) -> Result<(), Box<dyn Error>> {
        let tokens = tokens.into_iter().collect::<Vec<_>>();
        for (index, token) in tokens.iter().enumerate() {
            if let TokenTree::Group(group) = token {
                self.walk_include_tokens(group.stream(), source_file)?;
            }
            let is_include =
                matches!(token, TokenTree::Ident(identifier) if identifier.unraw() == "include");
            let has_bang = matches!(tokens.get(index + 1), Some(TokenTree::Punct(punct)) if punct.as_char() == '!');
            let Some(TokenTree::Group(arguments)) = tokens.get(index + 2) else {
                continue;
            };
            if !is_include || !has_bang {
                continue;
            }
            let literal = syn::parse2::<syn::LitStr>(arguments.stream()).map_err(|error| {
                IoError::new(
                    ErrorKind::InvalidData,
                    format!(
                        "include! in {} must use a literal path: {error}",
                        source_file.display()
                    ),
                )
            })?;
            self.walk_include_literal(&literal, source_file, SourceForm::Contextual)?;
        }
        Ok(())
    }

    fn reject_module_macro_tokens(
        tokens: TokenStream,
        source_file: &Path,
    ) -> Result<(), Box<dyn Error>> {
        if source_visitor::token_stream_contains_module(tokens) {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                format!(
                    "module-generating macro in {} has unresolved expansion paths",
                    source_file.display()
                ),
            )
            .into());
        }
        Ok(())
    }

    fn walk_include_literal(
        &mut self,
        literal: &syn::LitStr,
        source_file: &Path,
        source_form: SourceForm,
    ) -> Result<(), Box<dyn Error>> {
        let parent = source_file.parent().ok_or_else(|| {
            IoError::new(
                ErrorKind::InvalidData,
                format!(
                    "include! source {} has no parent directory",
                    source_file.display()
                ),
            )
        })?;
        let included = parent.join(literal.value());
        if !included.is_file() {
            return Err(IoError::new(
                ErrorKind::InvalidData,
                format!("include! target {} does not exist", included.display()),
            )
            .into());
        }
        let included_parent = included.parent().ok_or_else(|| {
            IoError::new(
                ErrorKind::InvalidData,
                format!(
                    "include! target {} has no parent directory",
                    included.display()
                ),
            )
        })?;
        // Rust resolves module paths in included source relative to that source's
        // directory. Keep the lexical include path so symlinks do not redirect it.
        self.walk_file(&included, included_parent, included_parent, source_form)
    }
}
