//! Rust module path attributes and source-relative child directories.

use std::error::Error;
use std::path::{Path, PathBuf};

use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Attribute, ItemMod, Meta, Token};

use super::{ModuleFile, graph, source_if_present};
use graph::Possibility;

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(super) struct PathAttribute {
    pub(super) path: PathBuf,
    pub(super) condition: Possibility,
    direct: bool,
}

pub(super) fn path_attributes(attributes: &[Attribute]) -> Outcome<Vec<PathAttribute>> {
    let mut output = Vec::new();
    for attribute in attributes {
        collect_path_attribute(&attribute.meta, Possibility::Always, true, &mut output)?;
    }
    Ok(output)
}

fn collect_path_attribute(
    meta: &Meta,
    parent_condition: Possibility,
    direct: bool,
    output: &mut Vec<PathAttribute>,
) -> Outcome<()> {
    if parent_condition == Possibility::Never {
        return Ok(());
    }
    match meta {
        Meta::NameValue(value) if value.path.is_ident("path") => {
            let syn::Expr::Lit(expression) = &value.value else {
                return Err("module path attribute is not a string literal".into());
            };
            let syn::Lit::Str(literal) = &expression.lit else {
                return Err("module path attribute is not a string literal".into());
            };
            output.push(PathAttribute {
                path: PathBuf::from(literal.value()),
                condition: parent_condition,
                direct,
            });
        }
        Meta::List(list) if list.path.is_ident("cfg_attr") => {
            let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
            let nested = parser.parse2(list.tokens.clone())?;
            let mut items = nested.iter();
            let condition = items.next().ok_or("cfg_attr has no condition")?;
            let condition = combine(parent_condition, graph::test_condition_state(condition)?);
            for nested in items {
                collect_path_attribute(nested, condition, false, output)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn inline_directories(
    item: &ItemMod,
    paths: &[PathAttribute],
    parent: &Path,
    module_condition: Possibility,
) -> Vec<(PathBuf, Possibility)> {
    let has_direct = paths.iter().any(|path| path.direct);
    let always_conditional = paths
        .iter()
        .any(|path| !path.direct && path.condition == Possibility::Always);
    let mut directories = Vec::new();
    if !has_direct && !always_conditional {
        directories.push((parent.join(item.ident.to_string()), module_condition));
    }
    for path in paths {
        if path.condition != Possibility::Never {
            directories.push((parent.join(&path.path), path.condition));
        }
    }
    directories.sort_by(|left, right| left.0.cmp(&right.0));
    directories.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    directories
}

pub(super) fn external_modules(
    item: &ItemMod,
    paths: &[PathAttribute],
    parent: &Path,
    source_base: &Path,
    module_condition: Possibility,
) -> Outcome<Vec<ModuleFile>> {
    let has_direct = paths.iter().any(|path| path.direct);
    let always_conditional = paths
        .iter()
        .any(|path| !path.direct && path.condition == Possibility::Always);
    let name = item.ident.to_string();
    let mut sources = Vec::new();
    if !has_direct && !always_conditional {
        for path in [
            parent.join(format!("{name}.rs")),
            parent.join(&name).join("mod.rs"),
        ] {
            if let Some(source) = source_if_present(&path)? {
                sources.push(ModuleFile {
                    module_dir: default_module_dir(&source),
                    source_base: source
                        .parent()
                        .ok_or("module source has no parent")?
                        .to_path_buf(),
                    source,
                    condition: module_condition,
                });
            }
        }
    }
    for path in paths {
        if path.condition == Possibility::Never {
            continue;
        }
        if let Some(source) = source_if_present(&source_base.join(&path.path))? {
            let module_dir = source
                .parent()
                .ok_or("explicit module path has no parent")?
                .to_path_buf();
            sources.push(ModuleFile {
                source_base: source
                    .parent()
                    .ok_or("module source has no parent")?
                    .to_path_buf(),
                source,
                module_dir,
                condition: path.condition,
            });
        }
    }
    sources.sort_by(|left, right| left.source.cmp(&right.source));
    sources.dedup_by(|left, right| {
        left.source == right.source
            && left.module_dir == right.module_dir
            && left.condition == right.condition
    });
    Ok(sources)
}

fn default_module_dir(source: &Path) -> PathBuf {
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    if source.file_name().is_some_and(|name| name == "mod.rs") {
        parent.to_path_buf()
    } else {
        parent.join(source.file_stem().unwrap_or_default())
    }
}

fn combine(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Never, _) | (_, Possibility::Never) => Possibility::Never,
        (Possibility::Always, Possibility::Always) => Possibility::Always,
        _ => Possibility::Sometimes,
    }
}
