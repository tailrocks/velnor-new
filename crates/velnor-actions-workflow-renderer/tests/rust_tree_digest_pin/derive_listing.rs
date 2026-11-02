// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_actions_workflow_renderer::verifier_builders::TreeDigestPin;

use crate::derive::{list_files, outcome, printed};
use crate::fixture::{
    CAP, CHANNEL_MANIFEST, FIXTURE_BOOKKEEPING, Fixture, MALFORMED_HEADERS, MULTIRUST_CONFIG,
    Outcome, PHASES, TARGET, VERSION, script,
};

type Env<'a> = &'a [(&'a str, &'a str)];

const NO_ENV: Env<'static> = &[];
const BLOCK_ENVS: [Env<'static>; 3] = [NO_ENV, &[("BLOCK_SIZE", "1M")], &[("LS_BLOCK_SIZE", "1M")]];

fn derive(cwd: &Path, tree: &OsStr, listing: Option<&OsStr>, env: Env<'_>) -> Outcome {
    let mut command = Command::new(crate::derive_tool());
    command.current_dir(cwd).envs(env.iter().copied());
    if let Some(listing) = listing {
        command.arg("--listing").arg(listing);
    }
    outcome(
        &command
            .arg("--tree")
            .arg(tree)
            .output()
            .expect("run derive tool"),
    )
}

fn derive_tree(fx: &Fixture, listing: &Path, env: Env<'_>) -> Outcome {
    derive(
        &fx.toolchains(),
        fx.root().as_os_str(),
        Some(listing.as_os_str()),
        env,
    )
}

fn tree_with(path: &str, bytes: &[u8]) -> Fixture {
    let fx = Fixture::standard();
    fx.put(path, bytes, 0o644);
    fx
}

fn failed_closed(outcome: &Outcome) -> bool {
    outcome.code.is_some_and(|code| code != 0)
        && !outcome.stdout.contains("tree_sha256=")
        && outcome.stderr.starts_with("derive-rust-tree-digest:")
}

fn tree_digest(outcome: &Outcome) -> String {
    outcome
        .stdout
        .lines()
        .find(|line| line.starts_with("tree_sha256="))
        .unwrap_or_else(|| panic!("no tree_sha256 in {outcome:?}"))
        .to_owned()
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut names = Vec::new();
    list_files(root, "", &mut names);
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let bytes = fs::read(root.join(&name)).expect("read tree file");
            (name, bytes)
        })
        .collect()
}

#[test]
fn n1d_derive_rejects_a_malformed_section_header_and_writes_no_listing() {
    let dir = tempfile::tempdir().expect("listing dir");
    let listing = dir.path().join("listing");
    let malformed =
        format!("{FIXTURE_BOOKKEEPING}\n[[artifacts.foo]\nurl = \"https://attacker.example/x\"\n");
    let outcome = derive_tree(
        &tree_with(CHANNEL_MANIFEST, malformed.as_bytes()),
        &listing,
        NO_ENV,
    );
    assert!(failed_closed(&outcome) && !listing.exists(), "{outcome:?}");
}

