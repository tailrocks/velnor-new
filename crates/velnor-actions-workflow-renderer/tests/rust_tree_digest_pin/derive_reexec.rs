// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::derive::{Bundle, TIMED_OUT, TIMEOUT_SECONDS};
use crate::fixture::{Outcome, TARGET, VERSION};
use crate::release::COMPONENT;

const SENTINEL: &str = "--velnor-clean-reexec";

struct Hook {
    dir: TempDir,
    tar_options: String,
}

impl Hook {
    fn armed() -> Self {
        let dir = tempfile::tempdir().expect("hook dir");
        std::fs::create_dir(dir.path().join("scripts")).expect("cdpath bait");
        let hook = dir.path().join("hook");
        let ran = dir.path().join("ran");
        std::fs::write(
            &hook,
            format!("#!/bin/sh\nprintf hook >> {}\n", ran.display()),
        )
        .expect("write hook");
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let tar_options = format!("--checkpoint=1 --checkpoint-action=exec={}", hook.display());
        let probe = dir.path().join("probe.tar");
        let pack = |flag: &str, path: &Path| {
            let mut tar = Command::new("tar");
            tar.args([flag.as_ref(), path.as_os_str()]);
            tar
        };
        let packed = pack("-cf", &probe)
            .args(["-C".as_ref(), dir.path().as_os_str(), "hook".as_ref()])
            .status()
            .expect("pack probe");
        assert!(packed.success());
        let listed = pack("-tf", &probe)
            .env("TAR_OPTIONS", &tar_options)
            .output()
            .expect("list probe");
        assert!(listed.status.success());
        let armed = Self { dir, tar_options };
        assert!(armed.fired(), "the hook never fired on a plain tar");
        std::fs::remove_file(ran).expect("reset probe");
        armed
    }

    fn fired(&self) -> bool {
        std::fs::read_to_string(self.dir.path().join("ran"))
            .is_ok_and(|ran| ran.starts_with("hook"))
    }

    fn reset(&self) {
        std::fs::remove_file(self.dir.path().join("ran")).ok();
    }

    fn path(&self) -> &str {
        self.dir.path().to_str().expect("utf-8 temp dir")
    }
}

fn manifest_args(sentinel: bool) -> Vec<&'static str> {
    sentinel
        .then_some(SENTINEL)
        .into_iter()
        .chain(["--from-manifest", VERSION, TARGET, COMPONENT])
        .collect()
}

struct Runs<'a> {
    bundle: &'a Bundle,
    seen: usize,
}

struct Seen {
    outcome: Outcome,
    new_requests: usize,
}

impl Runs<'_> {
    fn relative(&mut self, sentinel: bool, env: &[(&str, &str)]) -> Seen {
        let outcome = self.bundle.run_relative(&manifest_args(sentinel), env);
        let new_requests = outcome.requests.len() - self.seen;
        self.seen = outcome.requests.len();
        Seen {
            outcome,
            new_requests,
        }
    }
}

impl Seen {
    fn derived_like(&self, clean: &Outcome) -> bool {
        self.outcome.code == Some(0) && self.outcome.stdout == clean.stdout
    }

    fn refused_as_usage(&self) -> bool {
        self.outcome.code == Some(2) && self.outcome.stdout.is_empty() && self.new_requests == 0
    }

    fn brief(&self) -> String {
        let digests = self.outcome.stdout.lines().take(2).collect::<Vec<_>>();
        let timed_out = self.outcome.code == Some(TIMED_OUT);
        format!(
            "timed out {timed_out}, code {:?}, {digests:?}, stderr {:?}, new requests {}",
            self.outcome.code, self.outcome.stderr, self.new_requests
        )
    }
}

