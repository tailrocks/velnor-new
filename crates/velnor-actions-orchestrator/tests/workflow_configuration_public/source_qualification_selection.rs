// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::scoped_workspace::{commit, generate_lockfile, put};
use super::source_qualification::generator_repo;
use std::{error::Error, fs, path::Path};

pub(super) fn fixture() -> Result<(tempfile::TempDir, String), Box<dyn Error>> {
    let repo = generator_repo(true)?;
    let root = repo.path();
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/cli\", \"crates/orchestrator\", \"crates/guard\"]\nresolver = \"2\"\n",
    )?;
    for (name, manifest) in [
        (
            "cli",
            "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nvelnor-actions-orchestrator = { path = \"../orchestrator\" }\n",
        ),
        (
            "orchestrator",
            "[package]\nname = \"velnor-actions-orchestrator\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[dependencies]\nvelnor-archive-guard = { path = \"../guard\" }\n",
        ),
        (
            "guard",
            "[package]\nname = \"velnor-archive-guard\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        ),
    ] {
        put(root, &format!("crates/{name}/Cargo.toml"), manifest)?;
        put(
            root,
            &format!("crates/{name}/src/lib.rs"),
            "pub fn f() {}\n",
        )?;
    }
    let config = root.join(".velnor/config.toml");
    let old = fs::read_to_string(&config)?;
    fs::write(
        config,
        format!(
            "{old}\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n[[stacks.rust.inputs]]\nglobs = [\"fixtures/**\", \"scripts/**\", \".github/**\", \".velnor/**\", \"crates/**\", \"Cargo.toml\", \"Cargo.lock\"]\npackages = [\"velnor-archive-guard\"]\ncomplete = true\n"
        ),
    )?;
    put(root, "docs/guide.md", "old\n")?;
    put(root, "fixtures/oracle.txt", "old\n")?;
    put(root, "scripts/phase.sh", "old\n")?;
    generate_lockfile(root)?;
    let base = commit(root, "base")?;
    Ok((repo, base))
}

pub(super) fn install_selector(root: &Path) -> Result<(), Box<dyn Error>> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../scripts/generator-release/source-qualification-routing.py");
    let target = root.join("scripts/generator-release/source-qualification-routing.py");
    fs::create_dir_all(target.parent().ok_or("missing parent")?)?;
    fs::copy(source, target)?;
    Ok(())
}
