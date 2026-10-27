// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t33_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_lane_classification()?;
    check_serialized_order()?;
    check_group_plan()?;
    check_exact_commands_preserved()?;
    Ok(())
}

fn check_rejects_bad_inputs() -> Result<(), Box<dyn Error>> {
    let profile = fixture_profile();
    let features = fixture_features();
    for manifest in [
        "",
        "/abs/Cargo.toml",
        "../evil/Cargo.toml",
        "crates/demo/README.md",
    ] {
        let package = fixture_package(manifest);
        must_fail!(
            derive_task_groups(&DeriveInputs {
                package: &package,
                profile: &profile,
                configuration: "default",
                features: &features,
                all_features: false,
                clippy_all_features: false,
                target: "host",
                explicit_fmt: false,
                allow_empty_test_suite: false,
            }),
            "bad manifest accepted",
        );
    }
    let package = fixture_package("crates/demo/Cargo.toml");
    let group = derive_task_groups(&DeriveInputs {
        package: &package,
        profile: &profile,
        configuration: "default",
        features: &features,
        all_features: false,
        clippy_all_features: false,
        target: "host",
        explicit_fmt: false,
        allow_empty_test_suite: false,
    })
    .map_err(|err| fail(&format!("derive failed: {err}")))?
    .into_iter()
    .next()
    .ok_or_else(|| fail("missing clippy group"))?;
    let mut dashed = group.clone();
    dashed.features = vec!["-evil".to_owned()];
    must_fail!(cargo_payload_argv(&dashed), "leading-dash feature accepted");
    dashed = group.clone();
    "-x".clone_into(&mut dashed.target);
    must_fail!(cargo_payload_argv(&dashed), "leading-dash target accepted");
    dashed = group.clone();
    "-evil".clone_into(&mut dashed.package_name);
    must_fail!(cargo_payload_argv(&dashed), "leading-dash package accepted");
    must_fail!(
        group.clone().with_declared_inputs(&["/abs".to_owned()]),
        "absolute input"
    );
    must_fail!(
        group.clone().with_declared_inputs(&[String::new()]),
        "empty input"
    );
    must_fail!(
        group.clone().with_declared_inputs(&["a/../b".to_owned()]),
        "parent input",
    );
    Ok(())
}

fn check_rejects_unknown_kinds() -> Result<(), Box<dyn Error>> {
    for kind in ["", "bogus", "BUILD", "clippy "] {
        if runs_before_parallel_checks(kind) {
            return Err(fail("unknown kind treated as a pre-pass"));
        }
        if concurrent_target_lane(kind).is_some() {
            return Err(fail("unknown kind treated as an isolated lane"));
        }
        if task_kind_rank(kind) != u32::MAX {
            return Err(fail("unknown kind treated as a known lane"));
        }
        if !payload_env_for_kind(kind).is_empty() {
            return Err(fail("unknown kind gained payload environment"));
        }
    }
    if obligation_order_key("bogus", "bogus", "id").0 != u32::MAX {
        return Err(fail("unknown lane ordered as a known lane"));
    }
    must_fail!(shard_task_id("bogus", 0, 2), "sharded bad base");
    must_fail!(
        require_nextest_for_shards(TestRunner::CargoTest, 2),
        "sharded cargo test",
    );
    Ok(())
}

#[test]
fn gc1_t33_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_inputs()?;
    check_rejects_unknown_kinds()?;
    Ok(())
}
