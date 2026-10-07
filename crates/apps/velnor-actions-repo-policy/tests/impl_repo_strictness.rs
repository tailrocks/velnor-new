//! Repo strictness pins: P11 validated construction and error taxonomy.
//!
//! Split from `impl_repo_policy.rs` by responsibility: policy owns repo
//! configuration shape, this module owns negative/behavioral enforcement
//! (forbidden groups, alint verdicts, unsafe absence) plus the P11-3/4/5
//! structural guarantees — validated newtypes, constructor-only proofs,
//! untrusted-document bounds, and distinct error variants.

use std::error::Error;

use crate::impl_repo_policy::{MEMBERS, alint_miniyaml, manifest, p11_alint, read, tree_files};

#[test]
fn no_restriction_or_nursery_groups() -> Result<(), Box<dyn Error>> {
    for dir in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("restriction"), "{dir}");
        assert!(!body.contains("nursery"), "{dir}");
    }
    let root = read("Cargo.toml")?;
    assert!(!root.contains("restriction"));
    assert!(!root.contains("nursery"));
    Ok(())
}

#[test]
fn no_nightly_toolchain() -> Result<(), Box<dyn Error>> {
    for file in [
        "Cargo.toml",
        "mise.toml",
        "crates/adapters/velnor-actions-mise-catalog/src/catalog.rs",
        ".github/workflows/ci.yml",
    ] {
        assert!(!read(file)?.to_lowercase().contains("nightly"), "{file}");
    }
    for dir in MEMBERS {
        assert!(!manifest(dir)?.to_lowercase().contains("nightly"), "{dir}");
    }
    Ok(())
}

#[test]
fn no_git_dependencies() -> Result<(), Box<dyn Error>> {
    for dir in MEMBERS {
        let body = manifest(dir)?;
        assert!(!body.contains("git="), "{dir}");
        assert!(!body.contains("git ="), "{dir}");
    }
    let lock = read("Cargo.lock")?;
    assert!(!lock.contains("git+"), "lockfile has git source");
    Ok(())
}

#[test]
fn alint_rule_fixtures_pass_fail_and_express_command() -> Result<(), Box<dyn Error>> {
    for row in &alint_miniyaml::EXPECTED {
        let id = row.id;
        let pass = alint_miniyaml::parse(&alint_miniyaml::fixture(id, "pass")?)?;
        alint_miniyaml::check_policy(&pass).map_err(|err| format!("{id} pass: {err}"))?;
        let fail = alint_miniyaml::parse(&alint_miniyaml::fixture(id, "fail")?)?;
        let failed = alint_miniyaml::check_policy(&fail).is_err();
        assert!(failed, "{id} fail passed");
    }
    let extra = alint_miniyaml::parse(&alint_miniyaml::fixture("command", "expressible")?)?;
    let added = alint_miniyaml::rule(&extra, "example-toml-edition-rule").ok_or("added rule")?;
    alint_miniyaml::check_rule_shape(added)?;
    let rejected = alint_miniyaml::check_policy(&extra).is_err();
    assert!(rejected, "unknown id passed");
    Ok(())
}

#[test]
fn alint_comment_only_edits_keep_verdicts() -> Result<(), Box<dyn Error>> {
    let live = read(".alint.yml")?;
    let wrapped = format!("# p\n{live}\n# p\n");
    p11_alint::check_extended_policy(&wrapped)?;
    for row in &alint_miniyaml::EXPECTED {
        let body = format!("# p\n{}", alint_miniyaml::fixture(row.id, "pass")?);
        alint_miniyaml::check_policy(&alint_miniyaml::parse(&body)?)?;
    }
    Ok(())
}

