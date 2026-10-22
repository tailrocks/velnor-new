//! Quality-contract pins: QC and its verification extension own detail; OC mirrors by ref.
//!
//! T26 keeps `docs/proposed/rust-quality-contract.md` (QC) as the detailed
//! owner of the spec §3 policy, with verification detail in its linked
//! extension, and reconciles the `opentofu-contract.md`
//! (OC) §§4.4–4.5 tofu mirrors by reference. These tests pin the §-level
//! integration map, the eight-crate fix, the homonym notes, the two
//! divergence resolutions, and the no-orphan rule (every original QC
//! normative anchor survives).

use std::error::Error;

use crate::impl_cli_tmp::{cleanup, code, fresh_tempdir, git_fixture, git_init, spawn};
use crate::impl_repo_policy::read;
use crate::impl_repo_size::physical_lines;

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
    "full-SHA pin (repository-structure alint, distinct",
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
    "`rust-verification-contract.md`",
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

/// Read the current pin through the public generator's repository-policy output.
fn generated_alint_pin() -> Result<String, Box<dyn Error>> {
    let tmp = fresh_tempdir("quality-alint-pin")?;
    let result = (|| {
        let repo = tmp.join("repo");
        std::fs::create_dir(&repo)?;
        git_init(&repo)?;
        std::fs::create_dir(repo.join(".velnor"))?;
        std::fs::write(
            repo.join(".velnor/config.toml"),
            "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"velnor-repository-v1\"\ndefault_branch = \"main\"\n",
        )?;
        let remote = git_fixture::command(&repo)?
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ])
            .output()?;
        assert!(remote.status.success(), "remote fixture failed: {remote:?}");
        let preview = tmp.join("preview");
        let generated = spawn(
            &[
                "generate",
                "--output-dir",
                preview.to_str().ok_or("preview path")?,
            ],
            &[],
            &repo,
        )?;
        assert_eq!(code(&generated), 0, "generation failed: {generated:?}");
        let yaml = std::fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
        alint_pin(&yaml)
    })();
    cleanup(&tmp);
    result
}

/// Keep the ref and version bound to the same uniquely emitted action step.
fn alint_pin(yaml: &str) -> Result<String, Box<dyn Error>> {
    let lines: Vec<_> = yaml.lines().collect();
    let matches: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("        uses: asamarts/alint@"))
        .collect();
    let [(index, uses)] = matches.as_slice() else {
        return Err("expected exactly one generated Alint step".into());
    };
    let sha = uses
        .trim()
        .strip_prefix("uses: asamarts/alint@")
        .ok_or("Alint ref")?;
    assert!(
        sha.len() == 40
            && sha
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    let step: Vec<_> = lines
        .iter()
        .skip(*index + 1)
        .take_while(|line| line.is_empty() || line.starts_with("        "))
        .collect();
    let mappings: Vec<_> = step
        .iter()
        .enumerate()
        .filter(|(_, line)| line.as_bytes() == b"        with:")
        .map(|(index, _)| index)
        .collect();
    let [with] = mappings.as_slice() else {
        return Err("expected exactly one with mapping in the generated Alint step".into());
    };
    let versions: Vec<_> = step
        .iter()
        .skip(*with + 1)
        .take_while(|line| line.is_empty() || line.starts_with("          "))
        .filter_map(|line| line.strip_prefix("          version: v"))
        .collect();
    let [version] = versions.as_slice() else {
        return Err("expected exactly one version in the generated Alint step".into());
    };
    let parts: Vec<_> = version.split('.').collect();
    assert!(
        parts.len() == 3
            && parts
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    );
    Ok(format!("- {} # v{version}", uses.trim()))
}

#[test]
fn qc_integrates_spec_section3() -> Result<(), Box<dyn Error>> {
    let qc = read("docs/proposed/rust-quality-contract.md")?;
    let verification = read("docs/proposed/rust-verification-contract.md")?;
    assert!(
        qc.contains("rust-verification-contract.md"),
        "QC §9 must link its binding verification extension"
    );
    for anchor in INTEGRATION_ANCHORS {
        assert!(
            qc.contains(anchor) || verification.contains(anchor),
            "quality-contract set misses integrated clause {anchor}"
        );
    }
    Ok(())
}

#[test]
fn qc_states_eight_member_workspace() -> Result<(), Box<dyn Error>> {
    let qc = read("docs/proposed/rust-quality-contract.md")?;
    for anchor in [
        "exactly the eight product package names",
        "The eight V1 product crates",
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
        "workflow-syntax jactionlint",
    ] {
        assert!(qc.contains(anchor), "QC misses homonym note {anchor}");
    }
    Ok(())
}

#[test]
fn qc_keeps_original_normatives() -> Result<(), Box<dyn Error>> {
    let fixture = format!(
        "        uses: asamarts/alint@{}\n        with:\n          version: v1.2.3\n",
        "a".repeat(40)
    );
    assert!(alint_pin(&fixture).is_ok());
    for invalid in [
        fixture.replace("        with:", "        env:"),
        fixture.replace("        with:\n", ""),
        format!("{fixture}          version: v1.2.3\n"),
        format!("{fixture}        with:\n          version: v1.2.3\n"),
        fixture.replace(
            "        with:\n          version: v1.2.3",
            "        with:\n          path: .\n        env:\n          version: v1.2.3",
        ),
    ] {
        assert!(
            alint_pin(&invalid).is_err(),
            "accepted invalid Alint step: {invalid}"
        );
    }
    let qc = read("docs/proposed/rust-quality-contract.md")?;
    let verification = read("docs/proposed/rust-verification-contract.md")?;
    for anchor in ORIGINAL_ANCHORS {
        assert!(
            qc.contains(anchor) || verification.contains(anchor),
            "condensation orphaned {anchor}"
        );
    }
    let alint = generated_alint_pin()?;
    assert!(
        qc.lines()
            .chain(verification.lines())
            .any(|line| line == alint),
        "QC must document the exact current reviewed action: {alint}"
    );
    assert!(
        qc.contains("Alint MUST run as its own job"),
        "QC must retain the dedicated Alint job requirement"
    );
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
