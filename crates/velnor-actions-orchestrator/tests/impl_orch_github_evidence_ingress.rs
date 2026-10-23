//! Production-source boundary for the trusted GitHub evidence reader.

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::{Error as IoError, ErrorKind};
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::ext::IdentExt;

use crate::impl_orch_f2a_offline::{
    production_source_of_text, production_src_files, production_src_files_from,
};

type AuditResult<T> = Result<T, Box<dyn Error>>;

#[path = "impl_orch_github_evidence_ingress_ast.rs"]
mod ast;
#[path = "impl_orch_github_evidence_ingress_tests.rs"]
mod tests;

fn source_identifiers(source: &str) -> AuditResult<Vec<String>> {
    let production = production_source_of_text(source)?;
    let tokens = production.parse::<TokenStream>()?;
    let mut identifiers = Vec::new();
    append_identifiers(tokens, &mut identifiers);
    Ok(identifiers)
}

fn append_identifiers(tokens: TokenStream, identifiers: &mut Vec<String>) {
    for token in tokens {
        match token {
            TokenTree::Ident(identifier) => identifiers.push(identifier.unraw().to_string()),
            TokenTree::Group(group) => append_identifiers(group.stream(), identifiers),
            TokenTree::Literal(_) | TokenTree::Punct(_) => {}
        }
    }
}