#[test]
fn unsafe_forbidden_and_absent() -> Result<(), Box<dyn Error>> {
    assert!(read("Cargo.toml")?.contains("unsafe_code = \"forbid\""));
    let spellings = [
        "unsafe {",
        "unsafe{",
        "unsafe fn",
        "unsafe impl",
        "unsafe trait",
        "unsafe extern",
        "#[unsafe",
    ];
    for dir in MEMBERS {
        for path in tree_files(&format!("{dir}/src"), "rs")? {
            let body = std::fs::read_to_string(&path)?;
            for spelling in spellings {
                assert!(
                    !body.contains(spelling),
                    "{} has {spelling}",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

#[test]
fn strictness_identifiers_are_newtypes() -> Result<(), Box<dyn Error>> {
    let ids = read("crates/core/velnor-actions-contract/src/ids/mod.rs")?;
    for name in [
        "RunKey",
        "ManifestKey",
        "TaskId",
        "MatrixId",
        "MatrixKey",
        "PlanId",
        "ReportId",
        "TaskReportId",
        "ArtifactId",
        "TargetKey",
    ] {
        assert!(
            ids.contains(&format!("id_newtype!({name},")),
            "{name} unregistered"
        );
    }
    for marker in [
        "pub struct $name(String);",
        "pub fn parse(value: &str)",
        "pub fn into_inner(self)",
        "#[serde(try_from = \"String\")]",
    ] {
        assert!(ids.contains(marker), "newtype macro misses {marker}");
    }
    assert!(
        ids.contains("fn validate_manifest_key"),
        "manifest key unvalidated"
    );
    let artifact = read("crates/core/velnor-actions-contract/src/ids/artifact.rs")?;
    assert!(
        artifact.contains("fn validate_target_key"),
        "target key unvalidated"
    );
    let canonical = read("crates/core/velnor-actions-contract/src/canonical.rs")?;
    for name in ["Digest", "PosixPath"] {
        assert!(
            canonical.contains(&format!("pub struct {name}(String);")),
            "{name} lacks a private newtype"
        );
    }
    assert!(
        canonical.contains("fn digest_b3_typed"),
        "untyped digest producer"
    );
    Ok(())
}

#[test]
fn strictness_proofs_need_validating_constructors() -> Result<(), Box<dyn Error>> {
    let baseline = read("crates/core/velnor-actions-contract-workflow/src/workflow/baseline.rs")?;
    for marker in [
        "pub fn new(",
        "pub fn used(",
        "pub fn unavailable(",
        "pub fn mark_unavailable(",
        "BaselineProofUnchecked",
        "ManifestTaskProofUnchecked",
        "PlanBaselineUnchecked",
    ] {
        assert!(baseline.contains(marker), "baseline.rs misses {marker}");
    }
    for field in [
        "pub source_commit",
        "pub run_id",
        "pub artifact_id",
        "pub artifact_name",
        "pub manifest_digest",
        "pub status",
        "pub base_commit",
        "pub reason",
        "pub task_id",
        "pub profile",
        "pub proof_run_id",
    ] {
        assert!(!baseline.contains(field), "forgable field {field}");
    }
    let cache_ids = read("crates/core/velnor-actions-contract-workflow/src/workflow/cache_ids.rs")?;
    assert!(
        cache_ids.contains("pub fn new("),
        "cache ids lack a constructor"
    );
    assert!(!cache_ids.contains("pub workspace_id"), "forgable cache id");
    Ok(())
}

#[test]
fn strictness_no_proof_literals_anywhere() -> Result<(), Box<dyn Error>> {
    let mut files = Vec::new();
    for dir in [
        "crates/core/velnor-actions-contract",
        "crates/core/velnor-actions-contract-config",
        "crates/core/velnor-actions-contract-planning",
        "crates/core/velnor-actions-contract-release",
        "crates/core/velnor-actions-contract-workflow",
    ] {
        files.extend(tree_files(&format!("{dir}/src"), "rs")?);
        files.extend(tree_files(&format!("{dir}/tests"), "rs")?);
    }
    for dir in [
        "crates/services/velnor-actions-orchestrator",
        "crates/services/velnor-actions-orchestrator-check-acquisition",
        "crates/services/velnor-actions-orchestrator-check-evidence",
        "crates/services/velnor-actions-orchestrator-check-preparation",
        "crates/services/velnor-actions-orchestrator-core",
        "crates/services/velnor-actions-orchestrator-discovery",
        "crates/services/velnor-actions-orchestrator-graph",
        "crates/services/velnor-actions-orchestrator-pins",
        "crates/services/velnor-actions-orchestrator-provisioning",
        "crates/services/velnor-actions-orchestrator-selection",
    ] {
        files.extend(tree_files(&format!("{dir}/src"), "rs")?);
        files.extend(tree_files(&format!("{dir}/tests"), "rs")?);
    }
    for name in [
        "BaselineProof",
        "ManifestTaskProof",
        "PlanBaseline",
        "EntryCacheIds",
    ] {
        let literal = format!("{name} {{");
        for path in &files {
            let body = std::fs::read_to_string(path)?;
            for (index, line) in body.lines().enumerate() {
                // A `-> Type {` function signature binds the return type; only
                // expression-position literals forge proofs.
                let signature = line
                    .find("->")
                    .is_some_and(|arrow| line.find(&literal).is_some_and(|hit| arrow < hit));
                let innocent = line.contains("pub struct")
                    || line.contains("impl ")
                    || line.contains("for ")
                    || line.contains("Unchecked")
                    || signature
                    || line.trim_start().starts_with("//");
                assert!(
                    innocent || !line.contains(&literal),
                    "{}:{} forges {name}",
                    path.display(),
                    index + 1
                );
            }
        }
    }
    Ok(())
}

#[test]
fn strictness_resource_bounds_reject_zero() -> Result<(), Box<dyn Error>> {
    let graph = read("crates/core/velnor-actions-contract-planning/src/graph.rs")?;
    for name in ["CpuMilli", "MemoryMb"] {
        assert!(
            graph.contains(&format!("pub struct {name}(u32);")),
            "{name} lacks a private newtype"
        );
    }
    assert!(graph.contains("zero_bound"), "zero bound accepted");
    assert!(
        graph.contains("pub cpu_milli: Option<CpuMilli>"),
        "raw cpu bound"
    );
    assert!(
        graph.contains("pub memory_mb: Option<MemoryMb>"),
        "raw memory bound"
    );
    Ok(())
}

#[test]
fn strictness_documents_have_size_bound() -> Result<(), Box<dyn Error>> {
    let strict = read("crates/core/velnor-actions-contract/src/strict_json.rs")?;
    for marker in [
        "MAX_UNTRUSTED_DOCUMENT_BYTES",
        "fn check_document_size",
        "fn parse_strict_json_with_limit",
        "fn parse_strict_json_bytes",
    ] {
        assert!(strict.contains(marker), "strict_json.rs misses {marker}");
    }
    let errors = read("crates/core/velnor-actions-contract/src/errors.rs")?;
    assert!(errors.contains("DocumentTooLarge"), "no oversize variant");
    let manifest = read("crates/core/velnor-actions-contract-release/src/manifest.rs")?;
    assert!(
        manifest.contains("fn parse_json_with_limit"),
        "no manifest override"
    );
    Ok(())
}

#[test]
fn strictness_matrix_agreement_uses_strict_boundary() -> Result<(), Box<dyn Error>> {
    let artifacts = read("crates/core/velnor-actions-contract-workflow/src/workflow/artifacts.rs")?;
    assert!(
        artifacts.contains("parse_strict_json_bytes"),
        "lenient matrix parse"
    );
    assert!(
        !artifacts.contains("from_slice"),
        "raw serde bypass remains"
    );
    Ok(())
}

#[test]
fn strictness_orchestrator_errors_stay_distinct() -> Result<(), Box<dyn Error>> {
    let source = read("crates/services/velnor-actions-orchestrator-core/src/error.rs")?;
    for marker in [
        "Cancelled {",
        "Unsupported {",
        "cancelled: {operation}: {detail}",
        "unsupported: {capability}: {detail}",
        "pub fn cancelled(",
        "pub fn unsupported(",
        "pub fn is_cancelled(",
        "pub fn is_unsupported(",
        "UnsupportedSchema",
    ] {
        assert!(source.contains(marker), "error.rs misses {marker}");
    }
    Ok(())
}

#[test]
fn strictness_negative_coverage_pinned() -> Result<(), Box<dyn Error>> {
    let cases = [
        (
            "crates/core/velnor-actions-contract/src/ids/artifact/tests/newtypes.rs",
            "newtypes_accept_valid_and_reject_invalid",
        ),
        (
            "crates/core/velnor-actions-contract/src/ids/artifact/tests/newtypes.rs",
            "digest_and_path_newtypes_validate",
        ),
        (
            "crates/core/velnor-actions-contract/src/strict_json/tests.rs",
            "oversize_doc_fails_with_size_detail",
        ),
        (
            "crates/core/velnor-actions-contract/src/strict_json/tests.rs",
            "bytes_entry_rejects_bad_utf8_and_dup_keys",
        ),
        (
            "crates/core/velnor-actions-contract/src/strict_json/tests.rs",
            "nesting_boundary_matches_serde_json",
        ),
        (
            "crates/core/velnor-actions-contract/src/strict_json/tests.rs",
            "nesting_budget_counts_objects_and_mixed_shapes",
        ),
        (
            "crates/core/velnor-actions-contract-workflow/src/workflow/artifacts/tests.rs",
            "agreement_rejects_duplicate_keys",
        ),
        (
            "crates/core/velnor-actions-contract-workflow/src/workflow/cache_ids/tests.rs",
            "cache_ids_need_five_valid_digests",
        ),
        (
            "crates/core/velnor-actions-contract/src/ids/artifact/tests/proofs.rs",
            "proof_constructor_validates_every_input",
        ),
        (
            "crates/core/velnor-actions-contract/src/ids/artifact/tests/proofs.rs",
            "baseline_states_are_exhaustive",
        ),
        (
            "crates/core/velnor-actions-contract/src/ids/artifact/tests/proofs.rs",
            "task_proof_needs_valid_ids_and_digests",
        ),
        (
            "crates/services/velnor-actions-orchestrator-core/src/error.rs",
            "cancelled_and_unsupported_stay_distinct",
        ),
        (
            "crates/services/velnor-actions-orchestrator-core/src/error.rs",
            "unsupported_schema_maps_to_unsupported",
        ),
    ];
    for (file, test) in cases {
        assert!(read(file)?.contains(test), "{file} misses {test}");
    }
    Ok(())
}
