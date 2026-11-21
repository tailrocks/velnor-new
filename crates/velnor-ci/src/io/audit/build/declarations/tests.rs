// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;

use nix::sys::resource::{Resource, getrlimit, setrlimit};

use super::*;

const ROOM: u64 = 64;

fn exhaust_descriptors() -> Vec<File> {
    let mut held = Vec::new();
    loop {
        match File::open("/dev/null") {
            Ok(file) => held.push(file),
            Err(error) => {
                assert_eq!(error.raw_os_error(), Some(libc::EMFILE), "{error}");
                return held;
            }
        }
    }
}

#[test]
fn a_descriptor_shortage_while_matching_a_recorded_inventory_warns_and_distrusts_the_tree() {
    let directory = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-matching-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    let targets = BTreeSet::new();
    let mut pass = Pass::new(usize::MAX);
    pass.views.insert(
        (directory.clone(), targets.clone()),
        inventory::Entries::new(),
    );
    let (soft, hard) = getrlimit(Resource::RLIMIT_NOFILE).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        hard >= ROOM,
        "the hard open-file limit {hard} is below {ROOM}"
    );
    setrlimit(Resource::RLIMIT_NOFILE, ROOM, hard).unwrap_or_else(|e| unreachable!("{e}"));
    let held = exhaust_descriptors();
    let trusted = trusted(
        &directory.join("output"),
        &[],
        std::slice::from_ref(&directory),
        &targets,
        &mut pass,
    );
    drop(held);
    setrlimit(Resource::RLIMIT_NOFILE, soft, hard).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(trusted, Ok(false)), "{trusted:?}");
    assert_eq!(pass.budget.warnings_emitted(), 1);
}
