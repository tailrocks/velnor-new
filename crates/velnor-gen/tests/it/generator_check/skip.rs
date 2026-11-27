// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_rust::detect;

use super::world::{decide, put, runs, runs_every, skips_every, world};
use crate::facts::TestResult;

const OUTSIDE: [&str; 3] = ["crates/other/src/lib.rs", "docs/goals.md", "README.md"];
const INSIDE: [&str; 4] = [
    "crates/velnor-gen/src/main.rs",
    "crates/engine/src/lib.rs",
    "crates/core/src/lib.rs",
    "crates/core/README.md",
];

#[test]
fn a_change_outside_the_generator_closure_does_not_reach_the_generated_check() -> TestResult {
    let view = world()?;
    assert!(skips_every(&view, &OUTSIDE)?, "{OUTSIDE:?}");
    assert!(runs(&view, &["crates/velnor-gen/src/main.rs"])?);
    assert!(runs(
        &view,
        &["docs/goals.md", "crates/velnor-gen/src/main.rs"]
    )?);
    Ok(())
}

#[test]
fn a_change_in_a_generator_dependency_reaches_the_generated_check() -> TestResult {
    let view = world()?;
    assert!(runs_every(&view, &INSIDE)?, "{INSIDE:?}");
    assert!(runs(&view, &[".velnor/rust-ci.toml"])?);
    assert!(skips_every(&view, &["crates/other/src/lib.rs"])?);
    Ok(())
}

#[test]
fn a_file_the_lock_lists_for_a_closure_package_reaches_the_generated_check() -> TestResult {
    let view = world()?;
    let listed = [
        "shared/borrowed.rs",
        "docs/embedded.md",
        "data/dir/notes.md",
        "data/dir/deep/notes.md",
    ];
    assert!(runs_every(&view, &listed)?, "{listed:?}");
    assert!(skips_every(
        &view,
        &["shared/other.rs", "data/other.md", "data/dirty.md"]
    )?);
    let mut wide = world()?;
    put(
        &mut wide,
        ".velnor/rust-inputs.lock",
        ".\tcore\tcrates/core/Cargo.toml\n",
    )?;
    assert!(runs_every(&wide, &["docs/goals.md", "anywhere/else.rs"])?);
    Ok(())
}

#[test]
fn only_a_markdown_or_rust_file_can_skip_the_generated_check() -> TestResult {
    let view = world()?;
    let skipped = ["docs/a.md", "docs/B.MD", "scripts/tool.rs"];
    let run = [
        "docs/a.txt",
        "docs/a.toml",
        "docs/a.json",
        "scripts/a.sh",
        "scripts/a.RS",
        "scripts/Makefile",
        "docs/md",
    ];
    assert!(skips_every(&view, &skipped)?, "{skipped:?}");
    assert!(runs_every(&view, &run)?, "{run:?}");
    Ok(())
}

#[test]
fn a_file_under_the_generator_folders_reaches_the_generated_check() -> TestResult {
    let view = world()?;
    let folders = [
        ".github/notes.md",
        ".velnor/notes.md",
        "generator/notes.md",
        "generator/samples/x/src/lib.rs",
    ];
    assert!(runs_every(&view, &folders)?, "{folders:?}");
    assert!(skips_every(
        &view,
        &["generators/notes.md", "github/notes.md"]
    )?);
    Ok(())
}

#[test]
fn a_path_the_config_names_reaches_the_generated_check() -> TestResult {
    let mut view = world()?;
    let facts = detect(&view)?;
    let target = facts.rust_roots[0].target_dir.to_string();
    let delta = format!("{target}.velnor-delta");
    let target_note = format!("{target}/notes.md");
    let delta_note = format!("{delta}/notes.md");
    assert!(runs_every(&view, &[&target_note, &delta_note])?);
    assert!(skips_every(&view, &["targets/notes.md"])?);
    let config = format!(
        "{}\n[[check_configurations]]\nroot = '.'\nconfigurations = 'ci/features.md'\n\n[[image]]\nname = 'svc'\nbins = ['velnor-gen']\ndockerfile = 'docker/svc.Dockerfile'\ninputs = ['docker/extra']\n\n[[image.registry]]\nhost = 'docker.io'\nrepository = 'acme/svc'\nusername = 'secrets.U'\ntoken = 'secrets.T'\n",
        super::world::CONFIG
    );
    super::world::set_config(&mut view, &config)?;
    put(&mut view, "ci/features.md", "{}\n")?;
    let facts = detect(&view)?;
    let reach = |file: &str| decide(&facts, &view.tracked_paths, &[file], &[], &[]);
    assert!(reach("ci/features.md")?);
    assert!(reach("docker/extra/readme.md")?);
    assert!(reach("docker/svc.Dockerfile")?);
    assert!(!reach("docs/unrelated.md")?);
    assert!(!reach("docker/extra.md")?);
    Ok(())
}
