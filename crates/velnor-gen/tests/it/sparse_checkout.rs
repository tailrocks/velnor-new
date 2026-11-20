// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::archive::jobs_of;
use crate::facts::{TestResult, lib, package, repo};
use crate::pipeline::{view, workflow};
use velnor_model::{DependencyKind, TargetKind};

fn render(roots: &[&str], config: &str) -> Result<String, Box<dyn std::error::Error>> {
    workflow(&view(roots, Some(config.as_bytes()))?)
}

const OFF: &str = "full_run = \"off\"\n";
const NEXTEST_ONLY: &str = "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";
const ON: &str = "full_run = \"off\"\nsparse_checkout = true\n";

#[test]
fn off_is_byte_identical_to_the_default() -> TestResult {
    let roots = ["a/Cargo.toml", "b/Cargo.toml"];
    let mut plain = view(&roots, Some(OFF.as_bytes()))?;
    crate::pipeline::pin_helper(&mut plain, "0.1.92")?;
    let default = workflow(&plain)?;
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

fn lanes_of(text: &str) -> Vec<(String, String)> {
    jobs_of(text)
        .into_iter()
        .filter(|(_, block)| block.contains("sparse-checkout: |"))
        .collect()
}

fn lane_listing(block: &str) -> Vec<&str> {
    block
        .split("sparse-checkout: |\n")
        .nth(1)
        .unwrap_or_default()
        .lines()
        .take_while(|line| line.starts_with("            "))
        .map(str::trim)
        .collect()
}

#[test]
fn every_lane_keeps_every_manifest_so_check_metadata_sees_the_other_roots() -> TestResult {
    let text = render(&["a/Cargo.toml", "b/Cargo.toml"], ON)?;
    let lanes = lanes_of(&text);
    assert!(!lanes.is_empty(), "{text}");
    for (id, block) in &lanes {
        assert!(lane_listing(block).contains(&"**/Cargo.toml"), "{id}");
    }
    Ok(())
}

#[test]
fn a_nested_root_keeps_the_cargo_configuration_of_each_ancestor_folder() -> TestResult {
    let text = render(&["x/y/Cargo.toml", "b/Cargo.toml"], ON)?;
    let lanes = lanes_of(&text);
    let nested: Vec<_> = lanes
        .iter()
        .filter(|(_, block)| lane_listing(block).contains(&"/x/y/"))
        .collect();
    assert!(!nested.is_empty(), "{text}");
    for (id, block) in nested {
        assert!(lane_listing(block).contains(&"/x/.cargo/"), "{id}");
    }
    for (id, block) in lanes
        .iter()
        .filter(|(_, block)| !lane_listing(block).contains(&"/x/y/"))
    {
        assert!(!lane_listing(block).contains(&"/x/.cargo/"), "{id}");
    }
    Ok(())
}

#[test]
fn declared_inputs_are_escaped_for_gitignore_syntax() -> TestResult {
    let config =
        format!("{ON}[[inputs]]\nglob = \"fixtures/[data]/a?b c\"\npackages = [\"package-0\"]\n");
    let text = render(&["a/Cargo.toml", "b/Cargo.toml"], &config)?;
    let lanes = lanes_of(&text);
    assert!(!lanes.is_empty(), "{text}");
    for (id, block) in lanes {
        let listing = lane_listing(&block);
        assert!(
            listing.contains(&"/fixtures/\\[data\\]/a\\?b\\ c"),
            "{id}: {listing:?}"
        );
        assert!(!listing.contains(&"/fixtures/[data]/a?b c"), "{id}");
    }
    Ok(())
}

#[test]
fn a_path_toolchain_inside_the_repository_is_checked_out_and_an_escaping_one_forces_a_full_checkout()
-> TestResult {
    let render_with = |toolchain: &str| {
        let mut view = view(&["a/Cargo.toml", "b/Cargo.toml"], Some(ON.as_bytes()))?;
        let file = velnor_model::RepoPath::new("rust-toolchain.toml")?;
        view.tracked_paths.insert(file.clone());
        view.files.insert(file, toolchain.as_bytes().to_vec());
        view.files.insert(
            velnor_model::RepoPath::new("mise.toml")?,
            NEXTEST_ONLY.as_bytes().to_vec(),
        );
        workflow(&view)
    };
    let inside = render_with("[toolchain]\npath = \"toolchains/custom\"\n")?;
    let lanes = lanes_of(&inside);
    assert!(!lanes.is_empty(), "{inside}");
    for (id, block) in lanes {
        assert!(
            lane_listing(&block).contains(&"/toolchains/custom/"),
            "{id}"
        );
    }
    let absolute = render_with("[toolchain]\npath = \"/opt/toolchain\"\n")?;
    assert!(
        lanes_of(&absolute)
            .iter()
            .any(|(_, block)| lane_listing(block).contains(&"/.velnor/")),
        "{absolute}"
    );
    assert!(!absolute.contains("/opt/toolchain/"), "{absolute}");
    let escaping = render_with("[toolchain]\npath = \"../outside\"\n")?;
    assert!(!escaping.contains("sparse-checkout"), "{escaping}");
    Ok(())
}

const WITH_RUST: &str =
    "[tools]\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\nrust = \"1.99.1\"\n";

const VENDORED: [(&str, &str); 4] = [
    ("mise.toml", WITH_RUST),
    (".velnor/rust-ci.toml", ON),
    (
        "vendor/mid/Cargo.toml",
        "[package]\nname = \"mid\"\n\n[dependencies]\nleaf = { path = \"../leaf\" }\n",
    ),
    ("vendor/leaf/Cargo.toml", "[package]\nname = \"leaf\"\n"),
];

fn vendored(files: &[(&str, &str)]) -> Result<String, Box<dyn std::error::Error>> {
    let app = package(
        "app",
        "a/app/Cargo.toml",
        &[("mid", DependencyKind::Normal, "vendor/mid/Cargo.toml")],
        &[("app", TargetKind::Lib, true)],
    )?;
    workflow(&repo(
        vec![
            ("a/Cargo.toml", vec![app]),
            ("b/Cargo.toml", vec![lib("base", "b/base/Cargo.toml")?]),
        ],
        files,
    )?)
}

#[test]
fn a_lane_keeps_the_path_dependencies_that_sit_outside_every_root() -> TestResult {
    let text = vendored(&VENDORED)?;
    let lanes = lanes_of(&text);
    let with_app: Vec<_> = lanes
        .iter()
        .filter(|(_, block)| lane_listing(block).contains(&"/a/"))
        .collect();
    assert!(!with_app.is_empty(), "{text}");
    for (id, block) in with_app {
        let listing = lane_listing(block);
        assert!(listing.contains(&"/vendor/mid/"), "{id}: {listing:?}");
        assert!(listing.contains(&"/vendor/leaf/"), "{id}: {listing:?}");
    }
    for (id, block) in lanes
        .iter()
        .filter(|(_, block)| !lane_listing(block).contains(&"/a/"))
    {
        assert!(!lane_listing(block).contains(&"/vendor/mid/"), "{id}");
    }
    Ok(())
}

#[test]
fn an_outside_dependency_that_cannot_be_read_forces_a_full_checkout() -> TestResult {
    let unreadable = [
        ("mise.toml", WITH_RUST),
        (".velnor/rust-ci.toml", ON),
        (
            "vendor/mid/Cargo.toml",
            "[package]\nname = \"mid\"\n\n[dependencies]\nleaf = { workspace = true }\n",
        ),
    ];
    let text = vendored(&unreadable)?;
    let lanes = lanes_of(&text);
    assert!(!lanes.is_empty(), "{text}");
    assert!(
        lanes
            .iter()
            .all(|(_, block)| !lane_listing(block).contains(&"/a/")),
        "the lane that reaches the unreadable package stayed sparse\n{text}"
    );
    Ok(())
}
