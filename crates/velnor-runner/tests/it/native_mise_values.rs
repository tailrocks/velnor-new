// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::path::PathBuf;

use anyhow::Result;

use crate::mise_double::each_row;
use crate::mise_store::StoreMode;
use crate::mise_world::{World, assert_failed, job_fixture};

const HEAD: &str = "[settings]\nlockfile = true\n\n";
const GO_LOCK: &str = "[[tools.go]]\nversion = \"1.22.0\"\nbackend = \"core:go\"\n";
const NODE_LOCK: &str = "[[tools.node]]\nversion = \"26.13.1\"\nbackend = \"core:node\"\n";
const RUST_LOCK: &str = "[[tools.rust]]\nversion = \"1.90.0\"\nbackend = \"core:rust\"\n";
const NODE: &str = "[tools]\nnode = { version = \"26.13.1\", ";
const SHA256: &str = "sha256:ca1f81a5b86533eb45ab97e0064aa88e925e53aab9a698ca6331a7cad1f85f07";

type Row = (&'static str, &'static str, &'static str, &'static str);

fn refused(label: &str, body: &str, lock: &str, expected: &str) -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    world.init_workspace_with_lock(&format!("{HEAD}{body}"), lock)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(
        &ran,
        &format!("unsupported: mise policy: {expected} is not supported by velnor-runner"),
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new(), "{label}");
    Ok(())
}

#[test]
fn mise_action_refuses_tool_specs_that_name_a_local_or_remote_code_source() -> Result<()> {
    let rows: [Row; 11] = [
        (
            "path",
            "[tools]\ngo = \"path:/tmp/go\"\n",
            GO_LOCK,
            "tool 'go' source 'path:/tmp/go'",
        ),
        (
            "path in a table",
            "[tools]\ngo = { version = \"path:/tmp/go\" }\n",
            GO_LOCK,
            "tool 'go' source 'path:/tmp/go'",
        ),
        (
            "path in an array",
            "[tools]\ngo = [\"1.22.0\", \"path:/tmp/go\"]\n",
            GO_LOCK,
            "tool 'go' source 'path:/tmp/go'",
        ),
        (
            "file url",
            "[tools]\nnode = \"file:///tmp/node\"\n",
            NODE_LOCK,
            "tool 'node' source 'file:///tmp/node'",
        ),
        (
            "file url in an option",
            "[tools]\n\"packslip:github.com/jdx/mr-boxington\" = { version = \"1\", workflow = \"file:///tmp/w\" }\n",
            "",
            "tool 'packslip:github.com/jdx/mr-boxington' source 'file:///tmp/w'",
        ),
        (
            "vfox plugin url",
            "[tools]\n\"vfox:file:///tmp/p\" = \"1\"\n",
            "",
            "backend 'vfox' of tool 'vfox:file:///tmp/p'",
        ),
        (
            "asdf plugin",
            "[tools]\n\"asdf:evil\" = \"1\"\n",
            "",
            "backend 'asdf' of tool 'asdf:evil'",
        ),
        (
            "unknown plugin prefix",
            "[tools]\n\"plug:tool\" = \"1\"\n",
            "",
            "backend 'plug' of tool 'plug:tool'",
        ),
        (
            "registry name locked to vfox",
            "[tools]\nfoo = \"1\"\n",
            "[[tools.foo]]\nversion = \"1\"\nbackend = \"vfox:acme/foo\"\n",
            "backend 'vfox' of tool 'foo'",
        ),
        (
            "integer version",
            "[tools]\nnode = 22\n",
            NODE_LOCK,
            "tool 'node' with this value",
        ),
        (
            "registry name without a lock backend",
            "[tools]\nfoo = \"1\"\n",
            "[[tools.foo]]\nversion = \"1\"\n",
            "tool 'foo' without a backend in mise.lock",
        ),
    ];
    each_row(&rows, |row| refused(row.0, row.1, row.2, row.3))
}

