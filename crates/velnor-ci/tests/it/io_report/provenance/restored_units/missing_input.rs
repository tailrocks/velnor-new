// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

const MISSING_INPUT: &str = "00000000bad00001";
const KEPT_INPUT: &str = "00000000bad00002";
const IN_TARGET: &str = "00000000bad00003";

fn dep_info_unit(repo: &Repo, hash: &str, input: &str) {
    repo.write(
        &format!("target/debug/.fingerprint/serde-{hash}/lib-serde"),
        "{}\n",
    );
    repo.write(
        &format!("target/debug/deps/serde-{hash}.d"),
        &format!("target/debug/deps/libserde-{hash}.rmeta: src/lib.rs {input}\n"),
    );
}

fn restored_record(repo: &Repo) -> Result<String, Box<dyn std::error::Error>> {
    Ok(fs::read_to_string(
        repo.root().join("target").join(restored_units_file("")),
    )?)
}

#[test]
fn a_unit_whose_dep_info_names_a_missing_file_outside_target_is_a_base_gap_not_waste()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = fixture("report-restored-units-missing-input")?;
    let root = fs::canonicalize(repo.root())?;
    let outside = root.join("kept/private.rs");
    repo.write("kept/private.rs", "pub fn v() {}\n");
    let missing = format!("{}/store/out-dirs/v1/abc/private.rs", root.display());
    dep_info_unit(&repo, MISSING_INPUT, &missing);
    dep_info_unit(&repo, KEPT_INPUT, &outside.display().to_string());
    let inside = format!("{}/target/debug/build/serde/out/x.rs", root.display());
    dep_info_unit(&repo, IN_TARGET, &inside);
    restore(&repo);
    let record = restored_record(&repo)?;
    let lines: Vec<&str> = record.lines().collect();
    assert!(
        lines.contains(&format!("missing-input {MISSING_INPUT}").as_str()),
        "{record}"
    );
    assert!(!lines.contains(&MISSING_INPUT), "{record}");
    for held in [KEPT_INPUT, IN_TARGET] {
        assert!(lines.contains(&held), "{held}: {record}");
    }

    let gap = observation(&repo, &[], &[registry_artifact(&repo, MISSING_INPUT)?])?;
    let compilation = &gap.invocations[0].compilations[0];
    assert_eq!(compilation.restored, Some(false), "{gap:?}");
    assert!(compilation.missing_input, "{gap:?}");
    assert_eq!(gap.violations, [] as [String; 0]);

    for held in [KEPT_INPUT, IN_TARGET] {
        let metrics = observation(&repo, &[], &[registry_artifact(&repo, held)?])?;
        assert_eq!(metrics.invocations[0].compilations[0].restored, Some(true));
        assert!(
            metrics
                .violations
                .iter()
                .any(|line| line.contains("serde v1.0.0")),
            "{held}: {metrics:?}"
        );
    }
    Ok(())
}

const REGISTRY_SOURCE: &str = "00000000bad00004";
const GIT_CHECKOUT: &str = "00000000bad00005";
const DANGLING: &str = "00000000bad00006";

#[test]
fn sources_cargo_unpacks_again_do_not_make_a_unit_a_base_gap()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = fixture("report-restored-units-unpacked-sources")?;
    let root = repo.root().display().to_string();
    dep_info_unit(
        &repo,
        REGISTRY_SOURCE,
        &format!(
            "{root}/home/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serde-1.0.0/src/lib.rs"
        ),
    );
    dep_info_unit(
        &repo,
        GIT_CHECKOUT,
        &format!("{root}/home/.cargo/git/checkouts/dep-1a2b3c/0123abc/src/lib.rs"),
    );
    restore(&repo);
    let record = restored_record(&repo)?;
    let lines: Vec<&str> = record.lines().collect();
    for held in [REGISTRY_SOURCE, GIT_CHECKOUT] {
        assert!(lines.contains(&held), "{held}: {record}");
    }
    assert!(!record.contains("missing-input"), "{record}");
    Ok(())
}

#[test]
fn a_dep_info_input_that_is_a_dangling_symlink_is_missing() -> Result<(), Box<dyn std::error::Error>>
{
    let repo = fixture("report-restored-units-dangling")?;
    let link = repo.root().join("kept/link.rs");
    fs::create_dir_all(repo.root().join("kept"))?;
    std::os::unix::fs::symlink(repo.root().join("kept/gone.rs"), &link)?;
    dep_info_unit(&repo, DANGLING, &link.display().to_string());
    restore(&repo);
    let record = restored_record(&repo)?;
    assert!(
        record
            .lines()
            .any(|line| line == format!("missing-input {DANGLING}")),
        "{record}"
    );
    Ok(())
}