fn marker_files(root: &Path, files: &[PathBuf], marker: &str) -> AuditResult<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    for path in files {
        let source = fs::read_to_string(path)?;
        if source_identifiers(&source)?
            .iter()
            .any(|name| name == marker)
        {
            found.insert(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(found)
}

fn assert_marker_files(
    root: &Path,
    files: &[PathBuf],
    marker: &str,
    expected: &[&str],
) -> AuditResult<()> {
    let actual = marker_files(root, files, marker)?;
    let expected = expected.iter().map(|path| (*path).to_owned()).collect();
    if actual != expected {
        return Err(invalid(&format!(
            "{marker} appears in {actual:?}, expected {expected:?}"
        )));
    }
    Ok(())
}

fn invocation_from_env(syntax: &syn::File) -> AuditResult<&syn::ImplItemFn> {
    let mut methods = Vec::new();
    for item in &syntax.items {
        let syn::Item::Impl(implementation) = item else {
            continue;
        };
        let syn::Type::Path(self_type) = &*implementation.self_ty else {
            continue;
        };
        if implementation.trait_.is_some()
            || self_type.qself.is_some()
            || !self_type.path.is_ident("Invocation")
        {
            continue;
        }
        methods.extend(implementation.items.iter().filter_map(|item| match item {
            syn::ImplItem::Fn(method) if method.sig.ident == "from_env" => Some(method),
            _ => None,
        }));
    }
    if methods.len() != 1 {
        return Err(invalid("expected one Invocation::from_env method"));
    }
    Ok(methods[0])
}

pub(crate) fn assert_actual_acquisition_boundary() -> AuditResult<()> {
    let orch_root = crate::impl_orch_f2a::orch_src().canonicalize()?;
    let cli_root = orch_root
        .join("../../velnor-actions-cli/src")
        .canonicalize()
        .map_err(|error| invalid(&format!("CLI source root: {error}")))?;
    let orch_files = production_src_files(&orch_root)
        .map_err(|error| invalid(&format!("orchestrator production closure: {error}")))?;
    let cli_files = production_src_files_from(&cli_root.join("main.rs"), &cli_root, &cli_root)
        .map_err(|error| invalid(&format!("CLI production closure: {error}")))?;

    ast::validate_runtime_edges(&orch_root, &cli_root)
        .map_err(|error| invalid(&format!("runtime edge validation: {error}")))?;
    assert_orchestrator_marker_counts(&orch_root)?;
    assert_orchestrator_marker_files(&orch_root, &orch_files)?;
    assert_cli_marker_files(&cli_root, &cli_files)?;
    assert_renderer_dependencies_are_data_only(&orch_root)
        .map_err(|error| invalid(&format!("renderer dependency boundary: {error}")))
}

fn assert_orchestrator_marker_counts(root: &Path) -> AuditResult<()> {
    for (file, marker, expected) in [
        ("api.rs", "github_evidence", 1),
        ("api.rs", "github_evidence_http", 1),
        ("github_evidence.rs", "run_github_evidence_fetch", 1),
        ("github_evidence.rs", "run_github_evidence_import", 1),
        ("github_evidence_collect.rs", "github_evidence_http", 1),
        ("github_evidence_collect.rs", "GitHubEvidenceRead", 3),
        ("github_evidence_collect.rs", "NativeGitHubEvidenceRead", 2),
        (
            "github_evidence_collect_artifacts.rs",
            "github_evidence_http",
            1,
        ),
        (
            "github_evidence_collect_artifacts.rs",
            "GitHubEvidenceRead",
            2,
        ),
        (
            "github_evidence_collect_context.rs",
            "github_evidence_http",
            1,
        ),
        (
            "github_evidence_collect_workflows.rs",
            "github_evidence_http",
            1,
        ),
        (
            "github_evidence_collect_workflows.rs",
            "GitHubEvidenceRead",
            2,
        ),
        ("github_evidence_http.rs", "GitHubEvidenceRead", 2),
        ("github_evidence_http.rs", "NativeGitHubEvidenceRead", 2),
    ] {
        assert_source_marker_count(root, file, marker, expected)
            .map_err(|error| invalid(&format!("orchestrator {file} marker: {error}")))?;
    }
    Ok(())
}

fn assert_orchestrator_marker_files(root: &Path, files: &[PathBuf]) -> AuditResult<()> {
    for (marker, expected) in [
        ("github_evidence", &["api.rs"][..]),
        (
            "run_github_evidence_fetch",
            &["api.rs", "github_evidence.rs"][..],
        ),
        (
            "run_github_evidence_import",
            &["api.rs", "github_evidence.rs"][..],
        ),
        (
            "github_evidence_http",
            &[
                "api.rs",
                "github_evidence_collect.rs",
                "github_evidence_collect_artifacts.rs",
                "github_evidence_collect_context.rs",
                "github_evidence_collect_workflows.rs",
            ][..],
        ),
        (
            "GitHubEvidenceRead",
            &[
                "github_evidence_collect.rs",
                "github_evidence_collect_artifacts.rs",
                "github_evidence_collect_workflows.rs",
                "github_evidence_http.rs",
            ][..],
        ),
        (
            "NativeGitHubEvidenceRead",
            &["github_evidence_collect.rs", "github_evidence_http.rs"][..],
        ),
        ("ureq", &["github_evidence_http.rs"][..]),
    ] {
        assert_marker_files(root, files, marker, expected)
            .map_err(|error| invalid(&format!("orchestrator {marker} ingress: {error}")))?;
    }
    Ok(())
}

fn assert_cli_marker_files(root: &Path, files: &[PathBuf]) -> AuditResult<()> {
    for marker in ["run_github_evidence_fetch", "run_github_evidence_import"] {
        assert_marker_files(root, files, marker, &["dispatch_internal_run.rs"])
            .map_err(|error| invalid(&format!("CLI {marker} ingress: {error}")))?;
    }
    Ok(())
}

fn assert_renderer_dependencies_are_data_only(orch_root: &Path) -> AuditResult<()> {
    let manifest_path = orch_root.join("../../velnor-actions-workflow-renderer/Cargo.toml");
    let manifest: toml::Value = toml::from_str(&fs::read_to_string(manifest_path)?)?;
    let dependencies = manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid("renderer dependencies missing"))?;
    for forbidden in ["velnor-actions-orchestrator", "ureq"] {
        if dependencies.contains_key(forbidden) {
            return Err(invalid(&format!("renderer must not depend on {forbidden}")));
        }
    }
    Ok(())
}

fn assert_source_marker_count(
    root: &Path,
    relative: &str,
    marker: &str,
    expected: usize,
) -> AuditResult<()> {
    let source = fs::read_to_string(root.join(relative))?;
    let source = if relative == "api.rs" {
        crate::impl_orch_f2a::mask_api_reexport_for_legacy_scan(
            &root.join(relative),
            root,
            &source,
        )?
    } else {
        source
    };
    let actual = source_identifiers(&source)?
        .iter()
        .filter(|name| name == &marker)
        .count();
    if actual != expected {
        return Err(invalid(&format!(
            "{marker} appears {actual} times in {relative}, expected {expected}"
        )));
    }
    Ok(())
}

fn invalid(message: &str) -> Box<dyn Error> {
    IoError::new(ErrorKind::InvalidData, message.to_owned()).into()
}
