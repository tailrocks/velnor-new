//! Attribute handling for the offline-fetch production module graph.

use std::error::Error;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};

use syn::ext::IdentExt;

pub(super) fn path_attribute(attrs: &[syn::Attribute]) -> Result<Option<String>, Box<dyn Error>> {
    let mut found = None;
    for attribute in attrs {
        if attribute.path().is_ident("path") {
            collect_path_meta(&attribute.meta, CfgValue::True, &mut found)?;
        } else if attribute.path().is_ident("cfg_attr") {
            collect_cfg_attr(&attribute.meta, &mut found)?;
        }
    }
    Ok(found)
}

fn collect_cfg_attr(meta: &syn::Meta, found: &mut Option<String>) -> Result<(), Box<dyn Error>> {
    let syn::Meta::List(list) = meta else {
        return Err(attribute_error("malformed #[cfg_attr] attribute").into());
    };
    let arguments = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    )?;
    let mut arguments = arguments.into_iter();
    let Some(condition) = arguments.next() else {
        return Err(attribute_error("empty #[cfg_attr] attribute").into());
    };
    let condition = production_cfg_value(&condition)?;
    for attribute in arguments {
        collect_path_meta(&attribute, condition, found)?;
    }
    Ok(())
}

fn collect_path_meta(
    meta: &syn::Meta,
    active: CfgValue,
    found: &mut Option<String>,
) -> Result<(), Box<dyn Error>> {
    if active == CfgValue::False {
        return Ok(());
    }
    if active == CfgValue::Unknown {
        if meta_may_set_path(meta)? {
            return Err(attribute_error(
                "unresolved cfg_attr predicate controls a production module path",
            )
            .into());
        }
        return Ok(());
    }
    if meta.path().is_ident("path") {
        let syn::Meta::NameValue(value) = meta else {
            return Err(attribute_error("malformed #[path] attribute").into());
        };
        let syn::Expr::Lit(expression) = &value.value else {
            return Err(attribute_error("#[path] must be a string literal").into());
        };
        let syn::Lit::Str(path) = &expression.lit else {
            return Err(attribute_error("#[path] must be a string literal").into());
        };
        if found.replace(path.value()).is_some() {
            return Err(attribute_error("multiple active #[path] attributes on module").into());
        }
    } else if meta.path().is_ident("cfg_attr") {
        collect_cfg_attr(meta, found)?;
    }
    Ok(())
}

fn meta_may_set_path(meta: &syn::Meta) -> Result<bool, Box<dyn Error>> {
    if meta.path().is_ident("path") {
        return Ok(true);
    }
    let syn::Meta::List(list) = meta else {
        return Ok(false);
    };
    if !list.path.is_ident("cfg_attr") {
        return Ok(false);
    }
    let arguments = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    )?;
    arguments.iter().skip(1).try_fold(false, |found, nested| {
        Ok(found || meta_may_set_path(nested)?)
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CfgValue {
    True,
    False,
    Unknown,
}

fn production_cfg_value(meta: &syn::Meta) -> Result<CfgValue, Box<dyn Error>> {
    match meta {
        syn::Meta::Path(path) if path.is_ident("test") => Ok(CfgValue::False),
        syn::Meta::List(list) if list.path.is_ident("not") => {
            let arguments = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )?;
            if arguments.len() != 1 {
                return Err(attribute_error("not(...) requires one cfg predicate").into());
            }
            Ok(match production_cfg_value(&arguments[0])? {
                CfgValue::True => CfgValue::False,
                CfgValue::False => CfgValue::True,
                CfgValue::Unknown => CfgValue::Unknown,
            })
        }
        syn::Meta::List(list) if list.path.is_ident("all") => {
            let values = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )?;
            let mut result = CfgValue::True;
            for value in values
                .iter()
                .map(production_cfg_value)
                .collect::<Result<Vec<_>, _>>()?
            {
                result = match (result, value) {
                    (CfgValue::False, _) | (_, CfgValue::False) => CfgValue::False,
                    (CfgValue::Unknown, _) | (_, CfgValue::Unknown) => CfgValue::Unknown,
                    _ => CfgValue::True,
                };
            }
            Ok(result)
        }
        syn::Meta::List(list) if list.path.is_ident("any") => {
            let values = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )?;
            let mut result = CfgValue::False;
            for value in values
                .iter()
                .map(production_cfg_value)
                .collect::<Result<Vec<_>, _>>()?
            {
                result = match (result, value) {
                    (CfgValue::True, _) | (_, CfgValue::True) => CfgValue::True,
                    (CfgValue::Unknown, _) | (_, CfgValue::Unknown) => CfgValue::Unknown,
                    _ => CfgValue::False,
                };
            }
            Ok(result)
        }
        _ => Ok(CfgValue::Unknown),
    }
}

