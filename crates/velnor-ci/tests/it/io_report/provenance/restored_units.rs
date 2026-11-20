// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use velnor_ci::inputs::restored_units_file;

const HELD: &str = "0123456789abcdef";
const HELD_NEW_LAYOUT: &str = "fedcba9876543210";
const GAP: &str = "00000000deadbeef";

fn base(name: &str, layout: &str) -> Result<Repo, Box<dyn std::error::Error>> {
    let repo = fixture(name)?;
    let (path, held) = match layout {
        "old" => (
            format!("target/debug/.fingerprint/serde-{HELD}/lib-serde"),
            HELD,
        ),
        _ => (
            format!("target/debug/build/serde/{HELD_NEW_LAYOUT}/fingerprint/lib-serde"),
            HELD_NEW_LAYOUT,
        ),
    };
    repo.write(&path, "{}\n");
    restore(&repo);
    let record = fs::read_to_string(repo.root().join("target").join(restored_units_file("")))?;
    assert!(record.lines().any(|line| line == held), "{record}");
    Ok(repo)
}

fn registry_artifact(repo: &Repo, hash: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let mut value = artifact(
        repo,
        "serde",
        "lib",
        &format!("target/debug/deps/libserde-{hash}.rmeta"),
        false,
        false,
    )?;
    value["package_id"] =
        json!("registry+https://github.com/rust-lang/crates.io-index#serde@1.0.0");
    Ok(value)
}

#[test]
fn a_registry_unit_is_waste_only_when_the_restored_base_held_it_in_either_layout()
-> Result<(), Box<dyn std::error::Error>> {
    for (layout, held) in [("old", HELD), ("new", HELD_NEW_LAYOUT)] {
        let repo = base(&format!("report-restored-units-{layout}"), layout)?;
        let rebuilt = registry_artifact(&repo, held)?;
        let metrics = observation(&repo, &[], &[rebuilt])?;
        assert_eq!(metrics.invocations[0].compilations[0].restored, Some(true));
        assert!(
            metrics
                .violations
                .iter()
                .any(|line| line.contains("serde v1.0.0")),
            "{layout}: {metrics:?}"
        );

        let gap = repo.fork(&format!("report-restored-units-gap-{layout}"));
        let missing = registry_artifact(&gap, GAP)?;
        let metrics = observation(&gap, &[], &[missing])?;
        assert_eq!(metrics.invocations[0].compilations[0].restored, Some(false));
        assert_eq!(metrics.violations, [] as [String; 0], "{layout}");
    }
    Ok(())
}

#[test]
fn a_restore_that_left_no_record_keeps_every_dependency_compile_waste()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = base("report-restored-units-absent", "old")?;
    fs::remove_file(repo.root().join("target").join(restored_units_file("")))?;
    let missing = registry_artifact(&repo, GAP)?;
    let metrics = observation(&repo, &[], &[missing])?;
    assert_eq!(metrics.invocations[0].compilations[0].restored, None);
    assert!(
        metrics
            .violations
            .iter()
            .any(|line| line.contains("serde v1.0.0")),
        "{metrics:?}"
    );
    Ok(())
}
