// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::tools_cache_install_run::{Install, Outcome, run_install};
use super::tools_cache_pin_fixture::{
    MACHETE_BINARY, MACHETE_DIR, MACHETE_VERSION, machete_archive, pins_with_fixture_sha,
};
use super::tools_cache_verify_hook::step_in;

const RESTORE_ACTION: &str = ".github/actions/velnor-tools-cache-restore/action.yml";
const REVERIFY_STEP: &str = "Re-verify restored Mise installs";
const MATCHES: &str = "matches its mise.lock archive";
const DISCARDING: &str = "discarding restored";

#[derive(Default)]
struct Case<'a> {
    dir: &'a str,
    version: &'a str,
    links: &'a [(&'a str, &'a str)],
    workspace_links: &'a [(&'a str, &'a str)],
    files: &'a [(&'a str, &'a [u8], bool)],
    home_prefix: &'a str,
}

fn run(case: &Case<'_>) -> Result<Outcome, Box<dyn Error>> {
    let step = step_in(RESTORE_ACTION, REVERIFY_STEP)?;
    let rendered = step
        .tool_pins
        .as_deref()
        .ok_or("restore step has no pins")?;
    let scratch = tempfile::tempdir()?;
    let (archive, sha256) = machete_archive(scratch.path(), MACHETE_BINARY)?;
    let pins = pins_with_fixture_sha(rendered, &sha256);
    run_install(
        &step,
        &Install {
            dir: case.dir,
            version: case.version,
            pins: &pins,
            lock: None,
            archive_sha256: &sha256,
            archive: &archive,
            extra_archives: &[],
            file: MACHETE_BINARY,
            links: case.links,
            workspace_links: case.workspace_links,
            files: case.files,
            home_prefix: case.home_prefix,
        },
        &[],
    )
}

fn assert_discarded(what: &str, case: &Case<'_>) -> Result<(), Box<dyn Error>> {
    let outcome = run(case)?;
    assert_eq!(
        (outcome.exit, outcome.kept),
        (Some(0), false),
        "{what}, stdout: {}",
        outcome.stdout
    );
    assert!(
        outcome.stdout.contains(DISCARDING),
        "{what}, stdout: {}",
        outcome.stdout
    );
    assert!(
        !outcome.stdout.contains(MATCHES),
        "{what}, stdout: {}",
        outcome.stdout
    );
    Ok(())
}

fn plain() -> Case<'static> {
    Case {
        dir: MACHETE_DIR,
        version: MACHETE_VERSION,
        ..Case::default()
    }
}

#[test]
fn newline_named_escaping_link_is_discarded() -> Result<(), Box<dyn Error>> {
    assert_discarded(
        "link named x<newline>y",
        &Case {
            links: &[("x\ny", "/etc/hostname")],
            ..plain()
        },
    )
}

#[test]
fn newline_split_pieces_that_are_real_links_do_not_hide_an_escaping_link()
-> Result<(), Box<dyn Error>> {
    assert_discarded(
        "link x plus link foo/bar inside directory x<newline>foo",
        &Case {
            links: &[("x", "file"), ("x\nfoo/bar", "/etc/hostname")],
            workspace_links: &[("foo/bar", "file")],
            ..plain()
        },
    )
}

#[test]
fn newline_named_link_to_a_file_inside_the_install_is_kept() -> Result<(), Box<dyn Error>> {
    let kept = run(&Case {
        links: &[("x\ny", "file")],
        ..plain()
    })?;
    assert_eq!((kept.exit, kept.kept), (Some(0), true), "{}", kept.stdout);
    assert!(kept.stdout.contains(MATCHES), "stdout: {}", kept.stdout);
    assert!(!kept.stdout.contains(DISCARDING), "stdout: {}", kept.stdout);
    Ok(())
}

#[test]
fn install_named_like_the_pin_through_awk_escapes_is_discarded() -> Result<(), Box<dyn Error>> {
    assert_discarded(
        "version directory 0.9.\\62",
        &Case {
            version: "0.9.\\62",
            ..plain()
        },
    )?;
    assert_discarded(
        "tool directory http-cargo-mach\\145te",
        &Case {
            dir: "http-cargo-mach\\145te",
            ..plain()
        },
    )
}

#[test]
fn exec_in_mise_bookkeeping_is_discarded_even_when_home_has_glob_characters()
-> Result<(), Box<dyn Error>> {
    let files: &[(&str, &[u8], bool)] = &[(".mise-x/run", b"#!/bin/sh\n", true)];
    assert_discarded(
        "executable in .mise-x under a plain home",
        &Case { files, ..plain() },
    )?;
    assert_discarded(
        "executable in .mise-x under a home named h[a-z]...",
        &Case {
            files,
            home_prefix: "h[a-z]",
            ..plain()
        },
    )
}
