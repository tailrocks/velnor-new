use std::error::Error;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::impl_common::TestResult;

use super::ast;

const API_EXPORT: &str = "pub use github_evidence::{GITHUB_EVIDENCE_FETCH_OP, GITHUB_EVIDENCE_IMPORT_OP, run_github_evidence_fetch, run_github_evidence_import};";

fn source_tree(files: &[(&str, &str)]) -> Result<(TempDir, PathBuf, Vec<PathBuf>), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("src");
    fs::create_dir(&root)?;
    let mut lib = String::new();
    for (index, (path, _)) in files.iter().enumerate() {
        let module = format!("child_{index}");
        writeln!(&mut lib, "#[path = {path:?}] mod {module};")?;
    }
    fs::write(root.join("lib.rs"), lib)?;
    for (path, source) in files {
        let file = root.join(path);
        let parent = file.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fixture path has no parent",
            )
        })?;
        fs::create_dir_all(parent)?;
        fs::write(file, source)?;
    }
    let root = root.canonicalize()?;
    let sources = super::production_src_files(&root)?;
    Ok((temp, root, sources))
}

fn evidence_use() -> &'static str {
    "pub use github_evidence::{GITHUB_EVIDENCE_FETCH_OP, GITHUB_EVIDENCE_IMPORT_OP, run_github_evidence_fetch, run_github_evidence_import};"
}

#[test]
fn api_mask_accepts_only_the_canonical_closed_export() -> TestResult {
    let (temp, root, _) = source_tree(&[("api.rs", API_EXPORT)])?;
    let api = root.join("api.rs");
    let source = "/* π */ pub use github_evidence::{\r\n    GITHUB_EVIDENCE_FETCH_OP,\r\n    GITHUB_EVIDENCE_IMPORT_OP,\r\n    run_github_evidence_fetch,\r\n    run_github_evidence_import,\r\n};\r\n";
    let masked = crate::impl_orch_f2a::mask_api_reexport_for_legacy_scan(&api, &root, source)?;
    assert_eq!(masked.len(), source.len());
    assert_eq!(masked.matches('\n').count(), source.matches('\n').count());
    for marker in [
        "GITHUB_EVIDENCE_FETCH_OP",
        "GITHUB_EVIDENCE_IMPORT_OP",
        "run_github_evidence_fetch",
        "run_github_evidence_import",
    ] {
        assert!(
            !super::source_identifiers(&masked)?
                .iter()
                .any(|name| name == marker)
        );
    }

    let other = temp.path().join("other");
    fs::create_dir(&other)?;
    let same_basename = other.join("api.rs");
    fs::write(&same_basename, "")?;
    assert_eq!(
        crate::impl_orch_f2a::mask_api_reexport_for_legacy_scan(
            &same_basename,
            &root,
            evidence_use(),
        )?,
        evidence_use()
    );
    Ok(())
}

#[test]
fn api_mask_preserves_surrounding_calls_and_literal_fetches() -> TestResult {
    let (_temp, root, _) = source_tree(&[("api.rs", API_EXPORT)])?;
    let api = root.join("api.rs");
    let source = format!(
        "fn before() {{ let _ = run_github_evidence_fetch; }}\n{API_EXPORT}\nfn after() {{ let _ = run_github_evidence_import; let _ = \"cargo fetch\"; }}\nmacro_rules! hidden {{ () => {{ r#run_github_evidence_import() }}; }}"
    );
    let masked = crate::impl_orch_f2a::mask_api_reexport_for_legacy_scan(&api, &root, &source)?;
    let identifiers = super::source_identifiers(&masked)?;
    assert!(
        identifiers
            .iter()
            .any(|name| name == "run_github_evidence_fetch")
    );
    assert!(
        identifiers
            .iter()
            .any(|name| name == "run_github_evidence_import")
    );
    assert!(
        crate::impl_orch_f2a::code_of_text(&masked)
            .iter()
            .any(|(_, line)| line.contains("cargo fetch"))
    );
    Ok(())
}

