// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, ensure};

use crate::exec_support::Ran;
use crate::mise_real::{Check, Plant, Row, Verify, canonical};
use crate::mise_store::link_runtime_versions;
use crate::mise_world::{World, git};

pub(crate) const STATE_IDS: [&str; 7] = ["S1", "S2", "S3", "P1", "P2", "P3", "P4"];

pub(crate) const RUST_CONFIG: &str =
    "[settings]\nlockfile = true\n\n[env]\nACCEPTED = \"1\"\n\n[tools]\nrust = \"1.90.0\"\n";
pub(crate) const RUST_LOCK: &str =
    "[[tools.rust]]\nversion = \"1.90.0\"\nbackend = \"core:rust\"\nspecifiers = [\"1.90.0\"]\n";
const RUST_SCRIPT: &str =
    "echo \"accepted=$ACCEPTED\"\necho \"$PATH\" | tr ':' '\\n' | sed 's/^/pathentry=/'";
const GIT_CONFIG: &str = "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = true\n";

fn marker(world: &World, label: &str) -> PathBuf {
    world.root.join("markers").join(label)
}

fn stub(marker: Option<&Path>) -> String {
    let touch = marker.map_or_default(|marker| format!("touch {}\n", marker.display()));
    format!(
        "#!/bin/sh\n{touch}case \"$1 $2\" in\n  'show profile') echo minimal ;;\n  'component list') printf 'cargo\\nrustc\\nrust-std\\nclippy\\nrustfmt\\n' ;;\n  'target list') printf 'aarch64-unknown-linux-gnu\\nx86_64-unknown-linux-gnu\\n' ;;\nesac\nexit 0\n"
    )
}

impl World {
    pub(crate) fn install_store_homes(&self) -> Result<()> {
        self.unseal_store()?;
        let tools = self.store.join("tools");
        let bin = tools.join("cargo/bin");
        std::fs::create_dir_all(&bin).context("create the store cargo home")?;
        std::fs::create_dir_all(tools.join("rustup")).context("create the store rustup home")?;
        std::fs::write(tools.join("rustup/settings.toml"), "").context("write settings.toml")?;
        let installs = tools.join("data/installs/rust");
        std::fs::create_dir_all(&installs).context("create the rust installs")?;
        let link = installs.join("1.90.0");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&bin, &link).context("link the rust install")?;
        link_runtime_versions(&installs, "1.90.0")?;
        for name in ["rustup", "cargo", "rustc"] {
            let path = bin.join(name);
            std::fs::write(&path, stub(None)).context("write the store proxy")?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .context("make the store proxy executable")?;
        }
        Ok(())
    }
}

fn worktree_plant(world: &World, id: &str) -> Plant {
    let marker = marker(world, id);
    let content = format!(
        "[settings]\nlockfile = true\n\n[env]\nACCEPTED = \"evil\"\n\n[vars]\nV = \"{{{{ exec(command='touch {}') }}}}\"\n",
        marker.display()
    );
    Plant {
        path: world.workspace().join("mise.toml"),
        content,
        marker: Some(marker),
        listed: true,
    }
}

fn hide_edit(flag: &'static str) -> Check {
    Box::new(move |world, _| {
        git(&world.workspace(), &["update-index", flag, "mise.toml"]).map(drop)
    })
}

fn fsmonitor_row(world: &World, id: &'static str) -> Row {
    let marker = marker(world, id);
    let config = format!(
        "{GIT_CONFIG}\tfsmonitor = touch {}; true\n",
        marker.display()
    );
    Row {
        id,
        plants: vec![Plant {
            path: world.workspace().join(".git/config"),
            content: config,
            marker: Some(marker),
            listed: false,
        }],
        control: Some(Box::new(|world, _| {
            Command::new("git")
                .arg("-C")
                .arg(world.workspace())
                .args(["status", "--porcelain"])
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("HOME", world.home())
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .context("run the git control")
                .map(drop)
        })),
        ..Row::default()
    }
}