#[test]
fn n1e_derive_rejects_a_header_outside_the_dotted_key_grammar_and_accepts_every_real_header_form() {
    let dir = tempfile::tempdir().expect("listing dir");
    let mut failures = Vec::new();
    for (index, header) in MALFORMED_HEADERS.iter().enumerate() {
        let listing = dir.path().join(format!("listing-{index}"));
        let manifest =
            format!("{FIXTURE_BOOKKEEPING}\n{header}\nurl = \"https://attacker.example/x\"\n");
        let outcome = derive_tree(
            &tree_with(CHANNEL_MANIFEST, manifest.as_bytes()),
            &listing,
            NO_ENV,
        );
        if !failed_closed(&outcome) || listing.exists() {
            failures.push(format!("{header}: {outcome:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");

    let real = format!("{FIXTURE_BOOKKEEPING}\n{REAL_HEADER_FORMS}");
    let fx = tree_with(CHANNEL_MANIFEST, real.as_bytes());
    let derived = derive_tree(&fx, &dir.path().join("real-listing"), NO_ENV);
    assert_eq!(derived.code, Some(0), "{derived:?}");
    let pin = TreeDigestPin::new(
        VERSION,
        TARGET,
        printed(&derived, "components_sha256"),
        printed(&derived, "tree_sha256"),
    );
    for phase in PHASES {
        fx.run(&script(phase, &[pin])).assert_pinned();
    }
}

const REAL_HEADER_FORMS: &str = "[[artifacts.installer-msi.target.aarch64-pc-windows-gnullvm]]\nurl = \"https://static.rust-lang.org/dist/x.msi\"\n\n[[artifacts.source-code.target.\"*\"]]\nurl = \"https://static.rust-lang.org/dist/x.tar.gz\"\n\n[pkg.rust-src.target.\"*\"]\navailable = true\n\n[pkg.rust-std.target.\"thumbv8m.base-none-eabi\"]\navailable = false\n\n[[pkg.rust.target.aarch64-apple-darwin.components]]\npkg = \"rust-std\"\ntarget = \"aarch64-apple-darwin\"\n";

#[test]
fn n9_an_over_cap_bookkeeping_file_fails_closed_without_a_partial_listing() {
    let dir = tempfile::tempdir().expect("listing dir");
    let listing = dir.path().join("listing");
    let mut padded = FIXTURE_BOOKKEEPING.to_owned();
    padded.push_str(&"\n".repeat(CAP + 1 - padded.len()));
    let rows = [
        ("multirust config", MULTIRUST_CONFIG, vec![b'x'; CAP + 1]),
        ("channel manifest", CHANNEL_MANIFEST, padded.into_bytes()),
    ];
    let mut failures = Vec::new();
    for env in BLOCK_ENVS {
        let control = derive_tree(
            &tree_with(MULTIRUST_CONFIG, &vec![b'x'; CAP]),
            &listing,
            env,
        );
        if control.code != Some(0) || !control.stdout.contains("tree_sha256=") || !listing.exists()
        {
            failures.push(format!("control {env:?}: {control:?}"));
        }
        if listing.exists() {
            fs::remove_file(&listing).expect("remove listing");
        }
        for (name, path, bytes) in &rows {
            let outcome = derive_tree(&tree_with(path, bytes), &listing, env);
            if !failed_closed(&outcome) || listing.exists() {
                failures.push(format!("{name} {env:?}: {outcome:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

struct Refusal {
    name: &'static str,
    cwd: PathBuf,
    tree: PathBuf,
    listing: PathBuf,
}

#[test]
fn n11_a_listing_path_inside_the_tree_is_refused_before_anything_is_scanned_or_written() {
    let fx = Fixture::standard();
    fx.put("lib/prior.listing", b"earlier run\n", 0o644);
    let root = fx.root();
    let alias = fx.toolchains().join("alias");
    symlink(&root, &alias).expect("alias");
    let name = root.file_name().expect("root name").to_owned();
    let rows = [
        Refusal {
            name: "absolute",
            cwd: fx.toolchains(),
            tree: root.clone(),
            listing: root.join("out.listing"),
        },
        Refusal {
            name: "dot-dot",
            cwd: fx.toolchains(),
            tree: root.clone(),
            listing: root.join("lib/../out.listing"),
        },
        Refusal {
            name: "symlink alias",
            cwd: fx.toolchains(),
            tree: alias,
            listing: root.join("out.listing"),
        },
        Refusal {
            name: "relative",
            cwd: fx.toolchains(),
            tree: PathBuf::from(&name),
            listing: Path::new(&name).join("out.listing"),
        },
        Refusal {
            name: "existing file",
            cwd: fx.toolchains(),
            tree: root.clone(),
            listing: root.join("lib/prior.listing"),
        },
    ];
    let before = snapshot(&root);
    let failures = rows
        .iter()
        .filter_map(|row| {
            let outcome = derive(
                &row.cwd,
                row.tree.as_os_str(),
                Some(row.listing.as_os_str()),
                NO_ENV,
            );
            (!failed_closed(&outcome) || snapshot(&root) != before)
                .then(|| format!("{}: {outcome:?}", row.name))
        })
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn n11b_a_listing_path_that_resolves_into_the_tree_is_refused_and_leaves_the_tree_and_its_digest_unchanged()
 {
    type Aim = fn(&Path, &Path) -> PathBuf;
    let rows: [(&str, Aim); 4] = [
        ("tree itself", |root, _| root.to_path_buf()),
        ("tree with trailing slash", |root, _| {
            PathBuf::from(format!("{}/", root.display()))
        }),
        ("outside symlink to a file in the tree", |root, outside| {
            let link = outside.join("link");
            symlink(root.join("lib/prior.listing"), &link).expect("file link");
            link
        }),
        ("outside symlink to the tree", |root, outside| {
            let link = outside.join("link");
            symlink(root, &link).expect("tree link");
            link
        }),
    ];
    let failures = rows
        .iter()
        .filter_map(|(name, aim)| {
            let fx = Fixture::standard();
            fx.put("lib/prior.listing", b"earlier run\n", 0o644);
            let root = fx.root();
            let outside = tempfile::tempdir().expect("outside dir");
            let listing = aim(&root, outside.path());
            let before = snapshot(&root);
            let plain = derive(&fx.toolchains(), root.as_os_str(), None, NO_ENV);
            let outcome = derive(
                &fx.toolchains(),
                root.as_os_str(),
                Some(listing.as_os_str()),
                NO_ENV,
            );
            let after = derive(&fx.toolchains(), root.as_os_str(), None, NO_ENV);
            (!failed_closed(&outcome)
                || snapshot(&root) != before
                || tree_digest(&after) != tree_digest(&plain))
            .then(|| format!("{name}: {outcome:?}"))
        })
        .collect::<Vec<_>>();
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn n11_control_a_listing_outside_the_tree_is_written_and_leaves_the_digest_stable() {
    let fx = Fixture::standard();
    let dir = tempfile::tempdir().expect("listing dir");
    let listing = dir.path().join("out.listing");
    let plain = derive(&fx.toolchains(), fx.root().as_os_str(), None, NO_ENV);
    let first = derive_tree(&fx, &listing, NO_ENV);
    let second = derive_tree(&fx, &listing, NO_ENV);
    assert!(listing.exists(), "{first:?}");
    assert_eq!(tree_digest(&first), tree_digest(&plain), "{first:?}");
    assert_eq!(tree_digest(&second), tree_digest(&plain), "{second:?}");
}