#[test]
fn api_mask_rejects_changed_visibility_names_aliases_and_globs() -> TestResult {
    let (_temp, root, _) = source_tree(&[("api.rs", API_EXPORT)])?;
    let api = root.join("api.rs");
    let cases = [
        API_EXPORT.replace("pub use", "use"),
        API_EXPORT.replace(
            "run_github_evidence_import};",
            "run_github_evidence_import, extra};",
        ),
        API_EXPORT.replace(
            "run_github_evidence_fetch,",
            "run_github_evidence_fetch as fetch,",
        ),
        "pub use github_evidence::*;".to_owned(),
        API_EXPORT.replace("run_github_evidence_fetch,", "r#run_github_evidence_fetch,"),
        API_EXPORT.replace("github_evidence::", "github_evidences::"),
    ];
    for changed in cases {
        assert!(
            crate::impl_orch_f2a::mask_api_reexport_for_legacy_scan(&api, &root, &changed).is_err(),
            "accepted altered API export: {changed}"
        );
    }
    Ok(())
}

#[test]
fn runtime_recognition_requires_canonical_entry_paths() -> TestResult {
    let (_temp, root, _) = source_tree(&[
        ("github_evidence.rs", ""),
        ("github_evidence_collect_context.rs", ""),
        ("other/github_evidence.rs", ""),
    ])?;
    assert!(crate::impl_orch_f2a::is_exact_runtime_source(
        &root.join("github_evidence.rs"),
        &root,
    )?);
    assert!(crate::impl_orch_f2a::is_exact_runtime_source(
        &root.join("github_evidence_collect_context.rs"),
        &root,
    )?);
    assert!(!crate::impl_orch_f2a::is_exact_runtime_source(
        &root.join("other/github_evidence.rs"),
        &root,
    )?);
    Ok(())
}

#[test]
fn wrapper_cannot_route_local_import_into_collection() -> TestResult {
    let good = r"
pub fn run_github_evidence_fetch() -> Result<(), E> { collect::run() }
pub fn run_github_evidence_import() -> Result<(), E> { import::run() }
";
    ast::validate_wrapper(good)?;
    let changed = good.replace("import::run()", "collect::run()");
    assert!(ast::validate_wrapper(&changed).is_err());
    Ok(())
}

#[test]
fn recursive_production_ingress_rejects_analysis_and_nested_edges() -> TestResult {
    let expected = &["wrapper.rs"][..];
    let cases: &[(&str, &str)] = &[
        (
            "generate.rs",
            "fn generate() { run_github_evidence_fetch(); }",
        ),
        (
            "internal_plan.rs",
            "use crate::api::run_github_evidence_fetch as fetch; fn plan() { fetch(); }",
        ),
        (
            "source_prep.rs",
            "fn prepare() { crate::api::run_github_evidence_fetch(); }",
        ),
        ("nested/root.rs", "include!(\"inner.rs\");"),
        (
            "fixture_tests.rs",
            "fn generated_test_file() { run_github_evidence_fetch(); }",
        ),
    ];
    for (path, source) in cases {
        let wrapper = ("wrapper.rs", "pub fn run_github_evidence_fetch() {} ");
        let mut files = vec![wrapper];
        files.push((path, source));
        if *path == "nested/root.rs" {
            files.push((
                "nested/inner.rs",
                "fn nested() { run_github_evidence_fetch(); }",
            ));
        }
        let (_temp, root, sources) = source_tree(&files)?;
        assert!(
            super::assert_marker_files(&root, &sources, "run_github_evidence_fetch", expected)
                .is_err(),
            "accepted acquisition edge in {path}"
        );
    }
    Ok(())
}

