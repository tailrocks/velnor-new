// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use velnor_ci::report::check;
use velnor_ci::telemetry::INVOCATIONS;

#[test]
fn equivalent_implicit_and_explicit_targets_share_compilation_identity() -> super::ResultTest {
    for target in ["target", "cache-output"] {
        let result = check(&[observed_pair(target, "identical")?]);
        assert!(
            result.err().is_some_and(|error| error
                .to_string()
                .contains("identical observed compilation unit executed 2 times")),
            "{target}: repeated compilation was accepted"
        );
    }
    Ok(())
}

#[test]
fn compilation_identity_ignores_target_storage_and_retains_output_hash() -> super::ResultTest {
    for difference in ["storage", "hash"] {
        let result = check(&[observed_pair("target", difference)?]);
        if difference == "storage" {
            assert!(
                result.err().is_some_and(|error| error
                    .to_string()
                    .contains("identical observed compilation unit executed 2 times")),
                "repeated compilation in another target directory was accepted"
            );
        } else {
            assert!(result.is_ok(), "{difference}: {result:?}");
        }
    }
    Ok(())
}

#[test]
fn configured_target_parent_components_preserve_compilation_identity() -> super::ResultTest {
    for target in ["spare/../cache-output", "cache-output/../cache-output"] {
        let result = check(&[observed_pair(target, "identical")?]);
        assert!(
            result.as_ref().err().is_some_and(|error| error
                .to_string()
                .contains("identical observed compilation unit executed 2 times")),
            "{target}: configured target spelling concealed repeated compilation: {result:?}"
        );
    }
    Ok(())
}

#[test]
fn target_storage_at_checkout_root_preserves_source_identity() -> super::ResultTest {
    let result = check(&[observed_pair("target", "checkout-storage")?]);
    assert!(
        result.as_ref().err().is_some_and(|error| error
            .to_string()
            .contains("identical observed compilation unit executed 2 times")),
        "target storage at checkout root concealed repeated compilation: {result:?}"
    );
    Ok(())
}

fn observed_pair(
    target: &str,
    difference: &str,
) -> Result<velnor_ci::report::LaneMetrics, Box<dyn std::error::Error>> {
    let repo = super::fixture("report-equivalent-targets")?;
    if target != "target" {
        repo.write(
            ".cargo/config.toml",
            &format!("[build]\ntarget-dir='{target}'\n"),
        );
        velnor_gen::io::write_metadata(repo.root())?;
    }
    let root = fs::canonicalize(repo.root())?;
    let mut first = invocation(&root, target);
    let mut second = first.clone();
    let second_target = match difference {
        "identical" => target,
        "checkout-storage" => ".",
        _ => "other-target",
    };
    if difference != "identical" {
        first["target_directory"] = json!(root.join(target));
        first["argv"] = json!(["cargo", "check", "--target-dir", target]);
    }
    second["argv"] = json!(["cargo", "check", "--target-dir", second_target]);
    second["target_directory"] = json!(root.join(second_target));
    let filename = if difference == "hash" {
        "libdemo-other.rmeta"
    } else {
        "libdemo-hash.rmeta"
    };
    second["artifacts"][0]["filenames"] =
        json!([root.join(second_target).join("debug/deps").join(filename)]);
    let raw = format!("{first}\n{second}\n");
    let recorded = repo.root().join(INVOCATIONS);
    fs::write(&recorded, &raw)?;
    let metrics = super::report(
        &repo,
        &json!([]),
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
    )?;
    assert_eq!(metrics.invocations.len(), 2);
    assert_eq!(
        json!(metrics.invocations[0].target_directory),
        first["target_directory"]
    );
    assert_eq!(
        json!(metrics.invocations[1].target_directory),
        second["target_directory"]
    );
    assert_eq!(fs::read_to_string(recorded)?, raw);
    assert!(metrics.verification_errors.is_empty(), "{metrics:?}");
    Ok(metrics)
}

fn invocation(root: &Path, target: &str) -> Value {
    let mut invocation = crate::report::lane()["invocations"][0].clone();
    invocation["root"] = json!(root);
    invocation["argv"] = json!(["cargo", "check"]);
    invocation["compilations"] = json!([]);
    invocation["progress"] = json!([]);
    invocation["target_directory"] = json!(null);
    invocation["artifacts"] = json!([{
        "package_id":format!("path+file://{}#demo@0.1.0", root.display()),
        "manifest_path":root.join("Cargo.toml"),
        "target":{"name":"demo","kind":["lib"],"crate_types":["lib"],"src_path":root.join("src/lib.rs")},
        "profile":{"opt_level":"0","debuginfo":2,"debug_assertions":true,"overflow_checks":true,"test":false},
        "features":[],"filenames":[root.join(target).join("debug/deps/libdemo-hash.rmeta")],"fresh":false
    }]);
    invocation
}