#[test]
fn mise_action_refuses_tool_options_outside_the_safe_set() -> Result<()> {
    let rows: [Row; 7] = [
        (
            "postinstall",
            "postinstall = \"touch /tmp/x\" }\n",
            NODE_LOCK,
            "tool option 'postinstall' of 'node'",
        ),
        (
            "install_env",
            "install_env = \"A=1\" }\n",
            NODE_LOCK,
            "tool option 'install_env' of 'node'",
        ),
        (
            "depends",
            "depends = [\"go\"] }\n",
            NODE_LOCK,
            "tool option 'depends' of 'node'",
        ),
        (
            "url",
            "url = \"https://example.com/n.tgz\" }\n",
            NODE_LOCK,
            "tool option 'url' of 'node'",
        ),
        (
            "table value of a safe key",
            "os = { linux = \"x\" } }\n",
            NODE_LOCK,
            "tool option 'os' of 'node'",
        ),
        (
            "nested array of a safe key",
            "os = [[\"linux\"]] }\n",
            NODE_LOCK,
            "tool option 'os' of 'node'",
        ),
        (
            "mr_boxington",
            "mr_boxington = true }\n",
            NODE_LOCK,
            "tool option 'mr_boxington' of 'node'",
        ),
    ];
    each_row(&rows, |row| {
        refused(row.0, &format!("{NODE}{}", row.1), row.2, row.3)
    })?;
    let inline = "[tools]\n\"npm:foo[scripts=true]\" = \"1\"\n";
    refused(
        "inline options",
        inline,
        "",
        "tool 'npm:foo[scripts=true]' with inline options",
    )?;
    let rust = "[tools]\nrust = { version = \"1.90.0\", mr_boxington = true }\n";
    refused(
        "rust",
        rust,
        RUST_LOCK,
        "tool option 'mr_boxington' of 'rust'",
    )
}

fn package_lock(key: &str, platforms: &[&str]) -> String {
    platform_lock(key, platforms, "\"platforms.p{}\"")
}

fn nested_package_lock(key: &str, platforms: &[&str]) -> String {
    platform_lock(key, platforms, "platforms.p{}")
}

fn lone_entry(key: &str) -> String {
    format!("[[tools.\"{key}\"]]\nversion = \"2.0.0\"\nbackend = \"{key}\"\n")
}

fn platform_lock(key: &str, platforms: &[&str], table: &str) -> String {
    let mut lock = format!("[[tools.\"{key}\"]]\nversion = \"1.0.0\"\nbackend = \"{key}\"\n");
    for (index, platform) in platforms.iter().enumerate() {
        let name = table.replace("{}", &index.to_string());
        let _ = write!(lock, "\n[tools.\"{key}\".{name}]\n{platform}\n");
    }
    lock
}

#[test]
fn mise_action_refuses_every_package_backend_without_a_lock_checksum() -> Result<()> {
    let backends = [
        "cargo", "go", "npm", "gem", "pipx", "pypi", "conda", "dotnet", "spm", "spinel",
    ];
    each_row(&backends, |backend| {
        let key = format!("{backend}:pkg");
        let tools = format!("[tools]\n\"{key}\" = \"1.0.0\"\n");
        let lock = format!("[[tools.\"{key}\"]]\nversion = \"1.0.0\"\nbackend = \"{key}\"\n");
        let expected = format!("package tool '{key}' without a checksum in mise.lock");
        refused(backend, &tools, &lock, &expected)
    })
}

#[test]
fn mise_action_refuses_a_package_lock_entry_that_does_not_carry_checksums_throughout() -> Result<()>
{
    let key = "cargo:pkg";
    let tools = format!("[tools]\n\"{key}\" = \"1.0.0\"\n");
    let expected = format!("package tool '{key}' without a checksum in mise.lock");
    let good = format!("checksum = \"{SHA256}\"");
    let sha512 = format!("checksum = \"sha512:{}\"", "ab".repeat(64));
    let rows = [
        ("no entry", String::new()),
        ("no platforms", package_lock(key, &[])),
        (
            "no checksum",
            package_lock(key, &["checksum_source = \"mise-versions\""]),
        ),
        (
            "weak algorithm",
            package_lock(
                key,
                &["checksum = \"md5:d41d8cd98f00b204e9800998ecf8427e\""],
            ),
        ),
        (
            "short digest",
            package_lock(key, &["checksum = \"sha256:abc\""]),
        ),
        (
            "non hex digest",
            package_lock(
                key,
                &[&format!("checksum = \"sha256:{}\"", "zz".repeat(32))],
            ),
        ),
        (
            "one platform without",
            package_lock(key, &[&good, "url = \"x\""]),
        ),
        (
            "one platform empty",
            package_lock(key, &[&sha512, "checksum = \"\""]),
        ),
        ("nested without", nested_package_lock(key, &["url = \"x\""])),
        (
            "second entry without",
            format!("{}\n{}", lone_entry(key), package_lock(key, &[&good])),
        ),
    ];
    for (label, lock) in rows {
        refused(label, &tools, &lock, &expected)?;
    }
    Ok(())
}

