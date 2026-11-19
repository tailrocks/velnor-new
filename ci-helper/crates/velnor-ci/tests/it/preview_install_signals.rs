// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::time::{Duration, Instant};

use nix::sys::signal::{SigSet, Signal};
use serde_json::json;
use velnor_ci::io::preview::install::install;
use velnor_ci::preview::attest::Authority;
use velnor_ci::preview::release::Release;
use velnor_ci::preview::runtime::{Host, Verified};

use crate::support::fresh_dir;

fn verified() -> Verified {
    Verified {
        preview: json!({}),
        authority: Authority::pinned().unwrap_or_else(|e| unreachable!("{e}")),
        release: Release {
            receipt: b"{}".to_vec(),
            bundle: b"{}".to_vec(),
            linux: b"linux".to_vec(),
            darwin: b"darwin".to_vec(),
        },
        host: Host::Linux,
    }
}

fn held() -> Vec<Signal> {
    let mask = SigSet::thread_get_mask().unwrap_or_else(|e| unreachable!("{e}"));
    [Signal::SIGINT, Signal::SIGTERM, Signal::SIGALRM]
        .into_iter()
        .filter(|signal| mask.contains(*signal))
        .collect()
}

fn root(name: &str) -> std::path::PathBuf {
    let dir = fresh_dir("velnor-ci-preview", name);
    fs::canonicalize(dir).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn termination_signals_are_held_from_the_rename_through_the_output_handoff() {
    let temp = root("install-signals-held");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut during = Vec::new();
    let installed = install(&temp, &verified(), deadline, || {
        during = held();
        Ok(())
    });
    assert!(installed.is_ok());
    assert_eq!(
        during,
        [Signal::SIGINT, Signal::SIGTERM, Signal::SIGALRM],
        "the handoff must hold SIGINT, SIGTERM and SIGALRM like the legacy install"
    );
    assert!(held().is_empty(), "the mask is restored afterwards");
}

#[test]
fn a_failed_handoff_restores_the_mask_and_removes_the_install() {
    let temp = root("install-signals-failed");
    let deadline = Instant::now() + Duration::from_secs(60);
    let installed = install(&temp, &verified(), deadline, || {
        Err(velnor_ci::error::CiError::new("emit"))
    });
    assert!(installed.is_err());
    assert!(held().is_empty());
    assert!(!temp.join("source-preview-runtime").exists());
}
