// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use super::{Step, restore_with, save_with};
use crate::tools_local::{Inputs, Outcome, Verb};

const RUSTC: &str = ".rustup/toolchains/stable/rustc";
const COSIGN: &str = ".local/share/mise/installs/cosign/bin/cosign";

struct Node {
    root: PathBuf,
    store: String,
    owner: u32,
}

impl Drop for Node {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.root));
    }
}

fn node(name: &str) -> Node {
    let root =
        std::env::temp_dir().join(format!("velnor-tools-race-{name}-{}", std::process::id()));
    drop(fs::remove_dir_all(&root));
    for dir in ["store", "fresh", "home"] {
        fs::create_dir_all(root.join(dir)).expect("create the node");
    }
    let root = fs::canonicalize(&root).expect("canonical node");
    for (file, text) in [(RUSTC, "rustc"), (COSIGN, "cosign")] {
        let path = root.join("home").join(file);
        fs::create_dir_all(path.parent().expect("a parent")).expect("create the tool");
        fs::write(path, text).expect("write the tool");
    }
    let owner = fs::metadata(&root).expect("node metadata").uid();
    let store = root.join("store").display().to_string();
    Node { root, store, owner }
}

impl Node {
    fn inputs<'a>(&'a self, verb: Verb, key: &'a str, home: &'a Path, replace: bool) -> Inputs<'a> {
        Inputs {
            verb,
            key,
            event_name: "push",
            git_ref: "refs/heads/main",
            default_branch: Some("main"),
            shared_dir: Some(&self.store),
            seed_dir: Some(&self.store),
            home,
            replace,
        }
    }

    fn save(&self, key: &str, replace: bool, pause: &dyn Fn(Step)) -> Outcome {
        let home = self.root.join("home");
        save_with(
            &self.inputs(Verb::Save, key, &home, replace),
            self.owner,
            pause,
        )
    }

    fn restore(&self, key: &str, copied: &dyn Fn()) -> Outcome {
        let home = self.root.join("fresh");
        let inputs = self.inputs(Verb::Restore, key, &home, false);
        restore_with(&inputs, Some(&self.store), false, self.owner, copied)
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.root.join("store/tools").join(key)
    }

    fn saved_aged(&self, key: &str, hours: u64) {
        let saved = self.save(key, false, &|_| {});
        assert!(matches!(saved, Outcome::Saved { .. }), "{saved:?}");
        fs::File::open(self.entry(key))
            .and_then(|dir| dir.set_modified(SystemTime::now() - Duration::from_hours(hours)))
            .expect("age the entry");
    }

    fn listed(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.root.join("store/tools"))
            .expect("list the store")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

fn at(step: Step, pause: &dyn Fn()) -> impl Fn(Step) + '_ {
    move |reached| {
        if reached == step {
            pause();
        }
    }
}

fn interleave<T: Send + std::fmt::Debug>(
    paused: impl FnOnce(&dyn Fn()) -> T + Send,
    meanwhile: impl FnOnce(),
) -> T {
    std::thread::scope(|scope| {
        let (reached, at_pause) = mpsc::channel();
        let (resume, resumed) = mpsc::channel::<()>();
        let worker = scope.spawn(move || {
            let first = AtomicBool::new(true);
            paused(&|| {
                if first.swap(false, Ordering::SeqCst) {
                    reached.send(()).ok();
                    resumed.recv().ok();
                }
            })
        });
        assert!(
            at_pause.recv().is_ok(),
            "ended before its pause: {:?}",
            worker.join()
        );
        meanwhile();
        drop(resume);
        worker.join().expect("the paused operation panicked")
    })
}

#[test]
fn a_save_does_not_prune_an_entry_a_restore_is_copying_and_a_later_save_does() {
    let node = node("restore-pruned");
    node.saved_aged("k1", 3);
    node.saved_aged("k2", 2);
    let restored = interleave(
        |pause| node.restore("k1", pause),
        || {
            let saved = node.save("k3", false, &|_| {});
            let quiet = matches!(&saved, Outcome::Saved { kept, .. } if kept.is_empty());
            assert!(quiet, "{saved:?}");
        },
    );
    assert!(restored.hit(), "{restored:?}");
    let rustc = fs::read_to_string(node.root.join("fresh").join(RUSTC));
    assert_eq!(rustc.ok().as_deref(), Some("rustc"));
    assert_eq!(node.listed(), ["k1", "k2", "k3"]);
    let saved = node.save("k4", false, &|_| {});
    assert!(matches!(saved, Outcome::Saved { .. }), "{saved:?}");
    assert_eq!(node.listed(), ["k3", "k4"]);
}

#[test]
fn a_prune_does_not_remove_the_fresh_entry_a_replacing_save_published_after_the_prune_chose_it() {
    let node = node("prune-replaced");
    node.saved_aged("k1", 3);
    node.saved_aged("k2", 2);
    let saved = interleave(
        |pause| node.save("k3", false, &at(Step::Chosen, pause)),
        || {
            let healed = node.save("k1", true, &|_| {});
            assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
        },
    );
    assert!(matches!(saved, Outcome::Saved { .. }), "{saved:?}");
    assert_eq!(node.listed(), ["k1", "k3"]);
    let restored = node.restore("k1", &|| {});
    assert!(restored.hit(), "{restored:?}");
}

#[test]
fn a_restore_whose_entry_is_replaced_during_the_copy_is_not_a_hit() {
    let node = node("restore-replaced");
    node.saved_aged("k1", 0);
    fs::write(
        node.entry("k1")
            .join(".local_share_mise/installs/cosign/bin/cosign"),
        "evil",
    )
    .expect("poison the entry");
    let restored = interleave(
        |pause| node.restore("k1", pause),
        || {
            let healed = node.save("k1", true, &|_| {});
            assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
        },
    );
    assert!(!restored.hit(), "{restored:?}");
}
