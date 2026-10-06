//! Compiler-source closure for Cargo test harness roots.

use std::collections::{HashSet, VecDeque};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use syn::visit::{self, Visit};
use syn::{Item, ItemMod};

use super::graph::{self, Possibility};

#[path = "impl_repo_test_registration_macro_closure.rs"]
mod macro_closure;
#[path = "impl_repo_test_registration_module_paths.rs"]
mod module_paths;

type Outcome<T> = Result<T, Box<dyn Error>>;

struct ModuleFile {
    source: PathBuf,
    module_dir: PathBuf,
    source_base: PathBuf,
    condition: Possibility,
}

struct SourceRecord {
    file: ModuleFile,
    findings: graph::SourceFindings,
}

struct ModuleEdges<'a> {
    inline_items: Option<&'a [Item]>,
    inline_directories: Vec<(PathBuf, Possibility)>,
    external_modules: Vec<ModuleFile>,
}

struct ModuleCollector {
    module_dir: PathBuf,
    source_base: PathBuf,
    condition: Possibility,
    modules: Vec<ModuleFile>,
    error: Option<String>,
}

impl<'ast> Visit<'ast> for ModuleCollector {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        match module_edges(item, &self.module_dir, &self.source_base, self.condition) {
            Ok(ModuleEdges {
                inline_items: Some(items),
                inline_directories,
                ..
            }) => {
                for (directory, condition) in inline_directories {
                    let previous = std::mem::replace(&mut self.module_dir, directory.clone());
                    let previous_base = std::mem::replace(&mut self.source_base, directory);
                    let previous_condition = std::mem::replace(&mut self.condition, condition);
                    for child in items {
                        self.visit_item(child);
                    }
                    self.module_dir = previous;
                    self.source_base = previous_base;
                    self.condition = previous_condition;
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
    source_closure_with_evidence(root, Some(compiler_dependencies))
}

pub(super) fn declared_target_source_closure(root: &Path) -> Outcome<HashSet<PathBuf>> {
    source_closure_with_evidence(root, None)
}

fn source_closure_with_evidence(
    root: &Path,
    compiler_dependencies: Option<&HashSet<PathBuf>>,
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
        condition: Possibility::Always,
    }]);
    let mut visited = HashSet::new();
    let mut sources = HashSet::new();
    let mut records = Vec::new();
    let mut expanded_macro_definitions = HashSet::new();
    loop {
        while let Some(mut file) = pending.pop_front() {
            let source = file.source.canonicalize()?;
            file.source.clone_from(&source);
            if !visited.insert((source.clone(), file.module_dir.clone(), file.condition)) {
                continue;
            }
            sources.insert(source.clone());
            let text = fs::read_to_string(&source)?;
            let syntax = syn::parse_file(&text)?;
            let syntax_condition = graph::attrs_possibility_in_test(&syntax.attrs)?;
            let source_condition = combine(file.condition, syntax_condition);
            if source_condition != Possibility::Never {
                let mut collector = ModuleCollector {
                    module_dir: file.module_dir.clone(),
                    source_base: file.source_base.clone(),
                    condition: source_condition,
                    modules: Vec::new(),
                    error: None,
                };
                collector.visit_file(&syntax);
                if let Some(error) = collector.error {
                    return Err(error.into());
                }
                pending.extend(collector.modules);
            }
            let findings = graph::source_findings(&source)?;
            for include in &findings.includes {
                enqueue_compiled_include(
                    &include.path,
                    combine(file.condition, include.condition),
                    compiler_dependencies,
                    &file,
                    &mut pending,
                )?;
            }
            records.push(SourceRecord { file, findings });
        }
        let macro_includes = macro_closure::collect(&records, &mut expanded_macro_definitions)?;
        if macro_includes.is_empty() {
            break;
        }
        for (path, condition, parent) in macro_includes {
            enqueue_compiled_include(
                &path,
                condition,
                compiler_dependencies,
                &parent,
                &mut pending,
            )?;
        }
    }
    Ok(sources)
}

fn enqueue_compiled_include(
    include: &Path,
    condition: Possibility,
    compiler_dependencies: Option<&HashSet<PathBuf>>,
    parent: &ModuleFile,
    pending: &mut VecDeque<ModuleFile>,
) -> Outcome<()> {
    if condition == Possibility::Never {
        return Ok(());
    }
    let Some(source) = source_if_present(include)? else {
        if condition == Possibility::Always {
            return Err(format!("active include! source is missing: {}", include.display()).into());
        }
        return Ok(());
    };
    if compiler_dependencies.is_some_and(|dependencies| !dependencies.contains(&source))
        && condition == Possibility::Always
    {
        return Err(format!(
            "active include! source is missing from rustc dep-info: {}",
            source.display()
        )
        .into());
    }
    pending.push_back(ModuleFile {
        source,
        module_dir: parent.module_dir.clone(),
        source_base: parent.source_base.clone(),
        condition: combine(parent.condition, condition),
    });
    Ok(())
}

fn module_edges<'a>(
    item: &'a ItemMod,
    module_dir: &Path,
    source_base: &Path,
    parent_condition: Possibility,
) -> Outcome<ModuleEdges<'a>> {
    let module_condition = combine(
        parent_condition,
        graph::attrs_possibility_in_test(&item.attrs)?,
    );
    if module_condition == Possibility::Never {
        return Ok(ModuleEdges {
            inline_items: None,
            inline_directories: Vec::new(),
            external_modules: Vec::new(),
        });
    }
    let mut paths = module_paths::path_attributes(&item.attrs)?;
    for path in &mut paths {
        path.condition = combine(module_condition, path.condition);
    }
    if let Some((_, items)) = &item.content {
        return Ok(ModuleEdges {
            inline_items: Some(items),
            inline_directories: module_paths::inline_directories(
                item,
                &paths,
                module_dir,
                module_condition,
            ),
            external_modules: Vec::new(),
        });
    }
    let modules =
        module_paths::external_modules(item, &paths, module_dir, source_base, module_condition)?;
    Ok(ModuleEdges {
        inline_items: None,
        inline_directories: Vec::new(),
        external_modules: modules,
    })
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