pub(super) fn is_cfg_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attribute| attribute.path().is_ident("cfg") && cfg_expr_is_test_only(&attribute.meta))
}

fn cfg_expr_is_test_only(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("cfg") => list
            .parse_args::<syn::Meta>()
            .is_ok_and(|expression| cfg_expr_is_test_only(&expression)),
        syn::Meta::List(list) if list.path.is_ident("all") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|items| items.iter().any(cfg_expr_is_test_only)),
        syn::Meta::List(list) if list.path.is_ident("any") => list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|items| !items.is_empty() && items.iter().all(cfg_expr_is_test_only)),
        _ => false,
    }
}

pub(super) fn item_attrs(item: &syn::Item) -> Option<&[syn::Attribute]> {
    match item {
        syn::Item::Const(item) => Some(&item.attrs),
        syn::Item::Enum(item) => Some(&item.attrs),
        syn::Item::ExternCrate(item) => Some(&item.attrs),
        syn::Item::Fn(item) => Some(&item.attrs),
        syn::Item::ForeignMod(item) => Some(&item.attrs),
        syn::Item::Impl(item) => Some(&item.attrs),
        syn::Item::Macro(item) => Some(&item.attrs),
        syn::Item::Mod(item) => Some(&item.attrs),
        syn::Item::Static(item) => Some(&item.attrs),
        syn::Item::Struct(item) => Some(&item.attrs),
        syn::Item::Trait(item) => Some(&item.attrs),
        syn::Item::TraitAlias(item) => Some(&item.attrs),
        syn::Item::Type(item) => Some(&item.attrs),
        syn::Item::Union(item) => Some(&item.attrs),
        syn::Item::Use(item) => Some(&item.attrs),
        _ => None,
    }
}

pub(super) fn module_error(
    file: &Path,
    module: &syn::ItemMod,
    message: impl std::fmt::Display,
) -> IoError {
    let location = syn::spanned::Spanned::span(module).start();
    IoError::new(
        ErrorKind::InvalidData,
        format!(
            "{}:{}:{}: {message}",
            file.display(),
            location.line,
            location.column + 1
        ),
    )
}

pub(super) fn inline_dirs(
    module: &syn::ItemMod,
    explicit_path: Option<&str>,
    module_dir: &Path,
    attribute_dir: &Path,
) -> (PathBuf, PathBuf) {
    let name = module.ident.unraw().to_string();
    if let Some(path) = explicit_path {
        let path = attribute_dir.join(path);
        (path.clone(), path)
    } else {
        let child = module_dir.join(&name);
        (child.clone(), child)
    }
}

pub(super) fn resolve_external_module(
    module: &syn::ItemMod,
    explicit_path: Option<&str>,
    module_dir: &Path,
    attribute_dir: &Path,
    source_file: &Path,
) -> Result<(PathBuf, PathBuf, PathBuf), Box<dyn Error>> {
    if let Some(path) = explicit_path {
        let resolved = attribute_dir.join(path);
        if !resolved.is_file() {
            return Err(module_error(
                source_file,
                module,
                format!("#[path] target {} does not exist", resolved.display()),
            )
            .into());
        }
        let parent = resolved.parent().ok_or_else(|| {
            module_error(
                source_file,
                module,
                "#[path] target has no parent directory",
            )
        })?;
        let child_dir = parent.to_path_buf();
        return Ok((resolved, child_dir.clone(), child_dir));
    }

    let name = module.ident.unraw().to_string();
    let file_candidate = module_dir.join(format!("{name}.rs"));
    let directory_candidate = module_dir.join(&name).join("mod.rs");
    match (file_candidate.is_file(), directory_candidate.is_file()) {
        (true, false) => {
            let child_dir = file_candidate.with_extension("");
            let child_attribute_dir = file_candidate
                .parent()
                .ok_or_else(|| module_error(source_file, module, "module file has no parent"))?
                .to_path_buf();
            Ok((file_candidate, child_dir, child_attribute_dir))
        }
        (false, true) => {
            let child_dir = directory_candidate
                .parent()
                .ok_or_else(|| module_error(source_file, module, "mod.rs has no parent"))?
                .to_path_buf();
            Ok((directory_candidate, child_dir.clone(), child_dir))
        }
        (false, false) => Err(module_error(
            source_file,
            module,
            format!(
                "active production module `{name}` has no source at {} or {}",
                file_candidate.display(),
                directory_candidate.display()
            ),
        )
        .into()),
        (true, true) => Err(module_error(
            source_file,
            module,
            format!(
                "active production module `{name}` is ambiguous between {} and {}",
                file_candidate.display(),
                directory_candidate.display()
            ),
        )
        .into()),
    }
}

fn attribute_error(message: impl std::fmt::Display) -> IoError {
    IoError::new(ErrorKind::InvalidData, message.to_string())
}
