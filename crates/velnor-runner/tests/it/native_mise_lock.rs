// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::mise_double::each_row;
use crate::mise_world::{World, assert_failed, job_fixture};

const CONFIG: &str = "[settings]\nlockfile = true\n\n[tools]\nnode = \"26.13.1\"\n";
const NODE: &str = "[[tools.node]]\nversion = \"26.13.1\"\nbackend = \"core:node\"\n";

fn entry(backend: &str) -> String {
    format!("[[tools.node]]\nversion = \"26.13.1\"\nbackend = \"{backend}\"\n")
}

fn refused(config: &str, lock: &str, expected: &str) -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace_with_lock(config, lock)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(
        &ran,
        &format!("unsupported: mise policy: {expected} is not supported by velnor-runner"),
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new(), "{expected}");
    Ok(())
}

#[test]
fn mise_action_checks_every_lock_entry_of_a_tool() -> Result<()> {
    let inline = "core:node[postinstall='touch /tmp/x']";
    let rows = [
        (
            "go package in a second entry",
            format!("{NODE}\n{}", entry("go:example.com/pkg")),
            "backend 'go' of tool 'node'".to_owned(),
        ),
        (
            "go package in a first entry",
            format!("{}\n{NODE}", entry("go:example.com/pkg")),
            "backend 'go' of tool 'node'".to_owned(),
        ),
        (
            "inline option in a second entry",
            format!("{NODE}\n{}", entry(inline)),
            format!("backend '{inline}' of tool 'node' with inline options"),
        ),
        (
            "inline option in a single entry",
            entry(inline),
            format!("backend '{inline}' of tool 'node' with inline options"),
        ),
        (
            "second entry without a backend",
            format!("{NODE}\n[[tools.node]]\nversion = \"26.13.1\"\n"),
            "tool 'node' without a backend in mise.lock".to_owned(),
        ),
        (
            "plugin backend in a second entry",
            format!("{NODE}\n{}", entry("vfox:file:///tmp/p")),
            "backend 'vfox' of tool 'node'".to_owned(),
        ),
    ];
    for (label, lock, expected) in &rows {
        refused(CONFIG, lock, expected).map_err(|error| error.context(*label))?;
    }
    Ok(())
}

#[test]
fn mise_action_checks_the_lock_entries_of_tools_the_config_does_not_name() -> Result<()> {
    let lock =
        format!("{NODE}\n[[tools.\"cargo:evil\"]]\nversion = \"1\"\nbackend = \"cargo:evil\"\n");
    refused(CONFIG, &lock, "backend 'cargo' of tool 'cargo:evil'")
}

#[test]
fn mise_action_requires_the_lock_backend_to_match_the_prefix_of_its_key() -> Result<()> {
    let config = "[settings]\nlockfile = true\n\n[tools]\n\"github:acme/tool\" = \"1\"\n";
    let lock = "[[tools.\"github:acme/tool\"]]\nversion = \"1\"\nbackend = \"aqua:acme/tool\"\n";
    refused(
        config,
        lock,
        "backend 'aqua' of tool 'github:acme/tool' that differs from its key",
    )
}

#[test]
fn mise_action_accepts_several_download_entries_of_one_tool() -> Result<()> {
    let lock = format!("{NODE}\n{NODE}");
    let rows = [lock.as_str(), NODE];
    each_row(&rows, |lock| {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace_with_lock(CONFIG, lock)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
        assert_eq!(world.mise_installs().len(), 1);
        Ok(())
    })
}