#[test]
fn mise_action_accepts_a_package_backend_whose_lock_entry_carries_checksums() -> Result<()> {
    let sha512 = format!("checksum = \"sha512:{}\"", "ab".repeat(64));
    let blake3 = format!("checksum = \"blake3:{}\"", "cd".repeat(32));
    let good = [format!("checksum = \"{SHA256}\""), sha512, blake3];
    let words: Vec<&str> = good.iter().map(String::as_str).collect();
    let locks = [
        package_lock("cargo:pkg", &words),
        nested_package_lock("cargo:pkg", &words),
    ];
    each_row(&locks.each_ref().map(String::as_str), |lock| {
        let world = World::new()?;
        world.install_mise()?;
        world.init_workspace_with_lock(
            &format!("{HEAD}[tools]\n\"cargo:pkg\" = \"1.0.0\"\n"),
            lock,
        )?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
        assert_eq!(world.mise_installs().len(), 1);
        Ok(())
    })
}

#[test]
fn mise_action_refuses_reserved_env_names() -> Result<()> {
    let names = [
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUSTUP_TOOLCHAIN",
        "PATH",
        "Path",
        "HOME",
        "MISE_DATA_DIR",
        "MISE_SHIMS_DIR",
    ];
    each_row(&names, |name| {
        for section in ["[env]", "[[env]]"] {
            let body = format!("{section}\n{name} = \"/x\"\n");
            refused("env", &body, "", &format!("[env] name '{name}'"))?;
        }
        Ok(())
    })
}

#[test]
fn mise_action_accepts_table_options_inside_the_safe_set() -> Result<()> {
    let world = World::new()?;
    world.install_mise()?;
    let config = format!(
        "{HEAD}[env]\nGREETING = \"hi\"\n_.path = [\"bin\"]\n\n[tools]\nnode = {{ version = \"26.13.1\", os = [\"linux\", \"macos\"] }}\n"
    );
    world.init_workspace_with_lock(&config, NODE_LOCK)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert!(ran.has_line("job succeeded"), "{}", ran.stdout);
    assert_eq!(world.mise_installs().len(), 1);
    Ok(())
}

#[test]
fn mise_action_refuses_a_data_dir_the_step_can_write() -> Result<()> {
    for relative in ["tools/data", "tools", ""] {
        let mut world = World::new()?;
        world.store_mode = StoreMode::Open(relative);
        world.install_mise()?;
        world.init_workspace(true)?;
        let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
        let writable = world.store.join(relative).components().collect::<PathBuf>();
        assert_failed(
            &ran,
            &format!(
                "mise policy: {} is writable by this process; the tools store holding MISE_DATA_DIR {} must be read-only",
                writable.display(),
                world.data_dir().display()
            ),
        );
        assert_eq!(world.mise_calls(), Vec::<String>::new(), "{relative:?}");
    }
    Ok(())
}

#[test]
fn mise_action_refuses_a_data_dir_missing_from_the_store() -> Result<()> {
    let mut world = World::new()?;
    world.store_mode = StoreMode::NoData;
    world.install_mise()?;
    world.init_workspace(true)?;
    let ran = world.run(&job_fixture("mise_action_ok"), &[])?;
    assert_failed(
        &ran,
        &format!(
            "mise policy: MISE_DATA_DIR {} is missing from the tools store",
            world.data_dir().display()
        ),
    );
    assert_eq!(world.mise_calls(), Vec::<String>::new());
    Ok(())
}