#[test]
fn r9c_a_caller_supplied_reexec_argument_cannot_skip_the_clean_environment() {
    let hook = Hook::armed();
    let rows: [(&str, Vec<(&str, &str)>); 5] = [
        ("TAR_OPTIONS hook", vec![("TAR_OPTIONS", &hook.tar_options)]),
        ("XZ_DEFAULTS", vec![("XZ_DEFAULTS", "--memlimit=1B")]),
        ("TMPDIR", vec![("TMPDIR", "/nonexistent")]),
        ("CDPATH", vec![("CDPATH", hook.path())]),
        (
            "all four",
            vec![
                ("TAR_OPTIONS", &hook.tar_options),
                ("XZ_DEFAULTS", "--memlimit=1B"),
                ("TMPDIR", "/nonexistent"),
                ("CDPATH", hook.path()),
            ],
        ),
    ];
    let bundle = Bundle::new(&|_| {});
    let mut runs = Runs {
        bundle: &bundle,
        seen: 0,
    };
    let clean = runs.relative(false, &[]);
    assert_eq!(clean.outcome.code, Some(0), "{}", clean.brief());
    let mut wrong = Vec::new();
    for (name, env) in rows {
        let control = runs.relative(false, &env);
        if hook.fired() || !control.derived_like(&clean.outcome) {
            wrong.push(format!("{name}, no argument: {}", control.brief()));
        }
        hook.reset();
        let run = runs.relative(true, &env);
        if hook.fired() || !(run.derived_like(&clean.outcome) || run.refused_as_usage()) {
            wrong.push(format!(
                "{name}, {SENTINEL}: hook fired {}: {}",
                hook.fired(),
                run.brief()
            ));
        }
        hook.reset();
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

const ALLOWLISTED: [(&str, &str); 18] = [
    ("LC_ALL", "C"),
    ("HOME", "/nonexistent-home"),
    ("RUNNER_TEMP", "/tmp"),
    ("RUSTUP_HOME", "/nonexistent-rustup"),
    ("MISE_RUSTUP_HOME", "/nonexistent-mise-rustup"),
    ("CARGO_HOME", "/nonexistent-cargo"),
    ("VELNOR_TOOL_RUST", "/nonexistent-tool"),
    ("HTTPS_PROXY", "http://127.0.0.1:9"),
    ("https_proxy", "http://127.0.0.1:9"),
    ("HTTP_PROXY", "http://127.0.0.1:9"),
    ("http_proxy", "http://127.0.0.1:9"),
    ("NO_PROXY", "localhost"),
    ("no_proxy", "localhost"),
    ("ALL_PROXY", "http://127.0.0.1:9"),
    ("all_proxy", "http://127.0.0.1:9"),
    ("SSL_CERT_FILE", "/nonexistent-ca"),
    ("SSL_CERT_DIR", "/nonexistent-ca-dir"),
    ("CURL_CA_BUNDLE", "/nonexistent-ca"),
];

#[test]
fn r9d_the_reexec_guard_terminates_and_derives_for_every_kind_of_caller_environment() {
    let bundle = Bundle::new(&|_| {});
    let rows: [(&str, &[(&str, &str)]); 3] = [
        ("no variables but PATH", &[]),
        ("one unlisted variable", &[("FOO", "1")]),
        ("every allowlisted variable", &ALLOWLISTED),
    ];
    let runs = rows.map(|(name, env)| (name, bundle.run_derive(&manifest_args(false), env)));
    let reference = &runs[0].1;
    let wrong = runs
        .iter()
        .filter_map(|(name, run)| {
            let verdict = if run.code == Some(TIMED_OUT) {
                format!("did not finish within {TIMEOUT_SECONDS} s")
            } else if run.code == Some(0) && run.stdout == reference.stdout {
                return None;
            } else {
                "wrong result".to_owned()
            };
            Some(format!(
                "{name}: {verdict}: code {:?}, stderr {:?}",
                run.code, run.stderr
            ))
        })
        .collect::<Vec<_>>();
    assert!(wrong.is_empty(), "{wrong:#?}");
}