const SERDE_GAP: &str = "00000000bad00007";
const WRAPPER: &str = "00000000bad00008";
const UNRELATED: &str = "00000000bad00009";

fn registry_unit(
    repo: &Repo,
    package: &str,
    hash: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut value = artifact(
        repo,
        package,
        "lib",
        &format!("target/debug/deps/lib{package}-{hash}.rmeta"),
        false,
        false,
    )?;
    value["package_id"] = json!(format!(
        "registry+https://github.com/rust-lang/crates.io-index#{package}@1.0.0"
    ));
    Ok(value)
}

#[test]
fn a_restored_dependent_forced_to_rebuild_by_a_missing_input_gap_is_a_gap_not_waste()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = fixture("report-restored-units-cascade")?;
    let missing = format!("{}/store/out-dirs/v1/abc/private.rs", repo.root().display());
    dep_info_unit(&repo, SERDE_GAP, &missing);
    repo.write(
        &format!("target/debug/.fingerprint/wrapper-{WRAPPER}/lib-wrapper"),
        r#"{"deps":[[0,"serde",false,7621611783511691194]]}"#,
    );
    repo.write(
        &format!("target/debug/.fingerprint/unrelated-{UNRELATED}/lib-unrelated"),
        r#"{"deps":[]}"#,
    );
    restore(&repo);
    let record = restored_record(&repo)?;
    let lines: Vec<&str> = record.lines().collect();
    assert!(
        lines.contains(&format!("missing-input {SERDE_GAP}").as_str()),
        "{record}"
    );
    assert!(
        lines.contains(&format!("missing-input {WRAPPER}").as_str()),
        "{record}"
    );
    assert!(lines.contains(&UNRELATED), "{record}");

    let metrics = observation(
        &repo,
        &[],
        &[
            registry_unit(&repo, "serde", SERDE_GAP)?,
            registry_unit(&repo, "wrapper", WRAPPER)?,
            registry_unit(&repo, "unrelated", UNRELATED)?,
        ],
    )?;
    let compilation = |package: &str| {
        metrics.invocations[0]
            .compilations
            .iter()
            .find(|event| event.package == package)
            .unwrap_or_else(|| unreachable!("{package}: {metrics:?}"))
    };
    let wrapper = compilation("wrapper");
    assert_eq!(wrapper.restored, Some(false), "{metrics:?}");
    assert!(wrapper.missing_input, "{metrics:?}");
    assert_eq!(compilation("unrelated").restored, Some(true), "{metrics:?}");
    assert!(
        metrics
            .violations
            .iter()
            .all(|line| !line.contains("wrapper")),
        "{metrics:?}"
    );
    assert!(
        metrics
            .violations
            .iter()
            .any(|line| line.contains("unrelated v1.0.0")),
        "{metrics:?}"
    );
    Ok(())
}

const BUILD_SCRIPT_SIBLING: &str = "00000000bad0000a";

#[test]
fn a_build_script_sibling_of_the_same_package_does_not_hide_a_missing_input_gap()
-> Result<(), Box<dyn std::error::Error>> {
    let repo = fixture("report-restored-units-build-script-sibling")?;
    let missing = format!("{}/store/out-dirs/v1/abc/private.rs", repo.root().display());
    dep_info_unit(&repo, SERDE_GAP, &missing);
    repo.write(
        &format!("target/debug/.fingerprint/serde-{BUILD_SCRIPT_SIBLING}/run-build-script-build-script-build.json"),
        "{}\n",
    );
    repo.write(
        &format!("target/debug/.fingerprint/wrapper-{WRAPPER}/lib-wrapper"),
        r#"{"deps":[[0,"serde",false,7621611783511691194]]}"#,
    );
    restore(&repo);
    let record = restored_record(&repo)?;
    let lines: Vec<&str> = record.lines().collect();
    assert!(
        lines.contains(&format!("missing-input {WRAPPER}").as_str()),
        "{record}"
    );

    let metrics = observation(
        &repo,
        &[],
        &[
            registry_unit(&repo, "serde", SERDE_GAP)?,
            registry_unit(&repo, "wrapper", WRAPPER)?,
        ],
    )?;
    let wrapper = metrics.invocations[0]
        .compilations
        .iter()
        .find(|event| event.package == "wrapper")
        .unwrap_or_else(|| unreachable!("wrapper: {metrics:?}"));
    assert_eq!(wrapper.restored, Some(false), "{metrics:?}");
    assert!(wrapper.missing_input, "{metrics:?}");
    assert!(
        metrics
            .violations
            .iter()
            .all(|line| !line.contains("wrapper")),
        "{metrics:?}"
    );
    Ok(())
}
