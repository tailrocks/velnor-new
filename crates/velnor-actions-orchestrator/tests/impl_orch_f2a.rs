//! F2 structural scans: spawning, paths, registries, suggestions.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::impl_common::TestResult;
use syn::spanned::Spanned;

#[path = "impl_orch_runtime_acquisition.rs"]
pub(crate) mod runtime_acquisition;

/// Orchestrator `src/` directory.
pub(crate) fn orch_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Runtime acquisition is recognized only at its two canonical entry files.
pub(crate) fn is_exact_runtime_source(
    path: &Path,
    src_root: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    let path = path.canonicalize()?;
    for name in ["github_evidence.rs", "github_evidence_collect_context.rs"] {
        if path == src_root.join(name).canonicalize()? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Mask only the exact public evidence re-export at canonical `src/api.rs`.
pub(crate) fn mask_api_reexport_for_legacy_scan(
    path: &Path,
    src_root: &Path,
    source: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    if path.canonicalize()? != src_root.join("api.rs").canonicalize()? {
        return Ok(source.to_owned());
    }
    let syntax = syn::parse_file(source)?;
    let candidates = syntax
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Use(item) if evidence_use_root(&item.tree) => Some(item),
            _ => None,
        })
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(invalid_evidence_source(
            "expected one evidence API re-export",
        ));
    }
    validate_evidence_reexport(candidates[0])?;
    mask_evidence_span(source, candidates[0].span()).map_err(Into::into)
}

fn evidence_use_root(tree: &syn::UseTree) -> bool {
    matches!(tree, syn::UseTree::Path(path) if path.ident == "github_evidence")
}

fn validate_evidence_reexport(item: &syn::ItemUse) -> Result<(), syn::Error> {
    let expected = [
        "GITHUB_EVIDENCE_FETCH_OP",
        "GITHUB_EVIDENCE_IMPORT_OP",
        "run_github_evidence_fetch",
        "run_github_evidence_import",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    let syn::UseTree::Path(root) = &item.tree else {
        return Err(syn::Error::new(item.span(), "evidence API root changed"));
    };
    let syn::UseTree::Group(group) = &*root.tree else {
        return Err(syn::Error::new(
            item.span(),
            "evidence API export must be grouped",
        ));
    };
    let names = group
        .items
        .iter()
        .map(|tree| match tree {
            syn::UseTree::Name(name) => Ok(name.ident.to_string()),
            _ => Err(syn::Error::new(
                tree.span(),
                "evidence API exports must be direct names",
            )),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if root.ident != "github_evidence"
        || !matches!(item.vis, syn::Visibility::Public(_))
        || !item.attrs.is_empty()
        || item.leading_colon.is_some()
        || names != expected
    {
        return Err(syn::Error::new(
            item.span(),
            "evidence API re-export changed",
        ));
    }
    Ok(())
}

fn mask_evidence_span(source: &str, span: proc_macro2::Span) -> Result<String, syn::Error> {
    let start = source_offset(source, span.start().line, span.start().column);
    let end = source_offset(source, span.end().line, span.end().column);
    if start >= end || end > source.len() {
        return Err(syn::Error::new(span, "invalid API re-export source span"));
    }
    let mut bytes = source.as_bytes().to_vec();
    for byte in &mut bytes[start..end] {
        if !matches!(*byte, b'\n' | b'\r') {
            *byte = b' ';
        }
    }
    String::from_utf8(bytes).map_err(|_| syn::Error::new(span, "API masking changed UTF-8"))
}

fn source_offset(source: &str, line: usize, column: usize) -> usize {
    let mut start = 0;
    for _ in 1..line {
        let Some(relative) = source.as_bytes()[start..]
            .iter()
            .position(|byte| *byte == b'\n')
        else {
            return source.len();
        };
        start += relative + 1;
    }
    let end = source[start..]
        .find('\n')
        .map_or(source.len(), |relative| start + relative);
    (start + column).min(end)
}

fn invalid_evidence_source(message: &str) -> Box<dyn std::error::Error> {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message.to_owned()).into()
}

/// Sorted production `.rs` files directly under `src/`.
///
/// Unit-test companion modules are `#[cfg(test)]`; their assertions and
/// diagnostics must not be mistaken for executable product behavior.
pub(crate) fn product_src_files() -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(orch_src())? {
        let path = entry?.path();
        let test_companion = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.contains("_test") || name.contains("_fixture"));
        if path.extension().is_some_and(|ext| ext == "rs") && !test_companion {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Strip a trailing `//` comment, ignoring `//` inside string literals.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = false;
    let mut escape = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escape {
            escape = false;
        } else if byte == b'\\' && quoted {
            escape = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'/' && bytes[index + 1] == b'/' && !quoted {
            return line[..index].trim_end();
        }
        index += 1;
    }
    line
}

/// Code lines of one file: `(number, code)` with comments stripped.
pub(crate) fn code_of(path: &Path) -> Result<Vec<(usize, String)>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(code_of_text(&text))
}

/// Code lines from source text, preserving `code_of`'s comment handling.
pub(crate) fn code_of_text(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .map(|(number, line)| (number + 1, strip_line_comment(line).to_owned()))
        .filter(|(_, line)| !line.trim().is_empty())
        .collect()
}

/// Every `name:line` holding `token` in orchestrator code.
pub(crate) fn token_hits(token: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut hits = Vec::new();
    for path in product_src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        for (line, code) in code_of(&path)? {
            if code.contains(token) {
                hits.push(format!("{name}:{line}"));
            }
        }
    }
    Ok(hits)
}

