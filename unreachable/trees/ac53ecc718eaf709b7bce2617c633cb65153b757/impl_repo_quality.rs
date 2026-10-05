//! Quality-contract pins: spec §3 integration lives in QC; OC mirrors by ref.
//!
//! T26 keeps `docs/proposed/rust-quality-contract.md` (QC) as the detailed
//! owner of the spec §3 policy and reconciles the `opentofu-contract.md`
//! (OC) §§4.4–4.5 tofu mirrors by reference. These tests pin the §-level
//! integration map, the eight-crate fix, the homonym notes, the two
//! divergence resolutions, and the no-orphan rule (every original QC
//! normative anchor survives).

use std::error::Error;

use crate::impl_repo_deps::physical_lines;
use crate::impl_repo_policy::read;

/// QC anchors added by the T26 integration, one per integrated clause.
const INTEGRATION_ANCHORS: [&str; 9] = [
    "10 — One owner per",
    "11 — Never duplicate domain",
    "12 — No domain logic in rendering or transport",
    "behavior-preserving refactor",
    "golden parity",
    "KISS/YAGNI",
    "thiserror",
    "TryFrom",
    "regex parser for HCL",
];

/// Original QC normative anchors that MUST survive any condensation.
const ORIGINAL_ANCHORS: [&str; 13] = [
    "unsafe_code = \"forbid\"",
    "unwrap_used = \"deny\"",
    "too-many-lines-threshold = 80",
    "asamarts/alint@9f9d34ba0eae3888299b9e570f43338b0e7f2cdb",
    "MISE_RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "400 physical lines",
    "`cargo-metadata` edge test",
    "tooling-input-contract.md",
    "task-execution-contract.md",
    "agent-and-performance-contract.md",
    "version-policy.md",
    "150 physical lines",
];

/// OC reconciliation anchors: ownership pointers + divergence resolutions.
const OC_ANCHORS: [&str; 9] = [
    "detailed in `rust-quality-contract.md`",
    "ambient inheritance",
    "§4.4 approves generated values",
    "owns the Rust quality rules",
    "Compiler reuse stays under",
    "H1's denylist governs ambient inheritance",
    "tofu fmt once per scope",
    "Verification detail is owned by",
    "`rust-quality-contract.md` §9",
];

/// Quality-detail tokens that MUST NOT leak into the lean AGENTS.md.
const AGENTS_DETAIL_DENYLIST: [&str; 6] = [
    "too_many_lines",
    "clippy.toml",
    "RUSTUP_TOOLCHAIN",
    "unwrap_used",
    "MISE_RUSTUP_HOME",
    "actionlint@",
];

/// True when `body` reads as pointer-only agent instructions.
fn is_pointer_only(body: &str) -> bool {
    body.contains("docs/proposed") && !AGENTS_DETAIL_DENYLIST.iter().any(|t| body.contains(t))
}

#[test]
fn qc_integrates_spec_section3() -> Result<(), Box<dyn Error>> {
    let qc = read("docs/proposed/rust-quality-contract.md")?;
    for anchor in INTEGRATION_ANCHORS {
        assert!(qc.contains(anchor), "QC misses integrated clause {anchor}");
    }
    Ok(())
}

#[test]
fn qc_states_eight_member_workspace() -> Result<(), Box<dyn Error>> {
    let qc = read("docs/proposed/rust-quality-contract.md")?;
    for anchor in [
        "exactly the eight product package names",
        "The eight V1 crates",
    ] {
        assert!(qc.contains(anchor), "QC misses eight-crate fix {anchor}");
    }
    assert!(
        !qc.contains("seven"),
        "stale seven-crate claim survives in QC"
    );
    let tofu = qc.matches("velnor-actions-tofu").count();
    assert!(
        tofu >= 3,
        "QC names tofu only {tofu}x (diagram+table+members)"
    );
    Ok(())
}

#[test]
fn qc_disambiguates_homonyms() -> Result<(), Box<dyn Error>> {
    let qc = read("docs/proposed/rust-quality-contract.md")?;
    for anchor in [
        "cargo-fmt",
        "tofu fmt",
        "repository-structure alint",
        "workflow-syntax actionlint",
    ] {
        assert!(qc.contains(anchor), "QC misses homonym note {anchor}");
    }
    Ok(())
}

#[test]
fn qc_keeps_original_normatives() -> Result<(), Box<dyn Error>> {
    let qc = read("docs/proposed/rust-quality-contract.md")?;
    for anchor in ORIGINAL_ANCHORS {
        assert!(qc.contains(anchor), "condensation orphaned {anchor}");
    }
    let policy = qc.matches("version-policy.md").count();
    assert!(policy >= 2, "version-policy cited only {policy}x (need 2)");
    // Split companions keep their moved MUSTs verbatim; stubs point at them.
    for stub in ["rust-test-policy.md", "rust-dependency-policy.md"] {
        assert!(qc.contains(stub), "QC § stub misses {stub}");
    }
    let tests = read("docs/proposed/rust-test-policy.md")?;
    for anchor in ["--no-tests fail", "src/parser/tests.rs", "proptest"] {
        assert!(tests.contains(anchor), "test-policy orphaned {anchor}");
    }
    let deps = read("docs/proposed/rust-dependency-policy.md")?;
    for anchor in ["narrow features", "MUST reject yanked", "cargo machete"] {
        assert!(deps.contains(anchor), "dependency-policy orphaned {anchor}");
    }
    Ok(())
}

#[test]
fn oc_reconciles_mirrors_by_reference() -> Result<(), Box<dyn Error>> {
    let oc = read("docs/proposed/opentofu-contract.md")?;
    for anchor in OC_ANCHORS {
        assert!(oc.contains(anchor), "OC misses reconciliation {anchor}");
    }
    Ok(())
}

#[test]
fn agents_stays_lean_pointer_only() -> Result<(), Box<dyn Error>> {
    // The predicate is proven non-vacuous on synthetic bodies first.
    assert!(is_pointer_only("see docs/proposed for the rules"));
    assert!(!is_pointer_only("see docs/proposed; set too_many_lines"));
    assert!(!is_pointer_only("no pointer here"));
    let agents = read("AGENTS.md")?;
    assert!(
        physical_lines(&agents) <= 100,
        "AGENTS.md exceeds 100 lines"
    );
    assert!(agents.len() <= 16384, "AGENTS.md exceeds 16 KiB");
    assert!(is_pointer_only(&agents), "AGENTS.md carries quality detail");
    assert!(
        agents.contains("opentofu-contract.md"),
        "AGENTS.md misses the adopted-contract pointer"
    );
    Ok(())
}

#[test]
fn contracts_carry_no_fragment_links() -> Result<(), Box<dyn Error>> {
    for doc in [
        "docs/proposed/rust-quality-contract.md",
        "docs/proposed/opentofu-contract.md",
    ] {
        assert!(
            !read(doc)?.contains(".md#"),
            "{doc} gained a #fragment link"
        );
    }
    Ok(())
}