#[test]
fn recursive_authority_scan_rejects_adapter_and_native_client_escapes() -> TestResult {
    for (marker, expected, source) in [
        (
            "github_evidence_http",
            &["github_evidence_http.rs"][..],
            "use crate::api::github_evidence_http::GitHubEvidenceRead;",
        ),
        (
            "ureq",
            &["github_evidence_http.rs"][..],
            "fn escape() { let _ = ureq::Agent::new(); }",
        ),
    ] {
        let files = [
            ("github_evidence_http.rs", "pub struct Native;"),
            ("generate.rs", source),
        ];
        let (_temp, root, sources) = source_tree(&files)?;
        assert!(super::assert_marker_files(&root, &sources, marker, expected).is_err());
    }
    Ok(())
}

#[test]
fn actual_production_graph_keeps_acquisition_inside_the_trusted_ingress() -> TestResult {
    super::assert_actual_acquisition_boundary()?;
    Ok(())
}

#[test]
fn cli_dispatch_maps_fetch_and_import_to_their_exact_operations() -> TestResult {
    let cli = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../velnor-actions-cli/src");
    let dispatch = fs::read_to_string(cli.join("dispatch.rs"))?;
    let internal_run = fs::read_to_string(cli.join("dispatch_internal_run.rs"))?;
    let operation = fs::read_to_string(cli.join("dispatch/operation.rs"))?;
    ast::validate_cli_sources(&dispatch, &internal_run, &operation)?;
    let swapped = internal_run.replace(
        "InternalOp::GitHubEvidenceFetch => finish_internal_result(run_github_evidence_fetch()),",
        "InternalOp::GitHubEvidenceFetch => finish_internal_result(run_github_evidence_import()),",
    );
    assert_ne!(swapped, internal_run);
    assert!(ast::validate_cli_sources(&dispatch, &swapped, &operation).is_err());
    let extra = internal_run.replace(
        "fn run_internal",
        "fn extra() { let _ = run_github_evidence_fetch; }\nfn run_internal",
    );
    assert_ne!(extra, internal_run);
    assert!(ast::validate_cli_sources(&dispatch, &extra, &operation).is_err());

    let fetch_selector =
        "Ok(tag) if tag == GITHUB_EVIDENCE_FETCH_OP => InternalOp::GitHubEvidenceFetch,";
    let wrong_selector = operation.replace(
        fetch_selector,
        "Ok(tag) if tag == GITHUB_EVIDENCE_IMPORT_OP => InternalOp::GitHubEvidenceFetch,",
    );
    assert_ne!(wrong_selector, operation);
    assert!(ast::validate_cli_sources(&dispatch, &internal_run, &wrong_selector).is_err());

    let unguarded_selector = operation.replace(
        fetch_selector,
        "Ok(tag) => InternalOp::GitHubEvidenceFetch,",
    );
    assert_ne!(unguarded_selector, operation);
    assert!(ast::validate_cli_sources(&dispatch, &internal_run, &unguarded_selector).is_err());

    let guarded_dispatch = internal_run.replace(
        "InternalOp::GitHubEvidenceFetch => finish_internal_result(run_github_evidence_fetch()),",
        "InternalOp::GitHubEvidenceFetch if true => finish_internal_result(run_github_evidence_fetch()),",
    );
    assert_ne!(guarded_dispatch, internal_run);
    assert!(ast::validate_cli_sources(&dispatch, &guarded_dispatch, &operation).is_err());
    Ok(())
}

#[test]
fn generated_tree_scanner_does_not_hide_literal_fetch_verbs() -> TestResult {
    let source = "const NOTE: &str = \"cargo fetch\";";
    let (_temp, root, _sources) = source_tree(&[("api.rs", &format!("{API_EXPORT}\n{source}"))])?;
    let api = root.join("api.rs");
    let text = fs::read_to_string(&api)?;
    let masked = crate::impl_orch_f2a::mask_api_reexport_for_legacy_scan(&api, &root, &text)?;
    assert!(
        crate::impl_orch_f2a::code_of_text(&masked)
            .iter()
            .any(|(_, line)| line.contains("cargo fetch"))
    );
    Ok(())
}