#[test]
fn orch_spawns_no_processes_and_confines_shell_wrappers() -> TestResult {
    for token in [
        "Command::new",
        "process::Command",
        ".spawn(",
        ".output(",
        "CommandExt",
        "tokio::process",
        "StdCommand",
    ] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token} spawns: {:?}",
            token_hits(token)?
        );
    }
    for hit in token_hits("std::process")? {
        let (name, line) = hit.split_once(':').unwrap_or(("", ""));
        let line: usize = line.parse().unwrap_or(0);
        let code = code_of(&orch_src().join(name))?;
        let body = code
            .iter()
            .find(|(number, _)| *number == line)
            .map(|(_, body)| body.clone())
            .unwrap_or_default();
        assert!(
            body.contains("std::process::id()") && !body.contains("Command"),
            "non-spawn std::process use only: {hit}: {body}"
        );
    }
    // P08 C4: source_prep.rs emits one fixed `sh -c` probe-and-fetch template
    // (metadata probe, offline skip, explicit miss fetch) over validated roots.
    // Step IR has no conditions, so the shell conditional is required; roots
    // are fail-closed validated, never arbitrary shell. P05 reports:
    // matrix_step.rs emits the fixed obligation report-capture wrappers
    // (`sh -c` over a joined validated argv plus the fixed helper call);
    // exit capture needs one shell step, and the joined argv plus helper
    // path are fixed generator values, never repository shell.
    let mut sh_files = std::collections::BTreeSet::new();
    for path in product_src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if matches!(
            name.as_str(),
            "validate_shell_yaml.rs" | "validate_shell_yaml_shell.rs"
        ) {
            let text = std::fs::read_to_string(&path)?;
            assert!(!text.contains("StepKind"));
            assert!(!text.contains("argv"));
            assert!(!text.contains("Vec<String>"));
            assert!(!text.contains("Command::"));
            assert!(!text.contains("std::process"));
            assert!(!text.contains("\"-c\""));
            assert!(!text.contains("\"-s\""));
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        if text.contains("\"sh\"") {
            sh_files.insert(name);
        }
    }
    assert_eq!(
        sh_files,
        std::collections::BTreeSet::from([
            // cache_keys.rs: the fixed Plan `Compute cache keys` script
            // (generator-owned digest commands over validated roots).
            "cache_keys.rs".to_owned(),
            // jactionlint_verify_step.rs: the fixed `Verify jactionlint`
            // script; its only inputs are catalog digests and the validated
            // run-time jactionlint version.
            "jactionlint_verify_step.rs".to_owned(),
            "matrix_step_reports.rs".to_owned(),
            // publish_steps.rs: `sh -c` wrappers around the fixed publication
            // scripts in the renderer (tag parsing, bundle packing, upload);
            // every interpolated value is a contract-validated id or path.
            "publish_steps.rs".to_owned(),
            "pins.rs".to_owned(),
            "qualify.rs".to_owned(),
            // rust_manifest_step.rs: the fixed `Verify Rust channel manifest`
            // script; its only inputs are catalog digests and the validated
            // run-time Rust version.
            "rust_manifest_step.rs".to_owned(),
            "source_prep.rs".to_owned(),
        ]),
        "fixed sh wrappers live in cache_keys/matrix_step_reports/pins/qualify/source_prep only"
    );
    Ok(())
}

#[path = "impl_orch_prepare.rs"]
mod prepare;

#[test]
fn v1_registers_three_stacks_and_detects_rust_and_tofu() {
    use velnor_actions_contract::VelnorConfig;
    use velnor_actions_orchestrator::decisions::{DetectorInfo, detector_registry};
    assert_eq!(VelnorConfig::REGISTERED_STACKS, &["mise", "rust", "tofu"]);
    assert_eq!(
        detector_registry(),
        vec![
            DetectorInfo {
                stack_id: "rust",
                schema: 1
            },
            DetectorInfo {
                stack_id: "tofu",
                schema: 1
            },
        ]
    );
}

#[test]
fn v1_creates_no_generated_task_dirs() -> TestResult {
    let mut holders = std::collections::BTreeSet::new();
    for path in product_src_files()? {
        let text = std::fs::read_to_string(&path)?;
        if text.contains(".mise/tasks") {
            holders.insert(
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            );
        }
    }
    assert_eq!(
        holders,
        std::collections::BTreeSet::from(["evidence.rs".to_owned()])
    );
    for path in product_src_files()? {
        for (line, code) in code_of(&path)? {
            if code.contains("create_dir") {
                assert!(
                    !code.contains(".mise") && !code.contains("tasks"),
                    "task-dir creation at {}:{line}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn suggestions_are_never_executed() -> TestResult {
    for token in [
        "apply_suggestion",
        "write_suggestion",
        "execute_suggestion",
        "run_suggestion",
        "suggestion_argv",
    ] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token}: {:?}",
            token_hits(token)?
        );
    }
    Ok(())
}
