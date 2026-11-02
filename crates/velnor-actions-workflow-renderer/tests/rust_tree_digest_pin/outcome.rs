// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(
    not(target_os = "linux"),
    expect(
        dead_code,
        reason = "the pinned-listing helpers serve Linux-only tests"
    )
)]

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use crate::fixture::CHANNEL_URL;

#[derive(Debug)]
pub(crate) struct Outcome {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) requests: Vec<String>,
}

impl Outcome {
    pub(crate) fn combined(&self) -> String {
        format!("{}\n{}", self.stdout, self.stderr)
    }

    pub(crate) fn is_pinned(&self) -> bool {
        self.code == Some(0) && self.stdout.contains("matches pinned")
    }

    pub(crate) fn is_not_pinned(&self) -> bool {
        self.code != Some(0) && !self.combined().contains("matches pinned")
    }

    pub(crate) fn fell_through(&self) -> bool {
        self.is_not_pinned()
            && self.combined().contains("pin_mismatch")
            && self.requests.iter().any(|url| url == CHANNEL_URL)
    }

    pub(crate) fn hard_failed(&self, code: &str) -> bool {
        let combined = self.combined();
        self.code.is_some_and(|status| status != 0)
            && self.stderr.contains(code)
            && !combined.contains("matches pinned")
            && !combined.contains("verified file by file")
    }

    pub(crate) fn no_pin(&self) -> bool {
        self.fell_through()
            && self
                .stdout
                .contains("::notice::pin_mismatch: no_pin components=")
    }

    pub(crate) fn assert_pinned(&self) {
        assert!(self.is_pinned(), "{self:?}");
    }

    pub(crate) fn assert_fell_through(&self) {
        assert!(self.fell_through(), "{self:?}");
    }

    pub(crate) fn assert_hard_failed(&self, code: &str) {
        assert!(self.hard_failed(code), "{self:?}");
    }

    pub(crate) fn assert_offline_pinned(&self) {
        assert!(self.is_pinned() && self.requests.is_empty(), "{self:?}");
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Node {
    path: String,
    kind: char,
    mode: u32,
    bytes: Vec<u8>,
}

impl Node {
    pub(crate) fn path(&self) -> &str {
        &self.path
    }
}

pub(crate) fn collect(root: &Path, dir: &Path, out: &mut Vec<Node>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        let meta = fs::symlink_metadata(&path).expect("stat entry");
        let rel = path
            .strip_prefix(root)
            .expect("under root")
            .to_string_lossy()
            .into_owned();
        let kind = if meta.is_dir() {
            'd'
        } else if meta.is_file() {
            'f'
        } else {
            'l'
        };
        let bytes = if kind == 'f' {
            fs::read(&path).expect("read file")
        } else {
            Vec::new()
        };
        out.push(Node {
            path: rel,
            kind,
            mode: meta.mode() & 0o7777,
            bytes,
        });
        if kind == 'd' {
            collect(root, &path, out);
        }
    }
}
