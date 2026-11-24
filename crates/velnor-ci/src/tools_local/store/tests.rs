// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{Scope, ScopedJoinHandle};
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
        aged(&self.entry(key), hours);
    }

    fn with_a_killed_jobs_lock(&self) {
        self.saved_aged("k1", 3);
        self.saved_aged("k2", 2);
        let lock = self.entry(".lock-k1");
        fs::create_dir(&lock).expect("leave a killed job's lock");
        aged(&lock, 2);
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

fn aged(path: &Path, hours: u64) {
    fs::File::open(path)
        .and_then(|dir| dir.set_modified(SystemTime::now() - Duration::from_hours(hours)))
        .expect("age the directory");
}

fn quiet(saved: &Outcome) -> bool {
    matches!(saved, Outcome::Saved { kept, .. } if kept.is_empty())
}

fn at(step: Step, pause: &dyn Fn()) -> impl Fn(Step) + '_ {
    move |reached| {
        if reached == step {
            pause();
        }
    }
}

struct Paused<'scope, T> {
    resume: mpsc::Sender<()>,
    worker: ScopedJoinHandle<'scope, T>,
}

impl<T> Paused<'_, T> {
    fn finish(self) -> T {
        drop(self.resume);
        self.worker.join().expect("the paused operation panicked")
    }
}

fn started<'scope, T: Send + std::fmt::Debug + 'scope>(
    scope: &'scope Scope<'scope, '_>,
    operation: impl FnOnce(&dyn Fn()) -> T + Send + 'scope,
) -> (bool, Paused<'scope, T>) {
    let (reached, at_pause) = mpsc::channel();
    let (resume, resumed) = mpsc::channel::<()>();
    let worker = scope.spawn(move || {
        let first = AtomicBool::new(true);
        operation(&|| {
            if first.swap(false, Ordering::SeqCst) {
                reached.send(()).ok();
                resumed.recv().ok();
            }
        })
    });
    (at_pause.recv().is_ok(), Paused { resume, worker })
}

fn paused<'scope, T: Send + std::fmt::Debug + 'scope>(
    scope: &'scope Scope<'scope, '_>,
    operation: impl FnOnce(&dyn Fn()) -> T + Send + 'scope,
) -> Paused<'scope, T> {
    let (reached, operation) = started(scope, operation);
    assert!(reached, "ended before its pause: {:?}", operation.finish());
    operation
}

fn interleave<T: Send + std::fmt::Debug>(
    operation: impl FnOnce(&dyn Fn()) -> T + Send,
    meanwhile: impl FnOnce(),
) -> T {
    std::thread::scope(|scope| {
        let operation = paused(scope, operation);
        meanwhile();
        operation.finish()
    })
}

fn heal_k1_seeing_the_stale_lock_while_a_save_prunes_k1(
    node: &Node,
    meanwhile: impl FnOnce(),
) -> (Outcome, Outcome) {
    std::thread::scope(|scope| {
        let heal = paused(scope, |pause| {
            node.save("k1", true, &at(Step::Breaking, pause))
        });
        meanwhile();
        let prune = paused(scope, |pause| {
            node.save("k3", false, &at(Step::Retiring, pause))
        });
        (heal.finish(), prune.finish())
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

#[test]
fn two_jobs_that_find_the_same_stale_lock_do_not_both_take_the_key() {
    let node = node("stale-lock");
    node.with_a_killed_jobs_lock();
    let (healed, saved) = heal_k1_seeing_the_stale_lock_while_a_save_prunes_k1(&node, || {});
    let listed = node.listed();
    assert_eq!(
        healed,
        Outcome::Busy,
        "both held k1, the store lists {listed:?}"
    );
    assert!(quiet(&saved), "{saved:?}");
    assert_eq!(listed, ["k2", "k3"]);
}

#[test]
fn a_job_that_saw_a_stale_lock_does_not_take_the_key_after_the_lock_moved() {
    let node = node("moved-lock");
    node.with_a_killed_jobs_lock();
    let grave = node.entry(".staging-k1-0123456789abcdef");
    let (healed, saved) = heal_k1_seeing_the_stale_lock_while_a_save_prunes_k1(&node, || {
        fs::rename(node.entry(".lock-k1"), &grave).expect("move the lock to a grave");
        aged(&grave, 0);
    });
    let listed = node.listed();
    assert_eq!(
        healed,
        Outcome::Busy,
        "both held k1, the store lists {listed:?}"
    );
    assert!(quiet(&saved), "{saved:?}");
    assert_eq!(listed, [".staging-k1-0123456789abcdef", "k2", "k3"]);
}

#[test]
fn a_save_that_saw_a_stale_lock_does_not_remove_the_lock_another_job_took_since() {
    let node = node("swept-lock");
    node.with_a_killed_jobs_lock();
    let (first, healed, second) = std::thread::scope(|scope| {
        let sweep = paused(scope, |pause| {
            node.save("k3", false, &at(Step::Breaking, pause))
        });
        let prune = paused(scope, |pause| {
            node.save("k4", false, &at(Step::Retiring, pause))
        });
        let first = sweep.finish();
        aged(&node.entry("k4"), 1);
        let healed = node.save("k1", true, &|_| {});
        (first, healed, prune.finish())
    });
    let listed = node.listed();
    assert_eq!(
        healed,
        Outcome::Busy,
        "both held k1, the store lists {listed:?}"
    );
    assert!(quiet(&first) && quiet(&second), "{first:?} {second:?}");
    assert_eq!(listed, ["k3", "k4"]);
}

#[test]
fn a_lock_whose_taker_was_killed_is_taken_over_once_it_is_stale_again() {
    let node = node("killed-taker");
    node.with_a_killed_jobs_lock();
    let killed = catch_unwind(AssertUnwindSafe(|| {
        node.save("k1", true, &|step| {
            assert_ne!(step, Step::Taken, "killed while taking the lock over");
        })
    }));
    assert!(killed.is_err(), "{killed:?}");
    aged(&node.entry(".lock-k1"), 2);
    let healed = node.save("k1", true, &|_| {});
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    let restored = node.restore("k1", &|| {});
    assert!(restored.hit(), "{restored:?}");
    assert_eq!(node.listed(), ["k1", "k2"]);
}

#[test]
fn a_job_that_took_over_a_stale_lock_keeps_the_key_until_it_moved_the_lock() {
    let node = node("taken-lock");
    node.with_a_killed_jobs_lock();
    let (healed, saved) = std::thread::scope(|scope| {
        let heal = paused(scope, |pause| {
            node.save("k1", true, &at(Step::Taken, pause))
        });
        let (_, prune) = started(scope, |pause| {
            node.save("k3", false, &at(Step::Retiring, pause))
        });
        (heal.finish(), prune.finish())
    });
    let listed = node.listed();
    assert!(matches!(healed, Outcome::Healed { .. }), "{healed:?}");
    assert!(quiet(&saved), "{saved:?}");
    let restored = node.restore("k1", &|| {});
    assert!(restored.hit(), "{restored:?}, the store lists {listed:?}");
    assert_eq!(listed, ["k1", "k3"]);
}