fn homes_control(cargo: PathBuf, rustup: PathBuf) -> Check {
    Box::new(move |world, real| {
        let env = vec![
            ("CARGO_HOME".to_owned(), cargo.display().to_string()),
            ("RUSTUP_HOME".to_owned(), rustup.display().to_string()),
        ];
        let output = world
            .control(real, &world.workspace(), &env)
            .args(["install", "--locked"])
            .output()
            .context("run the control install")?;
        ensure!(
            output.status.success(),
            "the control install failed ({}): {}",
            real.version,
            String::from_utf8_lossy(&output.stderr)
        );
        let link = world.root.join("control-data/installs/rust/1.90.0");
        let target = std::fs::read_link(&link).context("the control's rust install link")?;
        ensure!(
            canonical(&target) == canonical(&cargo.join("bin")),
            "the control's rust install links to {}, not the planted bin",
            target.display()
        );
        Ok(())
    })
}

fn homes_verify(planted_bin: PathBuf) -> Verify {
    Box::new(move |world, ran: &Ran| {
        let store_bin = world.store.join("tools/cargo/bin");
        let link = world.store.join("tools/data/installs/rust/1.90.0");
        let target = std::fs::read_link(&link).ok();
        let mut problems = Vec::new();
        if target.as_deref().map(canonical) != Some(canonical(&store_bin)) {
            problems.push(format!(
                "the store's rust install links to {target:?}, not the store's cargo/bin"
            ));
        }
        let planted = ran.lines().into_iter().any(|line| {
            line.strip_prefix("pathentry=")
                .is_some_and(|entry| canonical(Path::new(entry)) == canonical(&planted_bin))
        });
        if planted {
            problems.push("the planted bin directory reached the later step's PATH".to_owned());
        }
        (!problems.is_empty()).then(|| problems.join("; "))
    })
}

fn homes_row(world: &World, id: &'static str) -> Row {
    let planted = match id {
        "P3" => world.root.join("planthome"),
        "P4" => world.home(),
        _ => world.root.join("plant"),
    };
    let (cargo, rustup) = if matches!(id, "P3" | "P4") {
        (planted.join(".cargo"), planted.join(".rustup"))
    } else {
        (planted.join("cargo"), planted.join("rustup"))
    };
    let bin = cargo.join("bin");
    let program = |name: &str, hook: bool| {
        let marker = hook.then(|| marker(world, &format!("{id}-{name}")));
        Plant {
            path: bin.join(name),
            content: stub(marker.as_deref()),
            marker,
            listed: false,
        }
    };
    let text = |path: &Path| path.display().to_string();
    let (env, prelude) = match id {
        "P1" => (
            vec![
                ("CARGO_HOME".to_owned(), text(&cargo)),
                ("RUSTUP_HOME".to_owned(), text(&rustup)),
            ],
            None,
        ),
        "P2" => (
            Vec::new(),
            Some(format!(
                "printf 'CARGO_HOME=%s\\nRUSTUP_HOME=%s\\n' '{}' '{}' >> \"$GITHUB_ENV\"",
                text(&cargo),
                text(&rustup)
            )),
        ),
        "P3" => (vec![("HOME".to_owned(), text(&planted))], None),
        _ => (Vec::new(), None),
    };
    Row {
        id,
        plants: vec![
            program("rustup", true),
            program("rustc", true),
            program("cargo", false),
            Plant::plain(rustup.join("settings.toml"), "", false),
        ],
        env,
        config: Some((RUST_CONFIG.to_owned(), RUST_LOCK.to_owned())),
        prepare: Some(Box::new(|world, _| world.install_store_homes())),
        control: Some(homes_control(cargo, rustup)),
        prelude,
        script: Some(RUST_SCRIPT.to_owned()),
        verify: Some(homes_verify(bin)),
        ..Row::default()
    }
}

pub(crate) fn state_row(world: &World, id: &'static str) -> Option<Row> {
    Some(match id {
        "S1" | "S2" => Row {
            id,
            plants: vec![worktree_plant(world, id)],
            prepare: Some(hide_edit(if id == "S1" {
                "--skip-worktree"
            } else {
                "--assume-unchanged"
            })),
            ..Row::default()
        },
        "S3" => fsmonitor_row(world, id),
        "P1" | "P2" | "P3" | "P4" => homes_row(world, id),
        _ => return None,
    })
}
