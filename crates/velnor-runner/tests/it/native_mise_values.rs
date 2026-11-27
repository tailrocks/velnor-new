// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use anyhow::Result;

use crate::mise_double::each_row;
use crate::mise_world::{World, assert_failed, job_fixture};

const HEAD: &str = "[settings]\nlockfile = true\n\n";
const GO_LOCK: &str = "[[tools.go]]\nversion = \"1.22.0\"\nbackend = \"core:go\"\n";
const NODE_LOCK: &str = "[[tools.node]]\nversion = \"26.13.1\"\nbackend = \"core:node\"\n";
const RUST_LOCK: &str = "[[tools.rust]]\nversion = \"1.90.0\"\nbackend = \"core:rust\"\n";
const NODE: &str = "[tools]\nnode = { version = \"26.13.1\", ";

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

#[test]
fn mise_action_refuses_every_package_and_ubi_backend() -> Result<()> {
    let backends = [
        "cargo", "go", "npm", "gem", "pipx", "pypi", "conda", "dotnet", "spm", "spinel", "ubi",
    ];
    each_row(&backends, |backend| {
        let key = format!("{backend}:pkg");
        let tools = format!("[tools]\n\"{key}\" = \"1.0.0\"\n");
        let lock = format!("[[tools.\"{key}\"]]\nversion = \"1.0.0\"\nbackend = \"{key}\"\n");
        let expected = format!("backend '{backend}' of tool '{key}'");
        refused(backend, &tools, &lock, &expected)?;
        refused(backend, &tools, "", &expected)
    })
}

#[test]
fn mise_action_refuses_a_ubi_tool_that_names_an_http_url() -> Result<()> {
    for key in [
        "ubi:http://example.com/tool.tgz",
        "ubi:https://example.com/tool.tgz",
    ] {
        let tools = format!("[tools]\n\"{key}\" = \"1\"\n");
        refused(key, &tools, "", &format!("backend 'ubi' of tool '{key}'"))?;
    }
    Ok(())
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
