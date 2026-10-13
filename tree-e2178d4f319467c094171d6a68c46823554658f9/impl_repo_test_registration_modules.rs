//! Compiler-source closure for Cargo test harness roots.

use std::collections::{HashSet, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Item, ItemMod, Meta, Token};

use super::graph::{self, Possibility};

type Outcome<T> = Result<T, Box<dyn Error>>;

struct ModuleFile {
    source: PathBuf,
    module_dir: PathBuf,
    source_base: PathBuf,
}

struct ModuleEdges<'a> {
    inline_items: Option<&'a [Item]>,
    inline_directories: Vec<PathBuf>,
    external_modules: Vec<ModuleFile>,
}

struct ModuleCollector {
    module_dir: PathBuf,
    source_base: PathBuf,
    modules: Vec<ModuleFile>,
    error: Option<String>,
}

impl<'ast> Visit<'ast> for ModuleCollector {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        match module_edges(item, &self.module_dir, &self.source_base) {
            Ok(ModuleEdges {
                inline_items: Some(items),
                inline_directories,
                ..
            }) => {
                for directory in inline_directories {
                    let previous = std::mem::replace(&mut self.module_dir, directory.clone());
                    let previous_base = std::mem::replace(&mut self.source_base, directory);
                    for child in items {
                        self.visit_item(child);
                    }
                    self.module_dir = previous;
                    self.source_base = previous_base;
                }
            }
            Ok(edges) => self.modules.extend(edges.external_modules),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        match graph::attrs_possible_in_test(&item.attrs) {
            Ok(true) => visit::visit_item_fn(self, item),
            Ok(false) => {}
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        match graph::attrs_possible_in_test(&item.attrs) {
            Ok(true) => visit::visit_item_macro(self, item),
            Ok(false) => {}
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

pub(super) fn source_closure(
    root: &Path,
    compiler_dependencies: &HashSet<PathBuf>,
) -> Outcome<HashSet<PathBuf>> {
    let root = root.canonicalize()?;
    let source_base = root
        .parent()
        .ok_or("test source has no parent")?
        .to_path_buf();
    let mut pending = VecDeque::from([ModuleFile {
        source: root.clone(),
        module_dir: source_base.clone(),
        source_base,
    }]);
    let mut visited = HashSet::new();
    let mut sources = HashSet::new();
    while let Some(file) = pending.pop_front() {
        let source = file.source.canonicalize()?;
        if !visited.insert((source.clone(), file.module_dir.clone())) {
            continue;
        }
        sources.insert(source.clone());
        let text = fs::read_to_string(&source)?;
        let syntax = syn::parse_file(&text)?;
        if !graph::attrs_possible_in_test(&syntax.attrs)? {
            continue;
        }
        let mut collector = ModuleCollector {
            module_dir: file.module_dir.clone(),
            source_base: file.source_base.clone(),
            modules: Vec::new(),
            error: None,
        };
        collector.visit_file(&syntax);
        if let Some(error) = collector.error {
            return Err(error.into());
        }
        pending.extend(collector.modules);
        let findings = graph::source_findings(&source)?;
        for include in findings.includes {
            enqueue_compiled_include(&include, true, compiler_dependencies, &file, &mut pending)?;
        }
        for include in findings.macro_includes {
            enqueue_compiled_include(&include, false, compiler_dependencies, &file, &mut pending)?;
        }
    }
    Ok(sources)
}

fn enqueue_compiled_include(
    include: &Path,
    required: bool,
    compiler_dependencies: &HashSet<PathBuf>,
    parent: &ModuleFile,
    pending: &mut VecDeque<ModuleFile>,
) -> Outcome<()> {
    let Some(source) = source_if_present(include)? else {
        return Ok(());
    };
    if !compiler_dependencies.contains(&source) {
        if required {
            return Err(format!(
                "active include! source is missing from rustc dep-info: {}",
                source.display()
            )
            .into());
        }
        return Ok(());
    }
    pending.push_back(ModuleFile {
        source,
        module_dir: parent.module_dir.clone(),
        source_base: parent.source_base.clone(),
    });
    Ok(())
}

fn module_edges<'a>(
    item: &'a ItemMod,
    module_dir: &Path,
    source_base: &Path,
) -> Outcome<ModuleEdges<'a>> {
    if !graph::attrs_possible_in_test(&item.attrs)? {
        return Ok(ModuleEdges {
            inline_items: None,
            inline_directories: Vec::new(),
            external_modules: Vec::new(),
        });
    }
    let paths = path_attributes(&item.attrs)?;
    if let Some((_, items)) = &item.content {
        return Ok(ModuleEdges {
            inline_items: Some(items),
            inline_directories: inline_directories(&item.ident.to_string(), &paths, module_dir),
            external_modules: Vec::new(),
        });
    }
    let modules = external_modules(&item.ident.to_string(), &paths, module_dir, source_base)?;
    Ok(ModuleEdges {
        inline_items: None,
        inline_directories: Vec::new(),
        external_modules: modules,
    })
}

struct PathAttribute {
    path: PathBuf,
    condition: Possibility,
    direct: bool,
}

fn path_attributes(attributes: &[Attribute]) -> Outcome<Vec<PathAttribute>> {
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

fn inline_directories(name: &str, paths: &[PathAttribute], parent: &Path) -> Vec<PathBuf> {
    let has_direct = paths.iter().any(|path| path.direct);
    let always_conditional = paths
        .iter()
        .any(|path| !path.direct && path.condition == Possibility::Always);
    let mut directories = Vec::new();
    if !has_direct && !always_conditional {
        directories.push(parent.join(name));
    }
    for path in paths {
        if path.condition != Possibility::Never {
            directories.push(parent.join(&path.path));
        }
    }
    directories.sort();
    directories.dedup();
    directories
}

fn external_modules(
    name: &str,
    paths: &[PathAttribute],
    parent: &Path,
    source_base: &Path,
) -> Outcome<Vec<ModuleFile>> {
    let has_direct = paths.iter().any(|path| path.direct);
    let always_conditional = paths
        .iter()
        .any(|path| !path.direct && path.condition == Possibility::Always);
    let mut sources = Vec::new();
    if !has_direct && !always_conditional {
        for path in [
            parent.join(format!("{name}.rs")),
            parent.join(name).join("mod.rs"),
        ] {
            if let Some(source) = source_if_present(&path)? {
                sources.push(ModuleFile {
                    module_dir: default_module_dir(&source),
                    source_base: source
                        .parent()
                        .ok_or("module source has no parent")?
                        .to_path_buf(),
                    source,
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
            });
        }
    }
    sources.sort_by(|left, right| left.source.cmp(&right.source));
    sources
        .dedup_by(|left, right| left.source == right.source && left.module_dir == right.module_dir);
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

fn source_if_present(path: &Path) -> Outcome<Option<PathBuf>> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(path.canonicalize()?)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn combine(left: Possibility, right: Possibility) -> Possibility {
    match (left, right) {
        (Possibility::Never, _) | (_, Possibility::Never) => Possibility::Never,
        (Possibility::Always, Possibility::Always) => Possibility::Always,
        _ => Possibility::Sometimes,
    }
}
