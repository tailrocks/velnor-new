//! Freshness-procedure pins: boot equality, renovate, update rules.
//!
//! Covers BOOT-3.4, GAP-E.2, RQ-2.11, RQ-9.8, VER-0.1, VER-1.5, VER-1.7,
//! VER-2.27, VER-3.2, VER-3.3, VER-3.4, VER-3.7, VER-4.4. Mechanical halves
//! only; human residuals live in docs/implemented/release-gates.md.

use std::error::Error;

use crate::impl_repo_policy::{quoted_value, read};

#[path = "fixtures/p12_property.rs"]
mod p12_property;

#[test]
fn boot34_mise_version_matches_catalog() -> Result<(), Box<dyn Error>> {
    let pinned = read(".mise-version")?;
    let catalog = read("crates/adapters/velnor-actions-mise/src/catalog.rs")?;
    assert_eq!(
        pinned.trim(),
        quoted_value(&catalog, "MISE_VERSION")?.as_str()
    );
    let gates = read("docs/implemented/release-gates.md")?;
    assert!(
        gates.contains("BOOT-3.4"),
        "seed equality half must be recorded"
    );
    assert!(
        gates.contains(".velnor/generator.lock"),
        "lock side must name the seed-created lock"
    );
    Ok(())
}

#[test]
fn gape2_seed_rules_documented() -> Result<(), Box<dyn Error>> {
    let gates = read("docs/implemented/release-gates.md")?;
    assert!(gates.contains("BOOT-4.2"), "seed rule must be recorded");
    assert!(
        gates.contains("2 distinct admin approvals"),
        "seed must require two approvals"
    );
    assert!(
        gates.contains("sha256"),
        "seed must require a rebuild hash match"
    );
    assert!(
        gates.contains("trust-on-review"),
        "pre-seed trust rule must be marked"
    );
    assert!(
        gates.contains("SEPARATE reviewed change"),
        "post-seed lock updates need their own review"
    );
    Ok(())
}

#[test]
fn rq211_lock_staleness_probe() -> Result<(), Box<dyn Error>> {
    let script = read("scripts/check-freshness.sh")?;
    assert!(
        script.contains("lock-staleness"),
        "script must probe staleness"
    );
    assert!(
        script.contains("exact `=x.y.z` (VER-2.26)"),
        "direct deps must declare exact versions"
    );
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(
        procedure.contains("MUST NOT remain stale"),
        "lock must not stay stale when the build passes"
    );
    assert!(
        procedure.contains("lock-staleness"),
        "procedure must cite the mechanical probe"
    );
    Ok(())
}

#[test]
fn rq98_risk_triggers_documented() -> Result<(), Box<dyn Error>> {
    let mutants = read(".cargo/mutants.toml")?;
    assert!(
        mutants.contains("examine_globs"),
        "mutant scope must be set"
    );
    assert!(
        mutants.contains("crates/services/velnor-actions-orchestrator/src/select.rs"),
        "selection must be in mutant scope"
    );
    assert!(
        mutants.contains("NOT wired into CI"),
        "manual-only status must be explicit"
    );
    let triggers = read("docs/implemented/verification-triggers.md")?;
    for technique in [
        "Mutation testing",
        "Property testing",
        "Fuzzing",
        "Miri / Loom",
        "cargo-semver-checks",
    ] {
        assert!(triggers.contains(technique), "triggers miss {technique}");
    }
    assert!(
        triggers.contains("NOT behavioral evidence"),
        "coverage must not count as behavioral evidence"
    );
    assert!(
        triggers.contains("MUST be pinned"),
        "CI use must require pinned tool versions"
    );
    Ok(())
}

#[test]
fn ver01_policy_header() -> Result<(), Box<dyn Error>> {
    let policy = read(".velnor/version-policy.toml")?;
    for setting in [
        "schema = 1",
        "channel = \"stable\"",
        "check_interval_hours = 24",
        "max_exception_days = 14",
    ] {
        assert!(policy.contains(setting), "policy header misses {setting}");
    }
    Ok(())
}

