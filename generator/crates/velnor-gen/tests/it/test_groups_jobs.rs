// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::test_groups::{
    DATABASE, SHELL, job, names, needs, rendered, run_of, step, steps, view_with,
};
use crate::yaml_tree::document;

#[test]
fn a_group_renders_as_a_test_job_with_its_own_limits_and_steps() -> TestResult {
    let workflow = rendered(DATABASE)?;
    let mut ids: Vec<&str> = workflow["jobs"]
        .as_object()
        .into_iter()
        .flat_map(|jobs| jobs.keys().map(String::as_str))
        .filter(|id| id.starts_with("rust-test"))
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        ["rust-test", "rust-test-database", "rust-test-default"]
    );
    let group = job(&workflow, "rust-test-database")?;
    assert_eq!(group["runs-on"], "ubuntu-24.04");
    assert_eq!(group["timeout-minutes"], 45);
    assert!(needs(group).contains(&"rust-plan") && needs(group).contains(&"rust-test"));
    let run = run_of(group, "Test")?;
    assert!(run.contains("--test-threads 2"), "{run}");
    assert!(run.contains("--archive-file"), "{run}");
    assert!(
        run.contains("-E \"(($TEST_FILTER) and (package(package-0) and test(/^db_/)))\""),
        "{run}"
    );
    let all = names(group);
    let docker = all
        .iter()
        .position(|name| *name == "Check that Docker answers")
        .ok_or("no docker check")?;
    let download = all
        .iter()
        .position(|name| name.starts_with("Download"))
        .ok_or("no download")?;
    let test = all
        .iter()
        .position(|name| *name == "Test")
        .ok_or("no test")?;
    assert!(download < docker && docker < test, "{all:?}");
    assert_eq!(run_of(group, "Check that Docker answers")?, "docker info");
    for absent in ["velnor-ci prepare", "Add Rust components", "Build tests"] {
        assert!(!all.contains(&absent), "{absent} in {all:?}");
    }
    let defaults = job(&workflow, "rust-test-default")?;
    assert!(!names(defaults).contains(&"Check that Docker answers"));
    assert_eq!(defaults["runs-on"], "ubuntu-latest");
    assert!(!run_of(defaults, "Test")?.contains("--test-threads"));
    Ok(())
}

#[test]
fn the_cargo_need_installs_the_full_toolchain_and_nothing_else_does() -> TestResult {
    let workflow = rendered(&format!("{DATABASE}\n{SHELL}"))?;
    let plain = job(&workflow, "rust-test-database")?;
    let cargo = job(&workflow, "rust-test-shell")?;
    let only_nextest = |job: &Value| {
        step(job, "Install tools")
            .map(|step| step["with"]["install_args"].as_str().map(str::to_owned))
    };
    assert_eq!(
        only_nextest(plain)?.as_deref(),
        Some("aqua:nextest-rs/nextest/cargo-nextest")
    );
    assert_eq!(only_nextest(cargo)?, None);
    assert!(
        names(cargo).contains(&"velnor-ci prepare"),
        "{:?}",
        names(cargo)
    );
    assert!(!names(plain).contains(&"velnor-ci prepare"));
    assert!(!names(cargo).contains(&"Check that Docker answers"));
    for forbidden in [
        "Restore the target cache",
        "Restore the sources cache",
        "Build tests",
    ] {
        assert!(!names(cargo).contains(&forbidden), "{forbidden}");
    }
    assert!(run_of(cargo, "Test")?.contains("and (binary(it) and test(~cargo_))"));
    assert!(!run_of(cargo, "Test")?.contains("--test-threads"));
    Ok(())
}

#[test]
fn the_default_job_leaves_out_every_test_a_group_owns() -> TestResult {
    let workflow = rendered(&format!("{DATABASE}\n{SHELL}"))?;
    let run = run_of(job(&workflow, "rust-test-default")?, "Test")?;
    assert!(
        run.contains("-E \"((($TEST_FILTER) and not (package(package-0) and test(/^db_/))) and not (binary(it) and test(~cargo_)))\""),
        "{run}"
    );
    Ok(())
}

#[test]
fn any_group_splits_the_root_into_an_archive_whatever_the_shape() -> TestResult {
    for shape in [
        "",
        "shape = 'one-job'\n",
        "shape = 'lint-test'\n",
        "shape = 'groups'\n",
    ] {
        let workflow = rendered(&format!("{shape}{DATABASE}"))?;
        let build = job(&workflow, "rust-test")?;
        assert!(names(build).contains(&"Upload the tests"), "{shape}");
        assert!(job(&workflow, "rust-test-database").is_ok(), "{shape}");
        assert!(job(&workflow, "rust").is_err(), "{shape}");
    }
    Ok(())
}

#[test]
fn the_required_gate_covers_every_group_job() -> TestResult {
    let text = workflow(&view_with(&["Cargo.toml"], DATABASE, "0.1.82")?)?;
    let tree = document(&text)?;
    let required = job(&tree, "required")?;
    assert!(
        needs(required).contains(&"rust-test-database"),
        "{:?}",
        needs(required)
    );
    let spec = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("GATE_SPEC: '"))
        .ok_or("no gate spec")?;
    assert!(
        spec.contains("--job rust-test-database=lane:root"),
        "{spec}"
    );
    assert!(spec.contains("--tests rust-test-database=root"), "{spec}");
    Ok(())
}

#[test]
fn every_group_has_a_job_per_root_and_a_name_cannot_collide_with_a_root() -> TestResult {
    let config = "[[test_groups]]\nname = 'lab'\nroot = '.'\nfilter = 'all()'\n\n[[test_groups]]\nname = 'db'\nroot = 'lab'\nfilter = 'all()'\nneeds = ['docker']\n";
    let text = workflow(&view_with(
        &["Cargo.toml", "lab/Cargo.toml"],
        config,
        "0.1.82",
    )?)?;
    let tree = document(&text)?;
    for id in [
        "rust-test-root",
        "rust-test-default-root",
        "rust-test-lab-root",
        "rust-test-lab",
        "rust-test-default-lab",
        "rust-test-db-lab",
    ] {
        assert!(job(&tree, id).is_ok(), "{id}");
    }
    assert!(job(&tree, "rust-test-db-root").is_err());
    let root_default = run_of(job(&tree, "rust-test-default-root")?, "Test")?;
    let lab_default = run_of(job(&tree, "rust-test-default-lab")?, "Test")?;
    assert!(root_default.contains("and not (all())"), "{root_default}");
    assert!(lab_default.contains("and not (all())"), "{lab_default}");
    let db = needs(job(&tree, "rust-test-db-lab")?);
    assert!(
        db.contains(&"rust-plan") && db.contains(&"rust-test-lab"),
        "{db:?}"
    );
    Ok(())
}

#[test]
fn a_generated_run_is_one_command_with_no_shell_syntax_in_any_group_job() -> TestResult {
    let both = format!("{DATABASE}\n{SHELL}");
    let workflow = rendered(&both)?;
    for id in [
        "rust-test",
        "rust-test-default",
        "rust-test-database",
        "rust-test-shell",
    ] {
        for step in steps(job(&workflow, id)?) {
            let Some(run) = step["run"].as_str() else {
                continue;
            };
            for forbidden in ["\n", ";", "|", "&", "${{"] {
                assert!(
                    !run.contains(forbidden),
                    "{id}: {run:?} contains {forbidden:?}"
                );
            }
        }
    }
    Ok(())
}
