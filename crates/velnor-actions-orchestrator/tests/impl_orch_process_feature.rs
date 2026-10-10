//! The sole orchestrator process-feature use is the read-only stage UID check.

use std::error::Error;
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::{self, Visit};
use syn::{Expr, ExprCall, ExprPath, Path as RustPath, UseTree};

const OWNER_CHECK_SOURCE: &str = "src/generate_stage_root_fs.rs";

#[derive(Default)]
struct ProcessFeatureAudit {
    uid_calls: usize,
    violations: usize,
}

impl<'ast> Visit<'ast> for ProcessFeatureAudit {
    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(ExprPath {
            qself: None, path, ..
        }) = &*call.func
            && is_geteuid(path)
            && call.args.is_empty()
        {
            self.uid_calls += 1;
            for argument in &call.args {
                self.visit_expr(argument);
            }
            return;
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_path(&mut self, path: &'ast RustPath) {
        if is_process_path(path) {
            self.violations += 1;
        }
        visit::visit_path(self, path);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if forbidden_rustix_import(&item.tree, &[]) {
            self.violations += 1;
        }
        visit::visit_item_use(self, item);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        if item.ident == "rustix" {
            self.violations += 1;
        }
        visit::visit_item_extern_crate(self, item);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac
            .path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "rustix")
            || token_stream_has_rustix(&mac.tokens)
        {
            self.violations += 1;
        }
        visit::visit_macro(self, mac);
    }
}

fn is_geteuid(path: &RustPath) -> bool {
    let mut segments = path.segments.iter();
    path.segments.len() == 3
        && segments
            .next()
            .is_some_and(|segment| segment.ident == "rustix")
        && segments
            .next()
            .is_some_and(|segment| segment.ident == "process")
        && segments
            .next()
            .is_some_and(|segment| segment.ident == "geteuid")
        && path
            .segments
            .iter()
            .all(|segment| matches!(segment.arguments, syn::PathArguments::None))
}

fn is_process_path(path: &RustPath) -> bool {
    let mut segments = path.segments.iter();
    segments
        .next()
        .is_some_and(|segment| segment.ident == "rustix")
        && segments
            .next()
            .is_some_and(|segment| segment.ident == "process")
}

fn forbidden_rustix_import(tree: &UseTree, prefix: &[String]) -> bool {
    match tree {
        UseTree::Path(path) => {
            let mut next = prefix.to_vec();
            next.push(path.ident.to_string());
            forbidden_rustix_import(&path.tree, &next)
        }
        UseTree::Name(name) => {
            let mut next = prefix.to_vec();
            next.push(name.ident.to_string());
            rustix_import_outside_fs(&next)
        }
        UseTree::Rename(rename) => {
            let mut next = prefix.to_vec();
            next.push(rename.ident.to_string());
            next.first().is_some_and(|root| root == "rustix")
                && (next.len() == 1 || next.get(1).is_none_or(|module| module != "fs"))
        }
        UseTree::Group(group) => group
            .items
            .iter()
            .any(|item| forbidden_rustix_import(item, prefix)),
        UseTree::Glob(_) => {
            prefix.first().is_some_and(|root| root == "rustix")
                && prefix.get(1).is_none_or(|module| module != "fs")
        }
    }
}

fn rustix_import_outside_fs(path: &[String]) -> bool {
    path.first().is_some_and(|root| root == "rustix")
        && path.get(1).is_none_or(|module| module != "fs")
}

fn token_stream_has_rustix(tokens: &TokenStream) -> bool {
    tokens.clone().into_iter().any(|token| match token {
        TokenTree::Ident(identifier) => identifier == "rustix",
        TokenTree::Group(group) => token_stream_has_rustix(&group.stream()),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

fn rust_sources(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut pending = vec![root.to_path_buf()];
    let mut sources = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "rs")
            {
                sources.push(path);
            }
        }
    }
    sources.sort();
    Ok(sources)
}

fn audit_source(source: &str) -> Result<ProcessFeatureAudit, syn::Error> {
    let syntax = syn::parse_file(source)?;
    let mut audit = ProcessFeatureAudit::default();
    audit.visit_file(&syntax);
    Ok(audit)
}

fn is_owner_check_source(path: &Path) -> bool {
    path == Path::new(OWNER_CHECK_SOURCE)
}

#[test]
fn orchestrator_process_feature_is_limited_to_exact_stage_uid_check() -> Result<(), Box<dyn Error>>
{
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_root = crate_root.join("src");
    let mut uid_calls = 0;
    for path in rust_sources(&source_root)? {
        let source = std::fs::read_to_string(&path)?;
        let audit = audit_source(&source)?;
        assert_eq!(audit.violations, 0, "process API in {}", path.display());
        if audit.uid_calls > 0 {
            let relative_path = path.strip_prefix(&crate_root)?;
            assert!(
                is_owner_check_source(relative_path),
                "geteuid call outside stage owner check: {}",
                path.display()
            );
            uid_calls += audit.uid_calls;
        }
    }
    assert_eq!(uid_calls, 1, "exactly one caller UID lookup is required");
    Ok(())
}

#[test]
fn process_feature_guard_requires_exact_owner_source_path() {
    assert!(is_owner_check_source(Path::new(OWNER_CHECK_SOURCE)));
    assert!(!is_owner_check_source(Path::new(
        "src/nested/generate_stage_root_fs.rs"
    )));
}

#[test]
fn process_feature_guard_rejects_import_aliases_and_other_apis() -> Result<(), Box<dyn Error>> {
    let valid = audit_source("fn owner() { rustix::process::geteuid(); }")?;
    assert_eq!(valid.violations, 0);
    assert_eq!(valid.uid_calls, 1);

    for source in [
        "use rustix::process::geteuid; fn owner() { geteuid(); }",
        "use rustix as rx; fn owner() { rx::process::geteuid(); }",
        "fn owner() { rustix::process::getuid(); }",
    ] {
        let audit = audit_source(source)?;
        assert!(
            audit.violations > 0,
            "accepted process-feature source: {source}"
        );
    }
    Ok(())
}
