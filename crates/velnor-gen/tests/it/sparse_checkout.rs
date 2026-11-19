// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn render(roots: &[&str], config: &str) -> Result<String, Box<dyn std::error::Error>> {
    workflow(&view(roots, Some(config.as_bytes()))?)
}

const OFF: &str = "full_run = \"off\"\n";
const ON: &str = "full_run = \"off\"\nsparse_checkout = true\n";

#[test]
fn off_is_byte_identical_to_the_default() -> TestResult {
    let roots = ["a/Cargo.toml", "b/Cargo.toml"];
    let default = render(&roots, OFF)?;
    let explicit = render(&roots, "full_run = \"off\"\nsparse_checkout = false\n")?;
    assert_eq!(default, explicit);
    assert!(!default.contains("sparse-checkout"));
    Ok(())
}

#[test]
fn on_lists_each_lane_its_own_root_and_the_tool_files() -> TestResult {
    let text = render(&["a/Cargo.toml", "b/Cargo.toml"], ON)?;
    let jobs = jobs_of(&text);
    let lanes: Vec<_> = jobs
        .iter()
        .filter(|(_, block)| block.contains("sparse-checkout: |"))
        .collect();
    assert!(!lanes.is_empty(), "{text}");
    for (id, block) in &lanes {
        assert!(block.contains("sparse-checkout-cone-mode: false"), "{id}");
        for always in [
            "/Cargo.lock",
            "/rust-toolchain.toml",
            "/mise.toml",
            "/.cargo/",
            "/.velnor/",
        ] {
            assert!(
                block.contains(&format!("            {always}\n")),
                "{id}: {always}"
            );
        }
        let a = block.contains("            /a/\n");
        let b = block.contains("            /b/\n");
        assert!(a || b, "{id} must list its own root");
    }
    let only = |own: &str, other: &str| {
        lanes
            .iter()
            .any(|(_, block)| block.contains(own) && !block.contains(other))
    };
    assert!(only("/a/\n", "/b/\n") && only("/b/\n", "/a/\n"));
    assert!(lanes.iter().any(|(_, block)| block.contains("/a/\n")));
    assert!(lanes.iter().any(|(_, block)| block.contains("/b/\n")));
    let plan = &jobs
        .iter()
        .find(|(id, _)| id == "rust-plan")
        .ok_or("plan")?
        .1;
    assert!(!plan.contains("sparse-checkout"), "{plan}");
    Ok(())
}

#[test]
fn declared_inputs_are_listed() -> TestResult {
    let config = format!("{ON}[[inputs]]\nglob = \"data/**\"\npackages = [\"package-0\"]\n");
    let text = render(&["a/Cargo.toml", "b/Cargo.toml"], &config)?;
    assert!(text.contains("            /data/**\n"), "{text}");
    Ok(())
}

#[test]
fn a_root_at_the_repository_top_fails_safe_to_a_full_checkout() -> TestResult {
    let text = render(&["Cargo.toml"], ON)?;
    assert!(!text.contains("sparse-checkout"), "{text}");
    Ok(())
}

#[test]
fn test_setup_scripts_are_unknown_reads_so_the_checkout_stays_full() -> TestResult {
    let config = format!(
        "{ON}[[test_setup]]\nroot = \"a\"\nname = \"Prepare\"\nrun = \"bash scripts/x.sh\"\n"
    );
    let text = render(&["a/Cargo.toml"], &config)?;
    assert!(!text.contains("sparse-checkout"), "{text}");
    Ok(())
}