#[test]
fn ver15_incompatible_is_migration() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(procedure.contains("VER-1.5"), "procedure must cite VER-1.5");
    assert!(
        procedure.contains("required migration"),
        "incompatible updates need a migration"
    );
    assert!(
        procedure.contains("never silently omitted"),
        "holds must never be silent"
    );
    assert!(
        read("docs/implemented/release-gates.md")?.contains("VER-1.5"),
        "human residual must be recorded"
    );
    Ok(())
}

#[test]
fn ver17_expedited_security_path() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(procedure.contains("VER-1.7"), "procedure must cite VER-1.7");
    assert!(
        procedure.contains("same-day"),
        "security path must be same-day"
    );
    assert!(
        procedure.contains("minimal scope"),
        "security path must be minimal-scope"
    );
    assert!(
        procedure.contains("full gate MUST"),
        "full gate must still pass before merge"
    );
    Ok(())
}

#[test]
fn ver227_renovate_proposes_all() -> Result<(), Box<dyn Error>> {
    let renovate = read("renovate.json")?;
    for token in [
        "\"cargo\"",
        "\"github-actions\"",
        "\"major\"",
        "needs-migration-review",
        "\"minor\"",
        "dependencyDashboard",
    ] {
        assert!(renovate.contains(token), "renovate.json misses {token}");
    }
    assert!(
        read("docs/implemented/update-procedure.md")?.contains("never merges"),
        "renovate must propose only, never merge"
    );
    Ok(())
}

#[test]
fn ver32_one_coherent_set() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(procedure.contains("VER-3.2"), "procedure must cite VER-3.2");
    assert!(
        procedure.contains("ONE change covering ALL"),
        "updates must ship as one coherent set"
    );
    Ok(())
}

#[test]
fn ver33_records_and_qualifies() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(procedure.contains("VER-3.3"), "procedure must cite VER-3.3");
    assert!(
        procedure.contains("timestamp + version delta"),
        "update set must record timestamp and delta"
    );
    assert!(
        procedure.contains("ONLY Velnor-owned pins and locks"),
        "refresh must stay Velnor-owned"
    );
    for gate in ["cargo fmt", "Clippy", "doctests", "cargo deny"] {
        assert!(procedure.contains(gate), "qual list misses {gate}");
    }
    Ok(())
}

#[test]
fn ver34_tool_files_untouched() -> Result<(), Box<dyn Error>> {
    let renovate = read("renovate.json")?;
    for file in ["mise.toml", "mise.lock", "rust-toolchain.toml"] {
        assert!(renovate.contains(file), "renovate must name {file}");
    }
    assert!(
        renovate.contains("\"enabled\": false"),
        "tool inputs must be disabled in renovate"
    );
    assert!(
        renovate.contains("\"matchManagers\": [\"mise\"]"),
        "tool-input guard must match the mise manager that owns mise.toml/mise.lock"
    );
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(procedure.contains("VER-3.4"), "procedure must cite VER-3.4");
    assert!(
        procedure.contains("MUST NOT edit `mise.toml`"),
        "procedure must forbid tool-file edits"
    );
    Ok(())
}

#[test]
fn ver37_merge_after_qual() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(
        procedure.contains("only after qualification passes"),
        "pins must merge only after qual"
    );
    assert!(
        procedure.contains("--locked"),
        "normal builds must use --locked"
    );
    assert!(
        procedure.contains("never resolve versions"),
        "normal builds must never resolve versions"
    );
    let gates = read("docs/implemented/release-gates.md")?;
    assert!(
        gates.contains("VER-3.7"),
        "protection residual must be recorded"
    );
    assert!(
        gates.contains("branch protection"),
        "residual must name branch protection"
    );
    Ok(())
}

#[test]
fn ver44_velnor_owned_refresh_only() -> Result<(), Box<dyn Error>> {
    let procedure = read("docs/implemented/update-procedure.md")?;
    assert!(procedure.contains("VER-4.4"), "procedure must cite VER-4.4");
    assert!(
        procedure.contains("ONLY Velnor-owned pins and locks"),
        "refresh must stay Velnor-owned"
    );
    assert!(
        procedure.contains("recommendations only"),
        "tool files get recommendations only"
    );
    assert!(
        read("docs/implemented/release-gates.md")?.contains("VER-4.4"),
        "human residual must be recorded"
    );
    Ok(())
}
